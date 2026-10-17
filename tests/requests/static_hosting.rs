use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use base64::Engine as _;
use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{testing::prelude::*, TestServer};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn admin_token(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.4.{ip_suffix}"))
        .json(&serde_json::json!({
            "email": "admin@example.com",
            "password": "1234"
        }))
        .await;
    assert_eq!(response.status_code(), 200);
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

fn basic(username: &str, password: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
    )
}

fn site_payload(extra: serde_json::Value) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "key": "locked-site",
        "name": "密码保护站点",
        "url_path": "/locked-static",
        "local_root": "storage/static-hosting/locked-tests",
        "index_file": "index.html",
        "directory_listing": true,
        "enabled": true,
        "cache_seconds": 60
    });
    payload
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    payload
}

#[tokio::test]
#[serial]
async fn static_site_directories_can_require_a_password_and_list_over_index() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);
        let root = PathBuf::from("storage/static-hosting/locked-tests");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }

        let missing_password = request
            .post("/api/admin/static-sites")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&site_payload(serde_json::json!({
                "access_rules": [{ "path": "private", "username": "alice" }]
            })))
            .await;
        assert_eq!(missing_password.status_code(), 400);

        let create = request
            .post("/api/admin/static-sites")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&site_payload(serde_json::json!({
                "access_rules": [
                    { "path": "private", "username": "alice", "password": "open-sesame" }
                ]
            })))
            .await;
        assert_eq!(create.status_code(), 200);
        let created = create.text();
        assert!(created.contains(r#""username":"alice""#));
        assert!(!created.contains("password_hash"));
        assert!(!created.contains("open-sesame"));

        fs::write(root.join("index.html"), b"<h1>home</h1>").unwrap();
        fs::create_dir_all(root.join("private")).unwrap();
        fs::write(root.join("private/report.txt"), b"secret report").unwrap();

        let listed_sites = request
            .get("/api/admin/static-sites")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert!(listed_sites.text().contains(r#""path":"private""#));
        assert!(!listed_sites.text().contains("password_hash"));

        // 未保护的内容照常访问；目录有首页时默认返回首页。
        let home = request.get("/locked-static/").await;
        assert_eq!(home.as_bytes().as_ref(), b"<h1>home</h1>");

        let anonymous = request.get("/locked-static/private/report.txt").await;
        assert_eq!(anonymous.status_code(), 401);
        let challenge = anonymous
            .headers()
            .get("www-authenticate")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(challenge.starts_with("Basic realm=\"locked-site/private\""));

        // 不存在的文件同样先要求认证，避免探测目录内容；大小写不同的路径也受保护。
        let probe = request.get("/locked-static/private/missing.txt").await;
        assert_eq!(probe.status_code(), 401);
        let upper = request.get("/locked-static/PRIVATE/report.txt").await;
        assert_eq!(upper.status_code(), 401);

        let wrong = request
            .get("/locked-static/private/report.txt")
            .add_header("authorization", basic("alice", "wrong"))
            .await;
        assert_eq!(wrong.status_code(), 401);

        let granted = request
            .get("/locked-static/private/report.txt")
            .add_header("authorization", basic("alice", "open-sesame"))
            .await;
        assert_eq!(granted.status_code(), 200);
        assert_eq!(granted.as_bytes().as_ref(), b"secret report");
        let cache_control = granted
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(cache_control.starts_with("private"));

        // 编辑站点时不填密码会保留原密码，同时开启“有首页也显示列表”。
        let update = request
            .put("/api/admin/static-sites/locked-site")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&site_payload(serde_json::json!({
                "listing_overrides_index": true,
                "access_rules": [{ "path": "private", "username": "alice" }]
            })))
            .await;
        assert_eq!(update.status_code(), 200);

        let still_granted = request
            .get("/locked-static/private/report.txt")
            .add_header("authorization", basic("alice", "open-sesame"))
            .await;
        assert_eq!(still_granted.status_code(), 200);

        let listing = request.get("/locked-static/").await;
        assert_eq!(listing.status_code(), 200);
        let listing_text = listing.text();
        assert!(listing_text.contains(r#"href="/locked-static/index.html""#));
        assert!(listing_text.contains(r#"href="/locked-static/private/""#));
        assert!(listing_text.contains("🔒"));

        let remove_rules = request
            .put("/api/admin/static-sites/locked-site")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&site_payload(serde_json::json!({ "access_rules": [] })))
            .await;
        assert_eq!(remove_rules.status_code(), 200);
        let public_again = request.get("/locked-static/private/report.txt").await;
        assert_eq!(public_again.status_code(), 200);

        let delete_site = request
            .delete("/api/admin/static-sites/locked-site")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_site.status_code(), 200);
        fs::remove_dir_all(root).unwrap();
    })
    .await;
}
