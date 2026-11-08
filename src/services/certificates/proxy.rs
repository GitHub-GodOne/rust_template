//! Built-in HTTPS reverse proxy for "direct" domain mappings.
//!
//! Each public port gets one dual-stack (IPv4 + IPv6) TLS listener. The
//! certificate is picked by SNI and the request is forwarded to the mapped
//! local service over plain HTTP, including WebSocket/HTTP upgrades.

use crate::services::ip_rules::IpRule;

use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    sync::{Arc, OnceLock, RwLock},
    time::Duration,
};

use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::{
    body::{Bytes, Incoming},
    header::{self, HeaderMap, HeaderName, HeaderValue},
    service::service_fn,
    Request, Response, StatusCode, Version,
};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::{TokioExecutor, TokioIo},
    server::conn::auto,
};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
    ServerConfig,
};
use serde::Serialize;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_rustls::TlsAcceptor;

type ProxyBody = BoxBody<Bytes, hyper::Error>;

/// One `https://host:port` → `http://upstream_host:upstream_port` mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyRoute {
    pub port: u16,
    pub host: String,
    pub upstream_host: String,
    pub upstream_port: u16,
    /// Empty means every client IP is allowed.
    pub allowed_ips: Vec<IpRule>,
}

/// PEM files for the names a certificate covers.
#[derive(Clone, Debug)]
pub struct ProxyCertificate {
    pub names: Vec<String>,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct ListenerStatus {
    pub port: u16,
    pub running: bool,
    pub hosts: Vec<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Upstream {
    host: String,
    port: u16,
    allowed_ips: Vec<IpRule>,
}

#[derive(Default)]
struct ProxyTable {
    routes: HashMap<(u16, String), Upstream>,
    certificates: HashMap<String, Arc<CertifiedKey>>,
}

struct Listener {
    task: Option<JoinHandle<()>>,
    error: Option<String>,
}

fn table() -> &'static RwLock<ProxyTable> {
    static TABLE: OnceLock<RwLock<ProxyTable>> = OnceLock::new();
    TABLE.get_or_init(|| RwLock::new(ProxyTable::default()))
}

fn listeners() -> &'static tokio::sync::Mutex<HashMap<u16, Listener>> {
    static LISTENERS: OnceLock<tokio::sync::Mutex<HashMap<u16, Listener>>> = OnceLock::new();
    LISTENERS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

fn upstream_client() -> &'static Client<HttpConnector, Incoming> {
    static CLIENT: OnceLock<Client<HttpConnector, Incoming>> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let mut connector = HttpConnector::new();
        connector.set_nodelay(true);
        connector.set_connect_timeout(Some(Duration::from_secs(10)));
        Client::builder(TokioExecutor::new()).build(connector)
    })
}

/// Picks a certificate for an SNI name: exact match first, then `*.parent`.
fn certificate_for(
    certificates: &HashMap<String, Arc<CertifiedKey>>,
    name: &str,
) -> Option<Arc<CertifiedKey>> {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    certificates.get(&name).cloned().or_else(|| {
        name.split_once('.')
            .and_then(|(_, parent)| certificates.get(&format!("*.{parent}")).cloned())
    })
}

struct TableResolver;

impl fmt::Debug for TableResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TableResolver")
    }
}

impl ResolvesServerCert for TableResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let name = client_hello.server_name()?;
        let table = table().read().ok()?;
        certificate_for(&table.certificates, name)
    }
}

pub fn load_certified_key(certificate: &ProxyCertificate) -> Result<Arc<CertifiedKey>, String> {
    let chain = CertificateDer::pem_file_iter(&certificate.cert_path)
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| format!("读取证书 {} 失败：{error}", certificate.cert_path.display()))?;
    if chain.is_empty() {
        return Err(format!("{} 中没有证书", certificate.cert_path.display()));
    }
    let key = PrivateKeyDer::from_pem_file(&certificate.key_path)
        .map_err(|error| format!("读取私钥 {} 失败：{error}", certificate.key_path.display()))?;
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|error| format!("不支持的私钥类型：{error}"))?;
    Ok(Arc::new(CertifiedKey::new(chain, signing_key)))
}

/// Replaces the routing table and starts/stops listeners to match it.
/// Returns human-readable problems (missing certificates, bind failures).
pub async fn apply(routes: Vec<ProxyRoute>, certificates: &[ProxyCertificate]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut loaded = HashMap::new();
    for certificate in certificates {
        match load_certified_key(certificate) {
            Ok(key) => {
                for name in &certificate.names {
                    loaded.insert(name.to_ascii_lowercase(), key.clone());
                }
            }
            Err(error) => problems.push(error),
        }
    }

    let mut table_routes = HashMap::new();
    for route in routes {
        let host = route.host.to_ascii_lowercase();
        if certificate_for(&loaded, &host).is_none() {
            problems.push(format!("{host} 的证书尚未就绪，暂不监听"));
            continue;
        }
        table_routes.insert(
            (route.port, host),
            Upstream {
                host: route.upstream_host,
                port: route.upstream_port,
                allowed_ips: route.allowed_ips,
            },
        );
    }
    let ports: HashSet<u16> = table_routes.keys().map(|(port, _)| *port).collect();
    if let Ok(mut table) = table().write() {
        table.routes = table_routes;
        table.certificates = loaded;
    }

    let mut active = listeners().lock().await;
    active.retain(|port, listener| {
        if ports.contains(port) {
            return true;
        }
        if let Some(task) = listener.task.take() {
            task.abort();
        }
        false
    });
    for port in ports {
        let entry = active.entry(port).or_insert(Listener {
            task: None,
            error: None,
        });
        if entry.task.as_ref().is_some_and(|task| !task.is_finished()) {
            continue;
        }
        match bind_dual_stack(port) {
            Ok(listener) => {
                entry.error = None;
                entry.task = Some(tokio::spawn(serve(listener, port)));
            }
            Err(error) => {
                let message = bind_error_message(port, &error);
                problems.push(message.clone());
                entry.error = Some(message);
                entry.task = None;
            }
        }
    }
    drop(active);
    problems
}

pub async fn status() -> Vec<ListenerStatus> {
    let hosts_by_port = table().read().map_or_else(
        |_| HashMap::new(),
        |table| {
            let mut hosts: HashMap<u16, Vec<String>> = HashMap::new();
            for (port, host) in table.routes.keys() {
                hosts.entry(*port).or_default().push(host.clone());
            }
            hosts
        },
    );
    let mut statuses: Vec<ListenerStatus> = listeners()
        .lock()
        .await
        .iter()
        .map(|(port, listener)| {
            let mut hosts = hosts_by_port.get(port).cloned().unwrap_or_default();
            hosts.sort();
            ListenerStatus {
                port: *port,
                running: listener
                    .task
                    .as_ref()
                    .is_some_and(|task| !task.is_finished()),
                hosts,
                error: listener.error.clone(),
            }
        })
        .collect();
    statuses.sort_by_key(|status| status.port);
    statuses
}

fn bind_error_message(port: u16, error: &std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        format!("没有权限监听端口 {port}：1024 以下端口需要管理员权限（Linux 可授予 CAP_NET_BIND_SERVICE），或改用其他端口")
    } else if error.kind() == std::io::ErrorKind::AddrInUse {
        format!("端口 {port} 已被其他程序占用")
    } else {
        format!("监听端口 {port} 失败：{error}")
    }
}

/// Listens on `[::]:port` accepting IPv4 too, falling back to `0.0.0.0:port`.
fn bind_dual_stack(port: u16) -> std::io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};

    let open = |domain: Domain, address: SocketAddr| -> std::io::Result<Socket> {
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        if domain == Domain::IPV6 {
            socket.set_only_v6(false)?;
        }
        #[cfg(unix)]
        socket.set_reuse_address(true)?;
        socket.bind(&address.into())?;
        socket.listen(1024)?;
        Ok(socket)
    };
    let socket = match open(
        Domain::IPV6,
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
    ) {
        Ok(socket) => socket,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => return Err(error),
        Err(_) => open(
            Domain::IPV4,
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        )?,
    };
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

fn tls_acceptor() -> Result<TlsAcceptor, String> {
    let mut config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(TableResolver));
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Accept loop for one port; `port` keys the routing table.
pub(crate) async fn serve(listener: TcpListener, port: u16) {
    let acceptor = match tls_acceptor() {
        Ok(acceptor) => acceptor,
        Err(error) => {
            tracing::error!(port, error, "failed to build TLS config for HTTPS proxy");
            return;
        }
    };
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(port, error = %error, "HTTPS proxy accept failed");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            let _ = stream.set_nodelay(true);
            let Ok(Ok(tls)) =
                tokio::time::timeout(Duration::from_secs(15), acceptor.accept(stream)).await
            else {
                return;
            };
            let sni = tls.get_ref().1.server_name().map(str::to_ascii_lowercase);
            let service =
                service_fn(move |request| proxy_request(request, port, sni.clone(), peer));
            let _ = auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(TokioIo::new(tls), service)
                .await;
        });
    }
}

fn text_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    let mut response = Response::new(
        Full::new(Bytes::from(message.to_string()))
            .map_err(|never| match never {})
            .boxed(),
    );
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn request_host<B>(request: &Request<B>) -> Option<String> {
    request
        .uri()
        .authority()
        .map(|authority| authority.host().to_string())
        .or_else(|| {
            request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .map(|host| {
                    // Strip the port, keeping bracketed IPv6 literals intact.
                    host.rsplit_once(':')
                        .filter(|(_, port)| port.chars().all(|ch| ch.is_ascii_digit()))
                        .map_or(host, |(name, _)| name)
                        .to_string()
                })
        })
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

fn client_ip_allowed(rules: &[IpRule], ip: IpAddr) -> bool {
    rules.is_empty() || rules.iter().any(|rule| rule.contains(ip))
}

const HOP_BY_HOP: [&str; 8] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Removes hop-by-hop headers, including any named by `Connection`.
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let named: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in named {
        headers.remove(name);
    }
    for name in HOP_BY_HOP {
        headers.remove(name);
    }
}

fn is_upgrade<B>(request: &Request<B>) -> bool {
    request.version() == Version::HTTP_11 && request.headers().contains_key(header::UPGRADE)
}

async fn proxy_request(
    mut request: Request<Incoming>,
    port: u16,
    sni: Option<String>,
    peer: SocketAddr,
) -> Result<Response<ProxyBody>, Infallible> {
    let host = request_host(&request);
    let Some(host) = host.or(sni) else {
        return Ok(text_response(StatusCode::BAD_REQUEST, "缺少 Host"));
    };
    let upstream = table()
        .read()
        .ok()
        .and_then(|table| table.routes.get(&(port, host.clone())).cloned());
    let Some(upstream) = upstream else {
        return Ok(text_response(
            StatusCode::MISDIRECTED_REQUEST,
            "该域名未配置到此端口的映射",
        ));
    };
    if !client_ip_allowed(&upstream.allowed_ips, peer.ip()) {
        tracing::info!(
            host,
            client_ip = %peer.ip(),
            "HTTPS domain mapping rejected client IP"
        );
        return Ok(text_response(
            StatusCode::FORBIDDEN,
            &format!(
                "当前访问 IP 为 {}，不在该域名的允许访问列表中",
                peer.ip().to_canonical()
            ),
        ));
    }

    let upgrade = is_upgrade(&request);
    let upgrade_protocol = request.headers().get(header::UPGRADE).cloned();
    let client_upgrade = upgrade.then(|| hyper::upgrade::on(&mut request));

    let (mut parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map_or("/", hyper::http::uri::PathAndQuery::as_str);
    let upstream_authority = if upstream.host.contains(':') {
        format!("[{}]:{}", upstream.host, upstream.port)
    } else {
        format!("{}:{}", upstream.host, upstream.port)
    };
    let Ok(uri) = format!("http://{upstream_authority}{path}").parse() else {
        return Ok(text_response(StatusCode::BAD_REQUEST, "无效的请求路径"));
    };
    let original_host = parts.headers.get(header::HOST).cloned().or_else(|| {
        parts
            .uri
            .authority()
            .and_then(|authority| HeaderValue::from_str(authority.as_str()).ok())
    });
    parts.uri = uri;
    parts.version = Version::HTTP_11;
    strip_hop_by_hop(&mut parts.headers);
    if let Some(host_value) = original_host {
        parts.headers.insert(header::HOST, host_value);
    }
    let client_ip = peer.ip().to_canonical().to_string();
    for (name, value) in [
        ("x-forwarded-for", client_ip.clone()),
        ("x-real-ip", client_ip),
        ("x-forwarded-proto", "https".to_string()),
        ("x-forwarded-host", host.clone()),
        ("x-forwarded-port", port.to_string()),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            parts.headers.insert(HeaderName::from_static(name), value);
        }
    }
    if let Some(protocol) = upgrade_protocol.filter(|_| upgrade) {
        parts
            .headers
            .insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
        parts.headers.insert(header::UPGRADE, protocol);
    }

    let mut response = match upstream_client()
        .request(Request::from_parts(parts, body))
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(host, error = %error, "HTTPS proxy upstream request failed");
            return Ok(text_response(
                StatusCode::BAD_GATEWAY,
                &format!("无法连接本地服务 {upstream_authority}"),
            ));
        }
    };

    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        if let Some(client_upgrade) = client_upgrade {
            let upstream_upgrade = hyper::upgrade::on(&mut response);
            tokio::spawn(async move {
                if let (Ok(client), Ok(server)) = (client_upgrade.await, upstream_upgrade.await) {
                    let mut client = TokioIo::new(client);
                    let mut server = TokioIo::new(server);
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                }
            });
        }
        let (parts, _) = response.into_parts();
        return Ok(Response::from_parts(
            parts,
            Empty::new().map_err(|never| match never {}).boxed(),
        ));
    }

    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop(&mut parts.headers);
    Ok(Response::from_parts(parts, body.boxed()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use uuid::Uuid;

    fn self_signed(name: &str) -> (ProxyCertificate, Vec<u8>) {
        let certified = rcgen::generate_simple_self_signed(vec![name.to_string()]).unwrap();
        let dir = std::env::temp_dir().join(format!("gpt-images-proxy-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert_path = dir.join("fullchain.pem");
        let key_path = dir.join("privkey.pem");
        std::fs::write(&cert_path, certified.cert.pem()).unwrap();
        std::fs::write(&key_path, certified.signing_key.serialize_pem()).unwrap();
        (
            ProxyCertificate {
                names: vec![name.to_string()],
                cert_path,
                key_path,
            },
            certified.cert.der().to_vec(),
        )
    }

    fn install(port: u16, host: &str, upstream_port: u16, certificate: &ProxyCertificate) {
        let key = load_certified_key(certificate).unwrap();
        let mut table = table().write().unwrap();
        table.certificates.insert(host.to_string(), key);
        table.routes.insert(
            (port, host.to_string()),
            Upstream {
                host: "127.0.0.1".to_string(),
                port: upstream_port,
                allowed_ips: Vec::new(),
            },
        );
    }

    #[test]
    fn wildcard_certificates_match_one_label() {
        let (certificate, _) = self_signed("*.wild.test");
        let key = load_certified_key(&certificate).unwrap();
        let certificates = HashMap::from([("*.wild.test".to_string(), key)]);
        assert!(certificate_for(&certificates, "app.wild.test").is_some());
        assert!(certificate_for(&certificates, "APP.Wild.Test.").is_some());
        assert!(certificate_for(&certificates, "wild.test").is_none());
        assert!(certificate_for(&certificates, "a.b.wild.test").is_none());
    }

    #[test]
    fn client_ip_rules_default_to_allow_all_and_support_cidr() {
        let ip = |value: &str| value.parse().unwrap();
        assert!(client_ip_allowed(&[], ip("203.0.113.9")));
        let rules = vec![IpRule::parse("192.168.1.0/24").unwrap()];
        assert!(client_ip_allowed(&rules, ip("192.168.1.42")));
        assert!(!client_ip_allowed(&rules, ip("192.168.2.42")));
    }

    #[test]
    fn hop_by_hop_headers_are_removed() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONNECTION,
            HeaderValue::from_static("keep-alive, x-secret"),
        );
        headers.insert("x-secret", HeaderValue::from_static("1"));
        headers.insert(
            header::TRANSFER_ENCODING,
            HeaderValue::from_static("chunked"),
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
        strip_hop_by_hop(&mut headers);
        assert_eq!(headers.len(), 1);
        assert!(headers.contains_key(header::ACCEPT));
    }

    /// Plain HTTP upstream: echoes request details, or switches protocols and echoes bytes.
    async fn spawn_echo_upstream() -> u16 {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = upstream.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buffer = vec![0_u8; 4096];
                    let read = stream.read(&mut buffer).await.unwrap();
                    let head = String::from_utf8_lossy(&buffer[..read]).to_lowercase();
                    if head.contains("upgrade: echo") {
                        stream
                            .write_all(b"HTTP/1.1 101 Switching Protocols\r\nconnection: upgrade\r\nupgrade: echo\r\n\r\n")
                            .await
                            .unwrap();
                        let read = stream.read(&mut buffer).await.unwrap();
                        stream.write_all(&buffer[..read]).await.unwrap();
                        return;
                    }
                    let body = head
                        .lines()
                        .filter(|line| {
                            line.starts_with("get ")
                                || line.starts_with("host:")
                                || line.starts_with("x-forwarded-proto:")
                                || line.starts_with("x-real-ip:")
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        upstream_port
    }

    #[tokio::test]
    async fn proxies_https_requests_and_websocket_upgrades() {
        let upstream_port = spawn_echo_upstream().await;

        let host = "app.proxy.test";
        let (certificate, cert_der) = self_signed(host);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        install(address.port(), host, upstream_port, &certificate);
        tokio::spawn(serve(listener, address.port()));

        let client = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_der(&cert_der).unwrap())
            .resolve(host, address)
            .build()
            .unwrap();
        let body = client
            .get(format!("https://{host}:{}/hello?x=1", address.port()))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(body.contains("get /hello?x=1 http/1.1"), "{body}");
        assert!(
            body.contains(&format!("host: {host}:{}", address.port())),
            "{body}"
        );
        assert!(body.contains("x-forwarded-proto: https"), "{body}");
        assert!(body.contains("x-real-ip: 127.0.0.1"), "{body}");

        // HTTP/2 takes the authority from the URL, so probe a foreign Host over HTTP/1.1.
        let http1 = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_der(&cert_der).unwrap())
            .resolve(host, address)
            .http1_only()
            .build()
            .unwrap();
        let unknown = http1
            .get(format!("https://{host}:{}/", address.port()))
            .header(header::HOST, "other.test")
            .send()
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::MISDIRECTED_REQUEST);

        // Raw upgrade through TLS: the proxy must splice both directions.
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(cert_der)).unwrap();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let tcp = tokio::net::TcpStream::connect(address).await.unwrap();
        let mut tls = connector
            .connect(
                rustls::pki_types::ServerName::try_from(host.to_string()).unwrap(),
                tcp,
            )
            .await
            .unwrap();
        tls.write_all(
            format!(
                "GET /ws HTTP/1.1\r\nhost: {host}\r\nconnection: upgrade\r\nupgrade: echo\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut buffer = vec![0_u8; 1024];
        let read = tls.read(&mut buffer).await.unwrap();
        assert!(String::from_utf8_lossy(&buffer[..read]).starts_with("HTTP/1.1 101"));
        tls.write_all(b"ping").await.unwrap();
        let read = tls.read(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..read], b"ping");
    }
}
