//! Who is calling: the client IP we can trust and the user agent.

use std::net::{IpAddr, SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header, request::Parts, HeaderMap},
};

use crate::middleware::access_log::forwarded_for;

/// Client details for handlers that record logins or sessions.
#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for ClientInfo {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| *address);
        Ok(Self {
            ip: trusted_client_ip(&parts.headers, peer).map(|ip| ip.to_string()),
            user_agent: user_agent(&parts.headers),
        })
    }
}

#[must_use]
pub fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.chars().take(512).collect())
}

/// The client address used for security decisions.
///
/// Forwarding headers are only believed when the TCP peer is this machine
/// (cloudflared, the built-in HTTPS proxy, a local nginx); otherwise anyone
/// could claim an allowed address. Without a peer (in-process test server)
/// the headers are all there is.
#[must_use]
pub fn trusted_client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<IpAddr> {
    let forwarded = || forwarded_for(headers).and_then(|value| parse_ip(&value));
    match peer {
        Some(address) if is_local(address.ip()) => forwarded().or_else(|| Some(address.ip())),
        Some(address) => Some(normalize(address.ip())),
        None => forwarded(),
    }
}

fn parse_ip(value: &str) -> Option<IpAddr> {
    let value = value.trim().trim_start_matches('[');
    value
        .parse::<IpAddr>()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|address| address.ip()))
        .or_else(|| {
            // "[::1]:443" style.
            value
                .split_once(']')
                .and_then(|(ip, _)| ip.parse::<IpAddr>().ok())
        })
        .map(normalize)
}

/// IPv4 clients on a dual-stack socket show up as `::ffff:a.b.c.d`.
#[must_use]
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

#[must_use]
pub fn is_local(ip: IpAddr) -> bool {
    normalize(ip).is_loopback()
}

#[cfg(test)]
mod tests {
    use super::{trusted_client_ip, HeaderMap, SocketAddr};

    fn headers(forwarded: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", forwarded.parse().unwrap());
        headers
    }

    #[test]
    fn forwarded_headers_are_trusted_only_from_a_local_proxy() {
        let proxy: SocketAddr = "127.0.0.1:50000".parse().unwrap();
        let remote: SocketAddr = "198.51.100.7:50000".parse().unwrap();
        let spoofed = headers("203.0.113.9, 10.0.0.1");

        assert_eq!(
            trusted_client_ip(&spoofed, Some(proxy))
                .unwrap()
                .to_string(),
            "203.0.113.9"
        );
        assert_eq!(
            trusted_client_ip(&spoofed, Some(remote))
                .unwrap()
                .to_string(),
            "198.51.100.7"
        );
        assert_eq!(
            trusted_client_ip(&HeaderMap::new(), Some(proxy))
                .unwrap()
                .to_string(),
            "127.0.0.1"
        );
        let mapped: SocketAddr = "[::ffff:198.51.100.7]:1".parse().unwrap();
        assert_eq!(
            trusted_client_ip(&HeaderMap::new(), Some(mapped))
                .unwrap()
                .to_string(),
            "198.51.100.7"
        );
    }
}
