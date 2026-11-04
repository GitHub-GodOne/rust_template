use std::sync::atomic::{AtomicUsize, Ordering};

use axum::http::{HeaderName, HeaderValue};
use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{testing::prelude::*, TestServer};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login_token(request: &TestServer, email: &str, password: &str) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let login = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.12.{ip_suffix}"))
        .json(&json!({ "email": email, "password": password }))
        .await;
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&login.text()).unwrap();
    login.data.unwrap().token
}

/// Creates a user (optionally with roles) and returns their id.
async fn create_user(
    request: &TestServer,
    admin: &(HeaderName, HeaderValue),
    email: &str,
    role_ids: &[i32],
) -> i64 {
    let created = request
        .post("/api/admin/users")
        .add_header(admin.0.clone(), admin.1.clone())
        .json(&json!({ "name": email, "email": email, "password": "Remote123pass" }))
        .await;
    assert_eq!(created.status_code(), 200, "{}", created.text());
    let created: Value = serde_json::from_str(&created.text()).unwrap();
    let id = created["data"]["id"].as_i64().unwrap();
    if !role_ids.is_empty() {
        let roles = request
            .put(&format!("/api/admin/users/{id}/roles"))
            .add_header(admin.0.clone(), admin.1.clone())
            .json(&json!({ "role_ids": role_ids }))
            .await;
        assert_eq!(roles.status_code(), 200, "{}", roles.text());
    }
    id
}

async fn get_json(request: &TestServer, path: &str, auth: &(HeaderName, HeaderValue)) -> Value {
    let response = request
        .get(path)
        .add_header(auth.0.clone(), auth.1.clone())
        .await;
    assert_eq!(response.status_code(), 200, "{path}: {}", response.text());
    serde_json::from_str(&response.text()).unwrap()
}

async fn open_session(
    request: &TestServer,
    kind: &str,
    target_key: &str,
    auth: &(HeaderName, HeaderValue),
) -> String {
    let response = request
        .post(&format!("/api/admin/{kind}/sessions"))
        .add_header(auth.0.clone(), auth.1.clone())
        .json(&json!({ "target_key": target_key }))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    let body: Value = serde_json::from_str(&response.text()).unwrap();
    body["data"]["id"].as_str().unwrap().to_string()
}

fn ids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["id"].as_str().map(ToString::to_string))
        .collect()
}

#[tokio::test]
#[serial]
async fn vnc_sessions_are_private_and_admins_manage_every_remote_session() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin_token = login_token(&request, "admin@example.com", "1234").await;
        let admin = prepare_data::auth_header(&admin_token);
        prepare_data::confirm_password(&request, &admin_token, "1234").await;

        let ops_id = create_user(&request, &admin, "ops3@example.com", &[1]).await;
        let ops_token = login_token(&request, "ops3@example.com", "Remote123pass").await;
        let ops = prepare_data::auth_header(&ops_token);
        prepare_data::confirm_password(&request, &ops_token, "Remote123pass").await;

        let admin_vnc = open_session(&request, "vnc", "local-vnc", &admin).await;
        let ops_vnc = open_session(&request, "vnc", "local-vnc", &ops).await;
        let ops_ssh = open_session(&request, "ssh", "local-shell", &ops).await;

        // Each user only lists, attaches to and closes their own VNC sessions.
        let listed = ids(&get_json(&request, "/api/admin/vnc/sessions", &ops).await);
        assert!(listed.contains(&ops_vnc));
        assert!(!listed.contains(&admin_vnc));
        let attach = request
            .post(&format!("/api/admin/vnc/sessions/{admin_vnc}/tickets"))
            .add_header(ops.0.clone(), ops.1.clone())
            .await;
        assert_eq!(attach.status_code(), 404);
        let body: Value = serde_json::from_str(&attach.text()).unwrap();
        assert_eq!(body["code"], "VNC_SESSION_NOT_FOUND");
        let close = request
            .delete(&format!("/api/admin/vnc/sessions/{admin_vnc}"))
            .add_header(ops.0.clone(), ops.1.clone())
            .await;
        assert_eq!(close.status_code(), 404);

        // The admin view lists everyone's SSH and VNC sessions with their owner.
        let all = get_json(&request, "/api/admin/sessions/remote", &admin).await;
        let all_ids = ids(&all);
        for id in [&admin_vnc, &ops_vnc, &ops_ssh] {
            assert!(all_ids.contains(id), "{id} missing from {all}");
        }
        let ops_row = all["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == ops_vnc.as_str())
            .unwrap();
        assert_eq!(ops_row["kind"], "vnc");
        assert_eq!(ops_row["user_email"], "ops3@example.com");
        let vnc_only =
            ids(&get_json(&request, "/api/admin/sessions/remote?kind=vnc", &admin).await);
        assert!(vnc_only.contains(&ops_vnc));
        assert!(!vnc_only.contains(&ops_ssh));
        let by_keyword =
            ids(&get_json(&request, "/api/admin/sessions/remote?keyword=OPS3", &admin).await);
        assert!(by_keyword.contains(&ops_vnc));
        assert!(!by_keyword.contains(&admin_vnc));

        // Without the permission the admin view is closed.
        create_user(&request, &admin, "viewer3@example.com", &[]).await;
        let viewer_token = login_token(&request, "viewer3@example.com", "Remote123pass").await;
        let viewer = prepare_data::auth_header(&viewer_token);
        let denied = request
            .get("/api/admin/sessions/remote")
            .add_header(viewer.0.clone(), viewer.1.clone())
            .await;
        assert_eq!(denied.status_code(), 403);
        let denied = request
            .delete(&format!("/api/admin/sessions/remote/vnc/{ops_vnc}"))
            .add_header(viewer.0, viewer.1)
            .await;
        assert_eq!(denied.status_code(), 403);

        // An admin can close someone else's session.
        let forced = request
            .delete(&format!("/api/admin/sessions/remote/vnc/{ops_vnc}"))
            .add_header(admin.0.clone(), admin.1.clone())
            .await;
        assert_eq!(forced.status_code(), 200, "{}", forced.text());
        let forced: Value = serde_json::from_str(&forced.text()).unwrap();
        assert_eq!(forced["data"]["user_email"], "ops3@example.com");
        assert!(
            !ids(&get_json(&request, "/api/admin/vnc/sessions", &ops).await).contains(&ops_vnc)
        );
        let again = request
            .delete(&format!("/api/admin/sessions/remote/vnc/{ops_vnc}"))
            .add_header(admin.0.clone(), admin.1.clone())
            .await;
        assert_eq!(again.status_code(), 404);
        let wrong_kind = request
            .delete(&format!("/api/admin/sessions/remote/rdp/{ops_ssh}"))
            .add_header(admin.0.clone(), admin.1.clone())
            .await;
        assert_eq!(wrong_kind.status_code(), 404);

        // Disabling the account ends the rest of their remote sessions.
        let disabled = request
            .put(&format!("/api/admin/users/{ops_id}/status"))
            .add_header(admin.0.clone(), admin.1.clone())
            .json(&json!({ "status": "disabled" }))
            .await;
        assert_eq!(disabled.status_code(), 200, "{}", disabled.text());
        let remaining = ids(&get_json(&request, "/api/admin/sessions/remote", &admin).await);
        assert!(!remaining.contains(&ops_ssh));
        assert!(remaining.contains(&admin_vnc));

        let own_close = request
            .delete(&format!("/api/admin/vnc/sessions/{admin_vnc}"))
            .add_header(admin.0, admin.1)
            .await;
        assert_eq!(own_close.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn rdp_connections_are_listed_per_user() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_token(&request, "admin@example.com", "1234").await;
        let auth = prepare_data::auth_header(&token);

        let listed = get_json(&request, "/api/admin/rdp/sessions", &auth).await;
        assert!(listed["data"].is_array());
        let close = request
            .delete("/api/admin/rdp/sessions/does-not-exist")
            .add_header(auth.0.clone(), auth.1.clone())
            .await;
        assert_eq!(close.status_code(), 404);
        let body: Value = serde_json::from_str(&close.text()).unwrap();
        assert_eq!(body["code"], "RDP_SESSION_NOT_FOUND");
        let forced = request
            .delete("/api/admin/sessions/remote/rdp/does-not-exist")
            .add_header(auth.0, auth.1)
            .await;
        assert_eq!(forced.status_code(), 404);
    })
    .await;
}
