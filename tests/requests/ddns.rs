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
use gpt_images::{
    app::App, models::_entities::system_settings, responses::ApiResponse,
    views::auth::LoginResponse,
};
use loco_rs::{app::AppContext, testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serial_test::serial;
use tokio::net::TcpListener;

use super::prepare_data;

const TOKEN: &str = "mock-cloudflare-token";
const ZONE_ID: &str = "zone-123";
const RECORD_NAME: &str = "home.example.com";

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

#[derive(Default)]
struct MockCloudflare {
    ipv6: String,
    records: Vec<serde_json::Value>,
    writes: Vec<String>,
}

type Shared = Arc<Mutex<MockCloudflare>>;

fn authorized(headers: &HeaderMap) -> bool {
    if headers
        .get("x-auth-key")
        .and_then(|value| value.to_str().ok())
        == Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    {
        return !headers.contains_key(header::AUTHORIZATION)
            && headers
                .get("x-auth-email")
                .and_then(|value| value.to_str().ok())
                == Some("owner@example.com");
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(&format!("Bearer {TOKEN}"))
}

fn envelope(result: serde_json::Value) -> axum::response::Response {
    Json(serde_json::json!({ "success": true, "errors": [], "result": result })).into_response()
}

async fn spawn_mock(state: Shared) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route(
            "/ip",
            get(|State(state): State<Shared>| async move { state.lock().unwrap().ipv6.clone() }),
        )
        .route(
            "/client/v4/zones",
            get(
                |headers: HeaderMap,
                 Query(query): Query<std::collections::HashMap<String, String>>| async move {
                    if !authorized(&headers) {
                        return (
                            StatusCode::FORBIDDEN,
                            Json(serde_json::json!({
                                "success": false,
                                "errors": [{ "code": 9109, "message": "Invalid access token" }],
                                "result": null
                            })),
                        )
                            .into_response();
                    }
                    let zones = if query.get("name").map(String::as_str) == Some("example.com") {
                        serde_json::json!([{ "id": ZONE_ID, "name": "example.com" }])
                    } else {
                        serde_json::json!([])
                    };
                    envelope(zones)
                },
            ),
        )
        .route(
            "/client/v4/zones/{zone}/dns_records",
            get(
                |State(state): State<Shared>,
                 Path(zone): Path<String>,
                 Query(query): Query<std::collections::HashMap<String, String>>| async move {
                    assert_eq!(zone, ZONE_ID);
                    assert_eq!(query.get("type").map(String::as_str), Some("AAAA"));
                    let records: Vec<serde_json::Value> = state
                        .lock()
                        .unwrap()
                        .records
                        .iter()
                        .filter(|record| {
                            Some(&record["name"].as_str().unwrap_or_default().to_string())
                                == query.get("name")
                        })
                        .cloned()
                        .collect();
                    envelope(serde_json::Value::Array(records))
                },
            )
            .post(
                |State(state): State<Shared>,
                 headers: HeaderMap,
                 Json(body): Json<serde_json::Value>| async move {
                    assert!(authorized(&headers));
                    let mut state = state.lock().unwrap();
                    let mut record = body.clone();
                    record["id"] = serde_json::json!(format!("record-{}", state.records.len() + 1));
                    state
                        .writes
                        .push(format!("POST {}", body["content"].as_str().unwrap()));
                    state.records.push(record.clone());
                    envelope(record)
                },
            ),
        )
        .route(
            "/client/v4/zones/{zone}/dns_records/{id}",
            axum::routing::patch(
                |State(state): State<Shared>,
                 Path((_zone, id)): Path<(String, String)>,
                 Json(body): Json<serde_json::Value>| async move {
                    let mut state = state.lock().unwrap();
                    state
                        .writes
                        .push(format!("PATCH {id} {}", body["content"].as_str().unwrap()));
                    let record = state
                        .records
                        .iter_mut()
                        .find(|record| record["id"] == id.as_str())
                        .unwrap();
                    record["content"] = body["content"].clone();
                    record["ttl"] = body["ttl"].clone();
                    record["proxied"] = body["proxied"].clone();
                    envelope(record.clone())
                },
            ),
        )
        .with_state(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

async fn set_setting(ctx: &AppContext, key: &str, value: &str) {
    let setting = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(key))
        .one(&ctx.db)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("setting {key} missing from fixtures"));
    let mut active = setting.into_active_model();
    active.value = Set(value.to_string());
    active.update(&ctx.db).await.unwrap();
}

async fn admin_token(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.3.{ip_suffix}"))
        .json(&serde_json::json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    assert_eq!(response.status_code(), 200);
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

async fn sync(request: &TestServer, token: &str) -> (u16, serde_json::Value) {
    let (key, value) = prepare_data::auth_header(token);
    let response = request
        .post("/api/admin/ddns/sync")
        .add_header(key, value)
        .await;
    let status = response.status_code().as_u16();
    (status, serde_json::from_str(&response.text()).unwrap())
}

#[tokio::test]
#[serial]
async fn ddns_endpoints_require_auth() {
    request::<App, _, _>(|request, _ctx| async move {
        assert_eq!(
            request.get("/api/admin/ddns/status").await.status_code(),
            401
        );
        assert_eq!(
            request.post("/api/admin/ddns/sync").await.status_code(),
            401
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn ddns_sync_creates_then_updates_cloudflare_aaaa_record() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let mock = Arc::new(Mutex::new(MockCloudflare {
            ipv6: "2409:8a1e:1234:5678::a1\n".to_string(),
            ..Default::default()
        }));
        let base = spawn_mock(mock.clone()).await;
        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let status = request
            .get("/api/admin/ddns/status")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(status.status_code(), 200);
        let status: serde_json::Value = serde_json::from_str(&status.text()).unwrap();
        assert_eq!(status["data"]["enabled"], false);
        assert_eq!(status["data"]["configured"], false);

        let (code, body) = sync(&request, &token).await;
        assert_eq!(code, 400, "disabled ddns cannot be synced: {body}");

        set_setting(&ctx, "ddns.enabled", "true").await;
        set_setting(&ctx, "ddns.cloudflare_api_token", TOKEN).await;
        set_setting(&ctx, "ddns.record_names", RECORD_NAME).await;
        set_setting(&ctx, "ddns.ipv6_source", "url").await;
        set_setting(&ctx, "ddns.ipv6_lookup_url", &format!("{base}/ip")).await;
        set_setting(
            &ctx,
            "ddns.cloudflare_api_base",
            &format!("{base}/client/v4"),
        )
        .await;

        let (code, body) = sync(&request, &token).await;
        assert_eq!(code, 200, "{body}");
        let data = &body["data"];
        assert_eq!(data["current_ipv6"], "2409:8a1e:1234:5678::a1");
        assert_eq!(data["last_synced_ipv6"], "2409:8a1e:1234:5678::a1");
        assert!(data["last_error"].is_null());
        assert_eq!(data["records"][0]["name"], RECORD_NAME);
        assert_eq!(data["records"][0]["action"], "created");

        let (_, body) = sync(&request, &token).await;
        assert_eq!(body["data"]["records"][0]["action"], "unchanged");

        mock.lock().unwrap().ipv6 = "2409:8a1e:1234:9999::b2".to_string();
        let (_, body) = sync(&request, &token).await;
        assert_eq!(body["data"]["records"][0]["action"], "updated");
        assert_eq!(body["data"]["current_ipv6"], "2409:8a1e:1234:9999::b2");
        assert_eq!(
            mock.lock().unwrap().writes,
            vec![
                "POST 2409:8a1e:1234:5678::a1".to_string(),
                "PATCH record-1 2409:8a1e:1234:9999::b2".to_string(),
            ]
        );

        set_setting(&ctx, "ddns.cloudflare_api_token", "wrong-token").await;
        set_setting(&ctx, "ddns.record_names", "nas.example.com").await;
        let (code, body) = sync(&request, &token).await;
        assert_eq!(code, 200);
        assert_eq!(body["data"]["records"][0]["action"], "failed");
        assert!(body["data"]["last_error"]
            .as_str()
            .unwrap()
            .contains("Invalid access token"));

        // The reported production failure: a Global API Key cannot be used as Bearer.
        set_setting(
            &ctx,
            "ddns.cloudflare_api_token",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await;
        let (code, body) = sync(&request, &token).await;
        assert_eq!(
            code, 400,
            "invalid credentials are rejected before calling Cloudflare: {body}"
        );
        assert!(body["message"].as_str().unwrap().contains("Global API Key"));
        set_setting(&ctx, "ddns.cloudflare_auth_mode", "global_api_key").await;
        set_setting(&ctx, "ddns.cloudflare_api_email", "owner@example.com").await;
        let (code, body) = sync(&request, &token).await;
        assert_eq!(code, 200, "{body}");
        assert!(body["data"]["last_error"].is_null(), "{body}");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn ddns_settings_are_validated_on_save() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);
        let setting = system_settings::Entity::find()
            .filter(system_settings::Column::Key.eq("ddns.ipv6_source"))
            .one(&ctx.db)
            .await
            .unwrap()
            .unwrap();
        let payload = |value: &str| {
            serde_json::json!({
                "key": setting.key,
                "name": setting.name,
                "group_key": setting.group_key,
                "value": value,
                "value_type": setting.value_type,
                "default_value": setting.default_value,
                "description": setting.description,
                "is_builtin": true,
                "sort_order": setting.sort_order,
            })
        };

        let rejected = request
            .put(&format!("/api/admin/settings/{}", setting.id))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&payload("dhcp"))
            .await;
        assert_eq!(rejected.status_code(), 400);

        let accepted = request
            .put(&format!("/api/admin/settings/{}", setting.id))
            .add_header(auth_key, auth_value)
            .json(&payload("interface"))
            .await;
        assert_eq!(accepted.status_code(), 200);
    })
    .await;
}

async fn put_config(
    request: &TestServer,
    token: &str,
    body: &serde_json::Value,
) -> (u16, serde_json::Value) {
    let (key, value) = prepare_data::auth_header(token);
    let response = request
        .put("/api/admin/ddns/config")
        .add_header(key, value)
        .json(body)
        .await;
    let status = response.status_code().as_u16();
    (status, serde_json::from_str(&response.text()).unwrap())
}

#[tokio::test]
#[serial]
async fn ddns_config_api_masks_token_and_syncs_manual_addresses() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let mock = Arc::new(Mutex::new(MockCloudflare {
            ipv6: "2409:8a1e:1234:5678::a1".to_string(),
            ..Default::default()
        }));
        let base = spawn_mock(mock.clone()).await;
        let token = admin_token(&request).await;
        let config = |enabled: bool, api_token: Option<&str>, source: &str, manual: &str| {
            serde_json::json!({
                "enabled": enabled,
                "api_token": api_token,
                "zone_id": "",
                "record_names": " Home.Example.com ",
                "proxied": false,
                "ttl": 1,
                "check_interval_seconds": 300,
                "ipv6_source": source,
                "interface_name": "",
                "manual_ipv6": manual,
                "ipv6_lookup_url": format!("{base}/ip"),
                "api_base": format!("{base}/client/v4/"),
            })
        };

        let (code, _) = put_config(&request, &token, &config(true, None, "url", "")).await;
        assert_eq!(code, 400, "enabling without a token is rejected");
        let (code, _) = put_config(&request, &token, &config(false, None, "manual", "")).await;
        assert_eq!(code, 400, "manual source needs an address");
        let (code, _) = put_config(&request, &token, &config(false, None, "url", "fe80::1")).await;
        assert_eq!(code, 400, "link-local manual address is rejected");

        let (code, body) =
            put_config(&request, &token, &config(true, Some(TOKEN), "url", "")).await;
        assert_eq!(code, 200, "{body}");
        assert_eq!(body["data"]["token_configured"], true);
        assert_eq!(body["data"]["record_names"], "home.example.com");
        assert_eq!(body["data"]["auth_mode"], "api_token");
        assert!(body["data"].get("api_token").is_none());

        // Saving again without a token keeps the stored one.
        let (code, body) = put_config(&request, &token, &config(true, None, "url", "")).await;
        assert_eq!(code, 200, "{body}");
        assert_eq!(body["data"]["token_configured"], true);

        let (auth_key, auth_value) = prepare_data::auth_header(&token);
        let fetched = request
            .get("/api/admin/ddns/config")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        let fetched: serde_json::Value = serde_json::from_str(&fetched.text()).unwrap();
        assert_eq!(fetched["data"]["api_base"], format!("{base}/client/v4"));

        let mut global = config(
            true,
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            "url",
            "",
        );
        global["auth_mode"] = serde_json::json!("global_api_key");
        let (code, _) = put_config(&request, &token, &global).await;
        assert_eq!(code, 400, "Global API Key requires an email");
        global["api_email"] = serde_json::json!("owner@example.com");
        let (code, body) = put_config(&request, &token, &global).await;
        assert_eq!(code, 200, "{body}");
        assert!(body["data"].get("api_token").is_none());
        let (code, body) = put_config(&request, &token, &config(true, None, "url", "")).await;
        assert_eq!(code, 200, "{body}");
        assert_eq!(
            body["data"]["auth_mode"], "global_api_key",
            "older clients preserve auth mode"
        );

        let invalid = request
            .post("/api/admin/ddns/sync")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "ipv6": "not-an-address" }))
            .await;
        assert_eq!(invalid.status_code(), 400);

        let manual = request
            .post("/api/admin/ddns/sync")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "ipv6": "2409:8a1e:abcd::1" }))
            .await;
        assert_eq!(manual.status_code(), 200);
        let manual: serde_json::Value = serde_json::from_str(&manual.text()).unwrap();
        assert_eq!(manual["data"]["records"][0]["action"], "created");
        assert_eq!(manual["data"]["last_synced_ipv6"], "2409:8a1e:abcd::1");
        assert_eq!(manual["data"]["last_sync_trigger"], "manual_address");

        // A pinned manual source is used by the regular sync as well.
        let (code, _) = put_config(
            &request,
            &token,
            &config(true, None, "manual", "2409:8a1e:abcd::2"),
        )
        .await;
        assert_eq!(code, 200);
        let (code, body) = sync(&request, &token).await;
        assert_eq!(code, 200, "{body}");
        assert_eq!(body["data"]["records"][0]["action"], "updated");
        assert_eq!(body["data"]["current_ipv6"], "2409:8a1e:abcd::2");
        assert_eq!(
            mock.lock().unwrap().writes,
            vec![
                "POST 2409:8a1e:abcd::1".to_string(),
                "PATCH record-1 2409:8a1e:abcd::2".to_string(),
            ]
        );
    })
    .await;
}
