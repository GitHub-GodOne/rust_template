use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{testing::prelude::*, TestServer};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);
const PATH: &str = "/api/user/preferences/dock";

async fn admin_token(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.6.{ip_suffix}"))
        .json(&serde_json::json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

async fn get_dock(request: &TestServer, token: &str) -> serde_json::Value {
    let (key, value) = prepare_data::auth_header(token);
    let response = request.get(PATH).add_header(key, value).await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    serde_json::from_str::<serde_json::Value>(&response.text()).unwrap()["data"].clone()
}

#[tokio::test]
#[serial]
async fn preferences_require_auth() {
    request::<App, _, _>(|request, _ctx| async move {
        assert_eq!(request.get(PATH).await.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn each_user_keeps_their_own_dock_preference() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = admin_token(&request).await;
        let (admin_key, admin_value) = prepare_data::auth_header(&admin);

        assert!(get_dock(&request, &admin).await["value"].is_null());

        let unknown = request
            .put("/api/user/preferences/theme-hack")
            .add_header(admin_key.clone(), admin_value.clone())
            .json(&serde_json::json!({ "value": {} }))
            .await;
        assert_eq!(unknown.status_code(), 400, "only known keys are stored");

        let not_object = request
            .put(PATH)
            .add_header(admin_key.clone(), admin_value.clone())
            .json(&serde_json::json!({ "value": [1, 2] }))
            .await;
        assert_eq!(not_object.status_code(), 400);

        let too_large = request
            .put(PATH)
            .add_header(admin_key.clone(), admin_value.clone())
            .json(&serde_json::json!({ "value": { "blob": "x".repeat(20_000) } }))
            .await;
        assert_eq!(too_large.status_code(), 400);

        let dock = serde_json::json!({
            "version": 1,
            "position": "left",
            "items": [{ "path": "/admin/system/users" }]
        });
        for _ in 0..2 {
            // Saving twice updates the same row instead of failing.
            let saved = request
                .put(PATH)
                .add_header(admin_key.clone(), admin_value.clone())
                .json(&serde_json::json!({ "value": dock }))
                .await;
            assert_eq!(saved.status_code(), 200, "{}", saved.text());
        }
        assert_eq!(get_dock(&request, &admin).await["value"], dock);

        let other = prepare_data::init_user_login(&request, &ctx).await;
        assert!(
            get_dock(&request, &other.token).await["value"].is_null(),
            "another user does not see the admin's dock"
        );

        let reset = request
            .delete(PATH)
            .add_header(admin_key, admin_value)
            .await;
        assert_eq!(reset.status_code(), 200);
        assert!(get_dock(&request, &admin).await["value"].is_null());
    })
    .await;
}
