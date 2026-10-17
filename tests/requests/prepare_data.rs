use std::sync::atomic::{AtomicUsize, Ordering};

use axum::http::{HeaderName, HeaderValue};
use gpt_images::{models::users, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{app::AppContext, TestServer};

const USER_EMAIL: &str = "test@loco.com";
const USER_PASSWORD: &str = "1234";

pub struct LoggedInUser {
    pub user: users::Model,
    pub token: String,
}

static HELPER_LOGIN_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub async fn init_user_login(request: &TestServer, ctx: &AppContext) -> LoggedInUser {
    let register_payload = serde_json::json!({
        "name": "loco",
        "email": USER_EMAIL,
        "password": USER_PASSWORD
    });

    //Creating a new user
    request
        .post("/api/auth/register")
        .json(&register_payload)
        .await;
    let user = users::Model::find_by_email(&ctx.db, USER_EMAIL)
        .await
        .unwrap();

    let verify_payload = serde_json::json!({
        "token": user.email_verification_token,
    });

    request.post("/api/auth/verify").json(&verify_payload).await;

    // A distinct client IP per login keeps the shared helper under the
    // per-IP login rate limit when many tests run back to back.
    let ip_suffix = HELPER_LOGIN_COUNTER.fetch_add(1, Ordering::Relaxed) % 250;
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.250.{}", ip_suffix + 1))
        .json(&serde_json::json!({
            "email": USER_EMAIL,
            "password": USER_PASSWORD
        }))
        .await;

    let login_response: ApiResponse<LoginResponse> =
        serde_json::from_str(&response.text()).unwrap();
    let login_data = login_response.data.unwrap();

    LoggedInUser {
        user: users::Model::find_by_email(&ctx.db, USER_EMAIL)
            .await
            .unwrap(),
        token: login_data.token,
    }
}

pub fn auth_header(token: &str) -> (HeaderName, HeaderValue) {
    let auth_header_value = HeaderValue::from_str(&format!("Bearer {}", &token)).unwrap();

    (HeaderName::from_static("authorization"), auth_header_value)
}
