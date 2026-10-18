use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{testing::prelude::*, TestServer};
use serde_json::{json, Value};
use serial_test::serial;
use tokio::net::TcpListener;

use super::prepare_data;

const TOKEN: &str = "mock-certificate-token";
const ACCOUNT: &str = "0123456789abcdef";
const ZONE: &str = "zone-cert";

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

/// In-memory stand-in for the Cloudflare DNS and Tunnel APIs.
#[derive(Default)]
struct MockCloudflare {
    records: Vec<Value>,
    tunnels: Vec<Value>,
    tunnel_config: Option<Value>,
}

type Shared = Arc<Mutex<MockCloudflare>>;

fn envelope(result: Value) -> axum::response::Response {
    Json(json!({ "success": true, "errors": [], "result": result })).into_response()
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(&format!("Bearer {TOKEN}"))
}

type Params = Query<std::collections::HashMap<String, String>>;

async fn spawn_mock(state: Shared) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route(
            "/zones",
            get(|headers: HeaderMap, Query(query): Params| async move {
                if !authorized(&headers) {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(json!({ "success": false, "errors": [{ "code": 9109, "message": "Invalid access token" }], "result": null })),
                    )
                        .into_response();
                }
                let zones = if query.get("name").map(String::as_str) == Some("example.test") {
                    json!([{ "id": ZONE }])
                } else {
                    json!([])
                };
                envelope(zones)
            }),
        )
        .route(
            "/zones/{zone}/dns_records",
            get(
                |State(state): State<Shared>, Path(zone): Path<String>, Query(query): Params| async move {
                    assert_eq!(zone, ZONE);
                    let records: Vec<Value> = state
                        .lock()
                        .unwrap()
                        .records
                        .iter()
                        .filter(|record| {
                            Some(&record["type"].as_str().unwrap().to_string()) == query.get("type")
                                && Some(&record["name"].as_str().unwrap().to_string())
                                    == query.get("name")
                        })
                        .cloned()
                        .collect();
                    envelope(Value::Array(records))
                },
            )
            .post(
                |State(state): State<Shared>, headers: HeaderMap, Json(body): Json<Value>| async move {
                    assert!(authorized(&headers));
                    let mut state = state.lock().unwrap();
                    let mut record = body;
                    record["id"] = json!(format!("record-{}", state.records.len() + 1));
                    state.records.push(record.clone());
                    envelope(record)
                },
            ),
        )
        .route(
            "/zones/{zone}/dns_records/{id}",
            axum::routing::patch(
                |State(state): State<Shared>,
                 Path((_zone, id)): Path<(String, String)>,
                 Json(body): Json<Value>| async move {
                    let mut state = state.lock().unwrap();
                    let record = state
                        .records
                        .iter_mut()
                        .find(|record| record["id"] == id.as_str())
                        .unwrap();
                    record["content"] = body["content"].clone();
                    record["proxied"] = body["proxied"].clone();
                    envelope(record.clone())
                },
            )
            .delete(
                |State(state): State<Shared>, Path((_zone, id)): Path<(String, String)>| async move {
                    state
                        .lock()
                        .unwrap()
                        .records
                        .retain(|record| record["id"] != id.as_str());
                    envelope(json!({ "id": id }))
                },
            ),
        )
        .route(
            "/accounts/{account}/cfd_tunnel",
            get(
                |State(state): State<Shared>, Path(account): Path<String>, Query(query): Params| async move {
                    assert_eq!(account, ACCOUNT);
                    let tunnels: Vec<Value> = state
                        .lock()
                        .unwrap()
                        .tunnels
                        .iter()
                        .filter(|tunnel| Some(&tunnel["name"].as_str().unwrap().to_string()) == query.get("name"))
                        .cloned()
                        .collect();
                    envelope(Value::Array(tunnels))
                },
            )
            .post(
                |State(state): State<Shared>, Path(account): Path<String>, Json(body): Json<Value>| async move {
                    assert_eq!(account, ACCOUNT);
                    assert_eq!(body["config_src"], "cloudflare");
                    let tunnel = json!({ "id": "tunnel-1", "name": body["name"] });
                    state.lock().unwrap().tunnels.push(tunnel.clone());
                    envelope(tunnel)
                },
            ),
        )
        .route(
            "/accounts/{account}/cfd_tunnel/{id}",
            get(|State(state): State<Shared>, Path((_account, id)): Path<(String, String)>| async move {
                let found = state
                    .lock()
                    .unwrap()
                    .tunnels
                    .iter()
                    .find(|tunnel| tunnel["id"] == id.as_str())
                    .cloned();
                found.map_or_else(
                    || {
                        (
                            StatusCode::NOT_FOUND,
                            Json(json!({ "success": false, "errors": [{ "code": 1003, "message": "not found" }], "result": null })),
                        )
                            .into_response()
                    },
                    |tunnel| envelope(json!({ "id": tunnel["id"], "deleted_at": null })),
                )
            }),
        )
        .route(
            "/accounts/{account}/cfd_tunnel/{id}/token",
            get(|| async { envelope(json!("tunnel-secret-token")) }),
        )
        .route(
            "/accounts/{account}/cfd_tunnel/{id}/configurations",
            axum::routing::put(
                |State(state): State<Shared>, Json(body): Json<Value>| async move {
                    state.lock().unwrap().tunnel_config = Some(body["config"].clone());
                    envelope(json!({}))
                },
            ),
        )
        .with_state(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

async fn admin_token(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.5.{ip_suffix}"))
        .json(&json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    assert_eq!(response.status_code(), 200);
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

fn record<'a>(state: &'a MockCloudflare, record_type: &str, name: &str) -> Option<&'a Value> {
    state
        .records
        .iter()
        .find(|record| record["type"] == record_type && record["name"] == name)
}

#[tokio::test]
#[serial]
async fn certificates_and_domain_mappings_drive_dns_and_tunnel() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let mock = Arc::new(Mutex::new(MockCloudflare::default()));
        let api_base = spawn_mock(mock.clone()).await;
        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let anonymous = request.get("/api/admin/domain-mappings").await;
        assert_eq!(anonymous.status_code(), 401);

        let settings = request
            .put("/api/admin/certificates/settings")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&json!({
                "acme_email": "ops@example.test",
                "acme_directory": "staging",
                "cloudflare_api_token": TOKEN,
                "cloudflare_account_id": ACCOUNT,
                "cloudflare_api_base": api_base,
                "cloudflared_path": "/definitely/not/installed/cloudflared",
                "tunnel_name": "gpt-images-test"
            }))
            .await;
        assert_eq!(settings.status_code(), 200, "{}", settings.text());
        assert!(!settings.text().contains(TOKEN));
        assert!(settings.text().contains(r#""cloudflare_api_token_set":true"#));

        for (payload, message) in [
            (json!({ "domain": "not a domain", "mode": "direct", "upstream_port": 3000 }), "不是有效的域名"),
            (json!({ "domain": "a.example.test", "mode": "magic", "upstream_port": 3000 }), "映射方式"),
            (json!({ "domain": "a.example.test", "mode": "direct", "upstream_port": 0 }), "本地服务端口"),
            (json!({ "domain": "a.example.test", "mode": "direct", "upstream_port": 3000, "public_port": ctx.config.server.port }), "后台服务端口冲突"),
            (json!({ "domain": "a.example.test", "mode": "direct", "upstream_port": 3000, "ipv4_mode": "manual", "ipv4_address": "nope" }), "IPv4"),
        ] {
            let response = request
                .post("/api/admin/domain-mappings")
                .add_header(auth_key.clone(), auth_value.clone())
                .json(&payload)
                .await;
            assert_eq!(response.status_code(), 400, "{payload}");
            assert!(response.text().contains(message), "{}", response.text());
        }

        // Direct mode: HTTPS on a custom port, A + AAAA records pointing here.
        let direct = request
            .post("/api/admin/domain-mappings")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&json!({
                "name": "本地应用",
                "domain": "App.Example.Test",
                "mode": "direct",
                "upstream_port": 3000,
                "public_port": 8443,
                "ipv4_mode": "manual",
                "ipv4_address": "203.0.113.10",
                "ipv6_mode": "manual",
                "ipv6_address": "2001:db8::10",
                "issue_certificate": false
            }))
            .await;
        assert_eq!(direct.status_code(), 200, "{}", direct.text());
        let direct: Value = serde_json::from_str(&direct.text()).unwrap();
        let direct_id = direct["data"]["mapping"]["id"].as_i64().unwrap();
        assert_eq!(direct["data"]["mapping"]["domain"], "app.example.test");
        assert_eq!(direct["data"]["mapping"]["public_url"], "https://app.example.test:8443");
        assert!(direct["data"]["notice"].as_str().unwrap().contains("手动申请"));

        let synced = request
            .post(&format!("/api/admin/domain-mappings/{direct_id}/sync"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(synced.status_code(), 200, "{}", synced.text());
        let synced: Value = serde_json::from_str(&synced.text()).unwrap();
        assert_eq!(synced["data"]["mapping"]["status"], "pending");
        assert!(synced["data"]["mapping"]["last_error"]
            .as_str()
            .unwrap()
            .contains("证书尚未签发"));
        {
            let state = mock.lock().unwrap();
            let a = record(&state, "A", "app.example.test").expect("A record");
            assert_eq!(a["content"], "203.0.113.10");
            assert_eq!(a["proxied"], false);
            let aaaa = record(&state, "AAAA", "app.example.test").expect("AAAA record");
            assert_eq!(aaaa["content"], "2001:db8::10");
        }

        // The mapping created a certificate record that is still in use.
        let certificates = request
            .get("/api/admin/certificates")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        let certificates: Value = serde_json::from_str(&certificates.text()).unwrap();
        let app_certificate = certificates["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|certificate| certificate["domain"] == "app.example.test")
            .expect("certificate created for the direct mapping");
        assert_eq!(app_certificate["status"], "pending");
        assert_eq!(app_certificate["mapping_count"], 1);
        let in_use = request
            .delete(&format!("/api/admin/certificates/{}", app_certificate["id"]))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(in_use.status_code(), 400);

        let wildcard = request
            .post("/api/admin/certificates")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&json!({ "domain": "example.test", "alt_names": ["*.example.test", "example.test"], "issue_now": false }))
            .await;
        assert_eq!(wildcard.status_code(), 200, "{}", wildcard.text());
        let wildcard: Value = serde_json::from_str(&wildcard.text()).unwrap();
        assert_eq!(wildcard["data"]["alt_names"], json!(["*.example.test"]));
        let duplicate = request
            .post("/api/admin/certificates")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&json!({ "domain": "example.test", "issue_now": false }))
            .await;
        assert_eq!(duplicate.status_code(), 400);

        // Tunnel mode: remotely-managed tunnel, ingress rules and a proxied CNAME.
        let tunnel = request
            .post("/api/admin/domain-mappings")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&json!({ "domain": "tun.example.test", "mode": "tunnel", "upstream_port": 8080, "public_port": 9999 }))
            .await;
        assert_eq!(tunnel.status_code(), 200, "{}", tunnel.text());
        let tunnel: Value = serde_json::from_str(&tunnel.text()).unwrap();
        let tunnel_id = tunnel["data"]["mapping"]["id"].as_i64().unwrap();
        assert_eq!(tunnel["data"]["mapping"]["public_port"], 443);
        assert_eq!(tunnel["data"]["mapping"]["public_url"], "https://tun.example.test");

        let synced = request
            .post(&format!("/api/admin/domain-mappings/{tunnel_id}/sync"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        let synced: Value = serde_json::from_str(&synced.text()).unwrap();
        assert_eq!(synced["data"]["mapping"]["status"], "error");
        assert!(synced["data"]["mapping"]["last_error"]
            .as_str()
            .unwrap()
            .contains("cloudflared"));
        {
            let state = mock.lock().unwrap();
            assert_eq!(state.tunnels.len(), 1);
            assert_eq!(state.tunnels[0]["name"], "gpt-images-test");
            assert_eq!(
                state.tunnel_config.clone().unwrap(),
                json!({ "ingress": [
                    { "hostname": "tun.example.test", "service": "http://127.0.0.1:8080" },
                    { "service": "http_status:404" }
                ]})
            );
            let cname = record(&state, "CNAME", "tun.example.test").expect("CNAME record");
            assert_eq!(cname["content"], "tunnel-1.cfargotunnel.com");
            assert_eq!(cname["proxied"], true);
        }

        let runtime = request
            .get("/api/admin/domain-mappings/runtime")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(runtime.status_code(), 200);
        let runtime: Value = serde_json::from_str(&runtime.text()).unwrap();
        assert_eq!(runtime["data"]["tunnel_id"], "tunnel-1");
        assert_eq!(runtime["data"]["tunnel"]["running"], false);
        assert!(!runtime.to_string().contains("tunnel-secret-token"));

        // Deleting mappings also removes the DNS records they managed.
        for id in [tunnel_id, direct_id] {
            let deleted = request
                .delete(&format!("/api/admin/domain-mappings/{id}"))
                .add_header(auth_key.clone(), auth_value.clone())
                .await;
            assert_eq!(deleted.status_code(), 200);
        }
        assert!(mock.lock().unwrap().records.is_empty());

        let released = request
            .delete(&format!("/api/admin/certificates/{}", app_certificate["id"]))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(released.status_code(), 200);
    })
    .await;
}
