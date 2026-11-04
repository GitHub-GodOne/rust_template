use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{
    app::App, models::_entities::system_settings, responses::ApiResponse,
    services::security_settings, views::auth::LoginResponse,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn set_setting(ctx: &loco_rs::app::AppContext, key: &str, value: &str) {
    let setting = system_settings::Entity::find()
        .filter(system_settings::Column::Key.eq(key))
        .one(&ctx.db)
        .await
        .unwrap()
        .expect("setting is seeded");
    let mut active = setting.into_active_model();
    active.value = Set(value.to_string());
    active.update(&ctx.db).await.unwrap();
    security_settings::invalidate();
}

#[tokio::test]
#[serial]
async fn missing_ssh_session_returns_a_recoverable_error_code() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let login = request
            .post("/api/auth/login")
            .add_header("x-forwarded-for", format!("127.0.4.{ip_suffix}"))
            .json(&serde_json::json!({ "email": "admin@example.com", "password": "1234" }))
            .await;
        let login: ApiResponse<LoginResponse> = serde_json::from_str(&login.text()).unwrap();
        let (auth_key, auth_value) = prepare_data::auth_header(&login.data.unwrap().token);

        let ticket = request
            .post("/api/admin/ssh/sessions/does-not-exist/tickets")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(ticket.status_code(), 404);
        let body: serde_json::Value = serde_json::from_str(&ticket.text()).unwrap();
        assert_eq!(body["code"], "SSH_SESSION_NOT_FOUND");

        let close = request
            .delete("/api/admin/ssh/sessions/does-not-exist")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(close.status_code(), 404);
    })
    .await;
}

async fn login_token(request: &TestServer, email: &str, password: &str) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let login = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.4.{ip_suffix}"))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&login.text()).unwrap();
    login.data.unwrap().token
}

#[tokio::test]
#[serial]
async fn ssh_sessions_are_private_to_their_owner() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = login_token(&request, "admin@example.com", "1234").await;
        let (admin_key, admin_value) = prepare_data::auth_header(&admin);
        prepare_data::confirm_password(&request, &admin, "1234").await;

        // A second operator with the same SSH permissions (super admin role).
        let created = request
            .post("/api/admin/users")
            .add_header(admin_key.clone(), admin_value.clone())
            .json(&serde_json::json!({
                "name": "运维二号", "email": "ops2@example.com", "password": "Ops2pass123"
            }))
            .await;
        let created: serde_json::Value = serde_json::from_str(&created.text()).unwrap();
        let other_id = created["data"]["id"].as_i64().unwrap();
        request
            .put(&format!("/api/admin/users/{other_id}/roles"))
            .add_header(admin_key.clone(), admin_value.clone())
            .json(&serde_json::json!({ "role_ids": [1] }))
            .await;
        let other = login_token(&request, "ops2@example.com", "Ops2pass123").await;
        let (other_key, other_value) = prepare_data::auth_header(&other);

        let session = request
            .post("/api/admin/ssh/sessions")
            .add_header(admin_key.clone(), admin_value.clone())
            .json(&serde_json::json!({ "target_key": "local-shell" }))
            .await;
        assert_eq!(session.status_code(), 200, "{}", session.text());
        let session: serde_json::Value = serde_json::from_str(&session.text()).unwrap();
        let id = session["data"]["id"].as_str().unwrap().to_string();

        let listed = |key: axum::http::HeaderName, value: axum::http::HeaderValue| {
            let request = &request;
            async move {
                let response = request
                    .get("/api/admin/ssh/sessions")
                    .add_header(key, value)
                    .await;
                let body: serde_json::Value = serde_json::from_str(&response.text()).unwrap();
                body["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|session| session["id"].as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()
            }
        };
        // Sessions live in memory for the whole test process; compare ids.
        assert!(listed(admin_key.clone(), admin_value.clone())
            .await
            .contains(&id));
        assert!(!listed(other_key.clone(), other_value.clone())
            .await
            .contains(&id));

        // Someone else's shell cannot be attached to, browsed or closed.
        let attach = request
            .post(&format!("/api/admin/ssh/sessions/{id}/tickets"))
            .add_header(other_key.clone(), other_value.clone())
            .await;
        assert_eq!(attach.status_code(), 404);
        let files = request
            .get(&format!("/api/admin/ssh/sessions/{id}/files"))
            .add_header(other_key.clone(), other_value.clone())
            .await;
        assert_eq!(files.status_code(), 404);
        let close = request
            .delete(&format!("/api/admin/ssh/sessions/{id}"))
            .add_header(other_key, other_value)
            .await;
        assert_eq!(close.status_code(), 404);

        let own_attach = request
            .post(&format!("/api/admin/ssh/sessions/{id}/tickets"))
            .add_header(admin_key.clone(), admin_value.clone())
            .await;
        assert_eq!(own_attach.status_code(), 200);
        let own_close = request
            .delete(&format!("/api/admin/ssh/sessions/{id}"))
            .add_header(admin_key, admin_value)
            .await;
        assert_eq!(own_close.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn ssh_tabs_can_be_renamed_by_their_owner() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = login_token(&request, "admin@example.com", "1234").await;
        let (key, value) = prepare_data::auth_header(&admin);
        prepare_data::confirm_password(&request, &admin, "1234").await;

        let session = request
            .post("/api/admin/ssh/sessions")
            .add_header(key.clone(), value.clone())
            .json(&serde_json::json!({ "target_key": "local-shell", "title": "构建机" }))
            .await;
        assert_eq!(session.status_code(), 200, "{}", session.text());
        let session: serde_json::Value = serde_json::from_str(&session.text()).unwrap();
        assert_eq!(session["data"]["title"], "构建机");
        let id = session["data"]["id"].as_str().unwrap().to_string();
        let path = format!("/api/admin/ssh/sessions/{id}");

        let renamed = request
            .put(&path)
            .add_header(key.clone(), value.clone())
            .json(&serde_json::json!({ "title": "  部署\n日志  " }))
            .await;
        assert_eq!(renamed.status_code(), 200, "{}", renamed.text());
        let renamed: serde_json::Value = serde_json::from_str(&renamed.text()).unwrap();
        // Surrounding spaces and control characters are dropped.
        assert_eq!(renamed["data"]["title"], "部署日志");

        let listed = request
            .get("/api/admin/ssh/sessions")
            .add_header(key.clone(), value.clone())
            .await;
        let listed: serde_json::Value = serde_json::from_str(&listed.text()).unwrap();
        let listed = listed["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id.as_str())
            .unwrap()
            .clone();
        assert_eq!(listed["title"], "部署日志");

        let too_long = request
            .put(&path)
            .add_header(key.clone(), value.clone())
            .json(&serde_json::json!({ "title": "长".repeat(41) }))
            .await;
        assert_eq!(too_long.status_code(), 400);

        // An empty name goes back to the default (the target name).
        let cleared = request
            .put(&path)
            .add_header(key.clone(), value.clone())
            .json(&serde_json::json!({ "title": "   " }))
            .await;
        let cleared: serde_json::Value = serde_json::from_str(&cleared.text()).unwrap();
        assert!(cleared["data"]["title"].is_null());

        // Someone else's tab cannot be renamed.
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (other_key, other_value) = prepare_data::auth_header(&user.token);
        let foreign = request
            .put(&path)
            .add_header(other_key, other_value)
            .json(&serde_json::json!({ "title": "x" }))
            .await;
        assert_ne!(foreign.status_code(), 200);

        let close = request.delete(&path).add_header(key, value).await;
        assert_eq!(close.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn local_session_file_browser_lists_and_climbs_directories() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let login = request
            .post("/api/auth/login")
            .add_header("x-forwarded-for", format!("127.0.4.{ip_suffix}"))
            .json(&serde_json::json!({ "email": "admin@example.com", "password": "1234" }))
            .await;
        let login: ApiResponse<LoginResponse> = serde_json::from_str(&login.text()).unwrap();
        let token = login.data.unwrap().token;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        // Remote connections skip the password confirmation by default.
        let unconfirmed = request
            .post("/api/admin/ssh/sessions")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "target_key": "local-shell" }))
            .await;
        assert_eq!(unconfirmed.status_code(), 200, "{}", unconfirmed.text());
        let unconfirmed: serde_json::Value = serde_json::from_str(&unconfirmed.text()).unwrap();
        let closed = request
            .delete(&format!(
                "/api/admin/ssh/sessions/{}",
                unconfirmed["data"]["id"].as_str().unwrap()
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(closed.status_code(), 200);

        // With the remote switch on, opening a shell needs a fresh password.
        set_setting(&ctx, security_settings::REMOTE_REAUTH_ENABLED, "true").await;
        let unconfirmed = request
            .post("/api/admin/ssh/sessions")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "target_key": "local-shell" }))
            .await;
        assert_eq!(unconfirmed.status_code(), 403);
        assert!(unconfirmed.text().contains("REAUTH_REQUIRED"));
        prepare_data::confirm_password(&request, &token, "1234").await;
        set_setting(&ctx, security_settings::REMOTE_REAUTH_ENABLED, "false").await;

        let session = request
            .post("/api/admin/ssh/sessions")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "target_key": "local-shell" }))
            .await;
        assert_eq!(session.status_code(), 200, "{}", session.text());
        let session: serde_json::Value = serde_json::from_str(&session.text()).unwrap();
        let session_id = session["data"]["id"].as_str().unwrap().to_string();
        let files_path = format!("/api/admin/ssh/sessions/{session_id}/files");

        // Opens at the session's working directory (a drive path on Windows).
        let listing = request
            .get(&files_path)
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(listing.status_code(), 200, "{}", listing.text());
        let listing: serde_json::Value = serde_json::from_str(&listing.text()).unwrap();
        let current = listing["data"]["path"].as_str().unwrap().to_string();
        assert!(
            !current.starts_with(r"\\?\"),
            "verbatim path leaked: {current}"
        );
        let parent = listing["data"]["parent_path"]
            .as_str()
            .expect("the working directory has a parent")
            .to_string();

        let up = request
            .get(&files_path)
            .add_query_param("path", &parent)
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(up.status_code(), 200, "{}", up.text());
        let up: serde_json::Value = serde_json::from_str(&up.text()).unwrap();
        assert_eq!(up["data"]["path"], parent);
        let child = up["data"]["directories"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|entry| entry["path"].as_str())
            .expect("the parent lists at least the working directory")
            .to_string();

        let down = request
            .get(&files_path)
            .add_query_param("path", &child)
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(down.status_code(), 200, "{}", down.text());

        let close = request
            .delete(&format!("/api/admin/ssh/sessions/{session_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(close.status_code(), 200);
    })
    .await;
}
