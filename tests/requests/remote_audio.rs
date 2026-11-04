use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Duration, Utc};
use gpt_images::{
    app::App, controllers::admin::remote_audio, models::_entities::remote_audio_sessions,
    models::users, responses::ApiResponse, views::auth::LoginResponse,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login_token(request: &TestServer, email: &str, password: &str) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let login = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.7.{ip_suffix}"))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&login.text()).unwrap();
    login.data.unwrap().token
}

async fn get_json(request: &TestServer, path: &str, token: &str) -> serde_json::Value {
    let (key, value) = prepare_data::auth_header(token);
    let response = request.get(path).add_header(key, value).await;
    assert_eq!(response.status_code(), 200, "{path}: {}", response.text());
    serde_json::from_str::<serde_json::Value>(&response.text()).unwrap()["data"].clone()
}

#[tokio::test]
#[serial]
async fn listening_needs_permission_and_a_known_quality() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = login_token(&request, "admin@example.com", "1234").await;

        let status = get_json(&request, "/api/admin/remote-audio/status", &admin).await;
        let qualities: Vec<&str> = status["qualities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|quality| quality["key"].as_str().unwrap())
            .collect();
        assert_eq!(qualities, ["high", "standard", "low"]);
        assert_eq!(status["default_quality"], "standard");
        assert_eq!(status["qualities"][2]["bitrate_kbps"], 256);

        let (key, value) = prepare_data::auth_header(&admin);
        let unknown = request
            .post("/api/admin/remote-audio/tickets")
            .add_header(key.clone(), value.clone())
            .json(&serde_json::json!({ "quality": "lossless" }))
            .await;
        assert_eq!(unknown.status_code(), 400);
        let ticket = request
            .post("/api/admin/remote-audio/tickets")
            .add_header(key.clone(), value.clone())
            .json(&serde_json::json!({}))
            .await;
        assert_eq!(ticket.status_code(), 200, "{}", ticket.text());
        let body: serde_json::Value = serde_json::from_str(&ticket.text()).unwrap();
        assert!(!body["data"]["ticket"].as_str().unwrap().is_empty());

        // An audio session that does not exist cannot be force-closed.
        let close = request
            .delete("/api/admin/sessions/remote/audio/999999")
            .add_header(key, value)
            .await;
        assert_eq!(close.status_code(), 404);

        // A user without a role sees neither the page nor the sound.
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (key, value) = prepare_data::auth_header(&user.token);
        let denied = request
            .get("/api/admin/remote-audio/status")
            .add_header(key.clone(), value.clone())
            .await;
        assert_eq!(denied.status_code(), 403);
        let denied = request
            .post("/api/admin/remote-audio/tickets")
            .add_header(key, value)
            .json(&serde_json::json!({}))
            .await;
        assert_eq!(denied.status_code(), 403);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn every_listening_session_records_its_traffic() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin_user = users::Model::find_by_email(&ctx.db, "admin@example.com")
            .await
            .unwrap();
        let admin = login_token(&request, "admin@example.com", "1234").await;
        let started = Utc::now() - Duration::minutes(10);
        for (offset, bytes, status) in [
            (0, 1_000_000_i64, "ended"),
            (2, 250_000, "ended"),
            (5, 4_096, "live"),
        ] {
            remote_audio_sessions::ActiveModel {
                quality: Set("standard".to_string()),
                sample_rate: Set(24_000),
                channels: Set(2),
                bytes_sent: Set(bytes),
                audio_ms: Set(bytes / 96),
                status: Set(status.to_string()),
                close_reason: Set((status == "ended").then(|| "client".to_string())),
                started_at: Set((started + Duration::minutes(offset)).into()),
                ended_at: Set(
                    (status == "ended").then(|| (started + Duration::minutes(offset + 1)).into())
                ),
                user_id: Set(Some(admin_user.id)),
                ..Default::default()
            }
            .insert(&ctx.db)
            .await
            .unwrap();
        }

        let page = get_json(&request, "/api/admin/remote-audio/sessions", &admin).await;
        assert_eq!(page["total"], 3);
        assert_eq!(page["total_bytes"], 1_254_096);
        // Newest first; each row knows its user and how long it lasted.
        assert_eq!(page["items"][0]["status"], "live");
        assert_eq!(page["items"][1]["bytes_sent"], 250_000);
        assert_eq!(page["items"][1]["duration_ms"], 60_000);
        assert_eq!(page["items"][1]["user_email"], "admin@example.com");

        let ended = get_json(
            &request,
            "/api/admin/remote-audio/sessions?status=ended",
            &admin,
        )
        .await;
        assert_eq!(ended["total"], 2);
        assert_eq!(ended["total_bytes"], 1_250_000);
        let nobody = get_json(
            &request,
            "/api/admin/remote-audio/sessions?keyword=nobody-listens",
            &admin,
        )
        .await;
        assert_eq!(nobody["total"], 0);
        assert_eq!(nobody["total_bytes"], 0);

        // A session a stopped server left open is closed on the next start.
        remote_audio::finish_stale_sessions(&ctx.db).await;
        let rows = remote_audio_sessions::Entity::find()
            .all(&ctx.db)
            .await
            .unwrap();
        assert!(rows.iter().all(|row| row.status == "ended"));
        let stale = rows.iter().find(|row| row.bytes_sent == 4_096).unwrap();
        assert_eq!(stale.close_reason.as_deref(), Some("server_stopped"));
        assert!(stale.ended_at.is_some());
    })
    .await;
}
