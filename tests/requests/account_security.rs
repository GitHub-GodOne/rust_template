//! Login sessions, account status, password resets, the admin IP allowlist
//! and API-docs access.

use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{
    app::App,
    models::{
        _entities::{system_settings, users},
        user_sessions,
    },
    services::security_settings,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data::{self, auth_header, confirm_password};

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);
const ADMIN_EMAIL: &str = "admin@example.com";
const ADMIN_PASSWORD: &str = "1234";

struct Login {
    token: String,
    refresh_token: String,
}

async fn login_from(request: &TestServer, email: &str, password: &str, ip: &str) -> Value {
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", ip.to_string())
        .json(&json!({ "email": email, "password": password }))
        .await;
    serde_json::from_str(&response.text()).unwrap()
}

async fn login(request: &TestServer, email: &str, password: &str) -> Login {
    let suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let body = login_from(request, email, password, &format!("127.0.8.{suffix}")).await;
    assert_eq!(body["success"], true, "{body}");
    Login {
        token: body["data"]["token"].as_str().unwrap().to_string(),
        refresh_token: body["data"]["refresh_token"].as_str().unwrap().to_string(),
    }
}

async fn get(request: &TestServer, path: &str, token: &str) -> (u16, Value) {
    let (key, value) = auth_header(token);
    let response = request.get(path).add_header(key, value).await;
    (
        response.status_code().as_u16(),
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

async fn send(
    request: &TestServer,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    let (key, value) = auth_header(token);
    let builder = match method {
        "POST" => request.post(path),
        "PUT" => request.put(path),
        "DELETE" => request.delete(path),
        _ => unreachable!(),
    };
    let response = builder.add_header(key, value).json(&body).await;
    (
        response.status_code().as_u16(),
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

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
async fn logout_and_revoked_sessions_end_access_immediately() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let laptop = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;
        let phone = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;

        let (status, sessions) = get(&request, "/api/user/profile/sessions", &laptop.token).await;
        assert_eq!(status, 200);
        let sessions = sessions["data"].as_array().unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            sessions
                .iter()
                .filter(|session| session["current"] == true)
                .count(),
            1
        );

        // Signing out the other devices kills the phone's access token now,
        // not when it expires, and its refresh token too.
        let (status, _) = send(
            &request,
            "POST",
            "/api/user/profile/sessions/revoke-others",
            &laptop.token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        let (status, body) = get(&request, "/api/auth/current", &phone.token).await;
        assert_eq!(status, 401);
        assert_eq!(body["code"], "SESSION_EXPIRED");
        let refresh = request
            .post("/api/auth/refresh")
            .json(&json!({ "refresh_token": phone.refresh_token }))
            .await;
        assert_eq!(refresh.status_code(), 401);
        assert_eq!(
            get(&request, "/api/auth/current", &laptop.token).await.0,
            200
        );

        // Refreshing keeps the same session.
        let refreshed: Value = serde_json::from_str(
            &request
                .post("/api/auth/refresh")
                .json(&json!({ "refresh_token": laptop.refresh_token }))
                .await
                .text(),
        )
        .unwrap();
        let token = refreshed["data"]["token"].as_str().unwrap().to_string();
        let refresh_token = refreshed["data"]["refresh_token"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(get(&request, "/api/auth/current", &token).await.0, 200);

        let logout = request
            .post("/api/auth/logout")
            .json(&json!({ "refresh_token": refresh_token }))
            .await;
        assert_eq!(logout.status_code(), 200);
        assert_eq!(get(&request, "/api/auth/current", &token).await.0, 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admins_disable_unlock_and_reset_users() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let member = prepare_data::init_user_login(&request, &ctx).await;
        let admin = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;
        let user_id = member.user.id;

        // Disabling ends the member's session and blocks new logins.
        let (status, body) = send(
            &request,
            "PUT",
            &format!("/api/admin/users/{user_id}/status"),
            &admin.token,
            json!({ "status": "disabled" }),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["status"], "disabled");
        assert_eq!(
            get(&request, "/api/auth/current", &member.token).await.0,
            401
        );
        let denied = login_from(&request, &member.user.email, "1234", "127.0.8.200").await;
        assert_eq!(denied["code"], "ACCOUNT_DISABLED");
        let (status, _) = send(
            &request,
            "PUT",
            "/api/admin/users/3/status",
            &admin.token,
            json!({ "status": "disabled" }),
        )
        .await;
        assert_eq!(status, 400, "an admin cannot disable themselves");

        send(
            &request,
            "PUT",
            &format!("/api/admin/users/{user_id}/status"),
            &admin.token,
            json!({ "status": "active" }),
        )
        .await;

        // Unlock clears a login lockout.
        let user = users::Entity::find_by_id(user_id)
            .one(&ctx.db)
            .await
            .unwrap()
            .unwrap();
        let mut locked = user.into_active_model();
        locked.failed_login_attempts = Set(3);
        locked.locked_until = Set(Some(
            (chrono::Local::now() + chrono::Duration::hours(2)).into(),
        ));
        locked.update(&ctx.db).await.unwrap();
        let (_, listed) = get(
            &request,
            "/api/admin/users?keyword=test%40loco",
            &admin.token,
        )
        .await;
        assert_eq!(listed["data"]["items"][0]["locked"], true);
        let (status, body) = send(
            &request,
            "POST",
            &format!("/api/admin/users/{user_id}/unlock"),
            &admin.token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["locked"], false);

        // Resetting a password is sensitive: it needs a fresh confirmation.
        let reset_path = format!("/api/admin/users/{user_id}/reset-password");
        let (status, body) = send(
            &request,
            "POST",
            &reset_path,
            &admin.token,
            json!({ "password": "Newpass123" }),
        )
        .await;
        assert_eq!(status, 403);
        assert_eq!(body["code"], "REAUTH_REQUIRED");
        let (status, body) = send(
            &request,
            "POST",
            "/api/auth/reauth",
            &admin.token,
            json!({ "password": "wrong" }),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(body["code"], "WRONG_PASSWORD");
        confirm_password(&request, &admin.token, ADMIN_PASSWORD).await;
        let (status, _) = send(
            &request,
            "POST",
            &reset_path,
            &admin.token,
            json!({ "password": "short" }),
        )
        .await;
        assert_eq!(status, 400, "weak passwords are rejected");
        let (status, body) = send(
            &request,
            "POST",
            &reset_path,
            &admin.token,
            json!({ "password": "Newpass123" }),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let relogin = login_from(&request, &member.user.email, "Newpass123", "127.0.8.201").await;
        assert_eq!(relogin["success"], true, "{relogin}");

        // Export follows the same filters as the list.
        let (key, value) = auth_header(&admin.token);
        let export = request
            .get("/api/admin/users/export?status=active")
            .add_header(key, value)
            .await;
        assert_eq!(export.status_code(), 200);
        let csv = export.text();
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("姓名,邮箱,状态"));
        assert!(csv.contains(ADMIN_EMAIL));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admins_see_and_end_other_sessions() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let member = prepare_data::init_user_login(&request, &ctx).await;
        let admin = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;

        let (status, body) = get(
            &request,
            &format!("/api/admin/sessions?user_id={}", member.user.id),
            &admin.token,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["total"], 1);
        let session_id = body["data"]["items"][0]["id"].as_i64().unwrap();
        assert_eq!(
            body["data"]["items"][0]["user_email"],
            member.user.email.as_str()
        );

        let (status, _) = send(
            &request,
            "DELETE",
            &format!("/api/admin/sessions/{session_id}"),
            &admin.token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(
            get(&request, "/api/auth/current", &member.token).await.0,
            401
        );
        let (_, ended) = get(
            &request,
            &format!(
                "/api/admin/sessions?user_id={}&status=ended",
                member.user.id
            ),
            &admin.token,
        )
        .await;
        assert_eq!(ended["data"]["items"][0]["revoked_reason"], "admin");

        // Regular users cannot list sessions.
        let again = prepare_data::init_user_login(&request, &ctx).await;
        assert_eq!(
            get(&request, "/api/admin/sessions", &again.token).await.0,
            403
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn users_change_their_own_password() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let first = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;
        let second = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;

        let (status, profile) = get(&request, "/api/user/profile", &first.token).await;
        assert_eq!(status, 200);
        assert_eq!(profile["data"]["email"], ADMIN_EMAIL);
        assert!(profile["data"]["last_login_at"].is_string());

        let (status, body) = send(
            &request,
            "PUT",
            "/api/user/profile",
            &first.token,
            json!({ "name": "  管理员  " }),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["name"], "管理员");

        let path = "/api/user/profile/password";
        let (status, body) = send(
            &request,
            "POST",
            path,
            &first.token,
            json!({ "current_password": "nope", "new_password": "Changed123" }),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(body["code"], "WRONG_PASSWORD");
        let (status, body) = send(
            &request,
            "POST",
            path,
            &first.token,
            json!({ "current_password": ADMIN_PASSWORD, "new_password": "Changed123" }),
        )
        .await;
        assert_eq!(status, 200, "{body}");

        // The other device is signed out; this one stays.
        assert_eq!(
            get(&request, "/api/auth/current", &second.token).await.0,
            401
        );
        assert_eq!(
            get(&request, "/api/auth/current", &first.token).await.0,
            200
        );
        assert!(
            user_sessions::Entity::find()
                .all(&ctx.db)
                .await
                .unwrap()
                .len()
                >= 2
        );
        let relogin = login_from(&request, ADMIN_EMAIL, "Changed123", "127.0.8.202").await;
        assert_eq!(relogin["success"], true);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_ip_allowlist_blocks_other_addresses() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let office_ip = "203.0.113.5";
        let body = login_from(&request, ADMIN_EMAIL, ADMIN_PASSWORD, office_ip).await;
        let token = body["data"]["token"].as_str().unwrap().to_string();
        confirm_password_from(&request, &token, office_ip).await;

        let setting = system_settings::Entity::find()
            .filter(system_settings::Column::Key.eq(security_settings::ADMIN_IP_ALLOWLIST))
            .one(&ctx.db)
            .await
            .unwrap()
            .unwrap();
        let save = |value: &str| {
            let (key, header) = auth_header(&token);
            request
                .put(&format!("/api/admin/settings/{}", setting.id))
                .add_header(key, header)
                .add_header("x-forwarded-for", office_ip)
                .json(&json!({
                    "key": setting.key,
                    "name": setting.name,
                    "group_key": setting.group_key,
                    "value": value,
                    "value_type": setting.value_type,
                    "is_builtin": true,
                }))
        };
        let locked_out = save("198.51.100.0/24").await;
        assert_eq!(locked_out.status_code(), 400, "would lock the editor out");
        let saved = save("203.0.113.0/24").await;
        assert_eq!(saved.status_code(), 200, "{}", saved.text());

        let blocked = login_from(&request, ADMIN_EMAIL, ADMIN_PASSWORD, "198.51.100.7").await;
        assert_eq!(blocked["code"], "IP_NOT_ALLOWED");
        let allowed = login_from(&request, ADMIN_EMAIL, ADMIN_PASSWORD, "203.0.113.77").await;
        assert_eq!(allowed["success"], true);
        // Public file links stay reachable.
        let public = request
            .get("/api/uploads/999999")
            .add_header("x-forwarded-for", "198.51.100.7")
            .await;
        assert_ne!(public.status_code(), 403);

        set_setting(&ctx, security_settings::ADMIN_IP_ALLOWLIST, "").await;
    })
    .await;
}

async fn confirm_password_from(request: &TestServer, token: &str, ip: &str) {
    let (key, value) = auth_header(token);
    let response = request
        .post("/api/auth/reauth")
        .add_header(key, value)
        .add_header("x-forwarded-for", ip.to_string())
        .json(&json!({ "password": ADMIN_PASSWORD }))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
}

#[tokio::test]
#[serial]
async fn notification_read_state_is_per_user() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;
        let member = prepare_data::init_user_login(&request, &ctx).await;
        let unread = |token: String| {
            let request = &request;
            async move {
                let (status, body) = get(request, "/api/user/notifications", &token).await;
                assert_eq!(status, 200, "{body}");
                body["data"]["unread_count"].as_u64().unwrap()
            }
        };
        let member_before = unread(member.token.clone()).await;
        let admin_before = unread(admin.token.clone()).await;

        let (status, created) = send(
            &request,
            "POST",
            "/api/admin/notifications",
            &admin.token,
            json!({
                "title": "系统维护",
                "content": "今晚 22:00 维护",
                "level": "info",
                "category": "system",
                "target_type": "all"
            }),
        )
        .await;
        assert_eq!(status, 200, "{created}");
        let id = created["data"]["id"].as_i64().unwrap();
        assert_eq!(unread(member.token.clone()).await, member_before + 1);

        let (status, _) = send(
            &request,
            "POST",
            &format!("/api/user/notifications/{id}/read"),
            &member.token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(unread(member.token.clone()).await, member_before);
        assert_eq!(
            unread(admin.token.clone()).await,
            admin_before + 1,
            "still unread for others"
        );

        send(
            &request,
            "POST",
            "/api/user/notifications/read-all",
            &admin.token,
            json!({}),
        )
        .await;
        assert_eq!(unread(admin.token.clone()).await, 0);

        // Live events need a ticket; a forged one is refused.
        let (status, ticket) = send(
            &request,
            "POST",
            "/api/user/events/ticket",
            &member.token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        assert!(ticket["data"]["ticket"].is_string());
        assert_eq!(
            request
                .get("/api/user/events?ticket=forged")
                .await
                .status_code(),
            401
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn api_docs_follow_the_access_setting() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = login(&request, ADMIN_EMAIL, ADMIN_PASSWORD).await;

        assert_eq!(
            request.get("/api-docs/openapi.json").await.status_code(),
            401
        );
        let (key, value) = auth_header(&admin.token);
        let with_token = request
            .get("/api-docs/openapi.json")
            .add_header(key, value)
            .await;
        assert_eq!(with_token.status_code(), 200);

        let (status, ticket) = send(
            &request,
            "POST",
            "/api/admin/docs/ticket",
            &admin.token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(ticket["data"]["mode"], "login");
        let ticket = ticket["data"]["ticket"].as_str().unwrap();
        let page = request.get(&format!("/swagger-ui?ticket={ticket}")).await;
        assert_eq!(page.status_code(), 200);
        assert!(page.text().contains("SwaggerUIBundle"));
        assert_eq!(
            request.get("/swagger-ui?ticket=forged").await.status_code(),
            401
        );

        set_setting(&ctx, security_settings::API_DOCS_ACCESS, "public").await;
        assert_eq!(request.get("/swagger-ui").await.status_code(), 200);
        set_setting(&ctx, security_settings::API_DOCS_ACCESS, "disabled").await;
        assert_eq!(
            request.get("/api-docs/openapi.json").await.status_code(),
            404
        );
        set_setting(&ctx, security_settings::API_DOCS_ACCESS, "login").await;
    })
    .await;
}
