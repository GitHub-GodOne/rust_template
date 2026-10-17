use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::testing::prelude::*;
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

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
