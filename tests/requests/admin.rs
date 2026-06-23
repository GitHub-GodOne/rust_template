use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use axum::http::{HeaderName, HeaderValue};
use gpt_images::{
    app::App,
    models::_entities::{database_backups, payment_channels, system_settings, upload_files},
    responses::ApiResponse,
    views::auth::LoginResponse,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, EntityTrait, Set};
use serial_test::serial;
use tokio::time::sleep;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn admin_token(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.1.{ip_suffix}"))
        .json(&serde_json::json!({
            "email": "admin@example.com",
            "password": "1234"
        }))
        .await;
    assert_eq!(response.status_code(), 200);

    let login_response: ApiResponse<LoginResponse> =
        serde_json::from_str(&response.text()).unwrap();
    login_response.data.unwrap().token
}

async fn wait_for_command_run(
    request: &TestServer,
    auth_key: &HeaderName,
    auth_value: &HeaderValue,
    run_id: i64,
) -> serde_json::Value {
    for _ in 0..30 {
        let response = request
            .get(&format!("/api/admin/command-runs/{run_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = serde_json::from_str(&response.text()).unwrap();
        if !matches!(body["data"]["status"].as_str(), Some("queued" | "running")) {
            return body;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("command run did not finish in time");
}

async fn wait_for_command_workflow_run(
    request: &TestServer,
    auth_key: &HeaderName,
    auth_value: &HeaderValue,
    run_id: i64,
) -> serde_json::Value {
    for _ in 0..30 {
        let response = request
            .get(&format!("/api/admin/command-workflow-runs/{run_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = serde_json::from_str(&response.text()).unwrap();
        if !matches!(body["data"]["status"].as_str(), Some("queued" | "running")) {
            return body;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("command workflow run did not finish in time");
}

#[tokio::test]
#[serial]
async fn admin_users_requires_auth() {
    request::<App, _, _>(|request, _ctx| async move {
        let response = request.get("/api/admin/users").await;

        assert_eq!(response.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_extensions_require_auth() {
    request::<App, _, _>(|request, _ctx| async move {
        let ticket_response = request
            .post("/api/admin/ssh/tickets")
            .json(&serde_json::json!({ "target_key": "local-shell" }))
            .await;
        assert_eq!(ticket_response.status_code(), 401);

        let vnc_session_response = request
            .post("/api/admin/vnc/sessions")
            .json(&serde_json::json!({ "target_key": "local-vnc" }))
            .await;
        assert_eq!(vnc_session_response.status_code(), 401);

        let http_test_response = request
            .post("/api/admin/http-client/test")
            .json(&serde_json::json!({ "url": "https://example.test" }))
            .await;
        assert_eq!(http_test_response.status_code(), 401);

        for path in [
            "/api/admin/logs",
            "/api/admin/settings",
            "/api/admin/http-client/config",
            "/api/admin/ai-images/configs",
            "/api/admin/ai-images/generations",
            "/api/admin/ssh/targets",
            "/api/admin/vnc/targets",
            "/api/admin/email-templates",
            "/api/admin/dict-types",
            "/api/admin/dict-types/1/items",
            "/api/admin/uploads",
            "/api/admin/uploads/browser",
            "/api/admin/uploads/1/preview",
            "/api/admin/files/roots",
            "/api/admin/files/browser?root_key=public-assets",
            "/api/admin/files/preview?root_key=public-assets&path=hello.txt",
            "/api/admin/storage-profiles",
            "/api/admin/tenants",
            "/api/admin/departments",
            "/api/admin/data-scopes",
            "/api/admin/notifications",
            "/api/admin/scheduled-tasks",
            "/api/admin/scheduled-task-runs",
            "/api/admin/backups",
            "/api/admin/rate-limits",
            "/api/admin/rate-limit-events",
            "/api/admin/monitoring/overview",
            "/api/admin/monitoring/server",
            "/api/admin/monitoring/processes",
            "/api/admin/commands",
            "/api/admin/command-workflows",
            "/api/admin/command-workflow-runs",
            "/api/admin/command-runs",
            "/api/admin/work-orders",
            "/api/admin/payment-channels",
            "/api/admin/payment-orders",
            "/api/admin/payment-callbacks",
            "/api/admin/payment-refunds",
            "/api/admin/content-categories",
            "/api/admin/content-articles",
        ] {
            let response = request.get(path).await;
            assert_eq!(response.status_code(), 401, "{path}");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn embedded_frontend_serves_spa_without_hiding_api_routes() {
    request::<App, _, _>(|request, _ctx| async move {
        let index_response = request.get("/").await;
        assert_eq!(index_response.status_code(), 200);
        assert!(index_response.text().contains("id=\"root\""));

        let spa_response = request.get("/admin/dashboard").await;
        assert_eq!(spa_response.status_code(), 200);
        assert!(spa_response.text().contains("id=\"root\""));

        let swagger_response = request.get("/swagger-ui").await;
        assert_eq!(swagger_response.status_code(), 200);
        assert!(swagger_response.text().contains("SwaggerUIBundle"));

        let api_response = request.get("/api/admin/users").await;
        assert_eq!(api_response.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_access_admin_users() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let users_response = request
            .get("/api/admin/users")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(users_response.status_code(), 200);

        let current_response = request
            .get("/api/auth/current")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(current_response.status_code(), 200);
        let body = current_response.text();
        assert!(body.contains("super_admin"));
        assert!(body.contains("system:user:list"));
        assert!(body.contains("system:backup:restore"));
        assert!(body.contains("system:content_article:publish"));
        assert!(body.contains("system:docs:view"));
        assert!(body.contains("system:file:list"));
        assert!(body.contains("system:command:list"));
        assert!(body.contains("system:ssh:list"));
        assert!(body.contains("system:vnc:list"));
        assert!(body.contains("system:ai_image:list"));
        assert!(body.contains("/admin/system/users"));
        assert!(body.contains("/admin/system/content"));
        assert!(body.contains("/admin/system/docs"));
        assert!(body.contains("/admin/system/files"));
        assert!(body.contains("/admin/system/commands"));
        assert!(body.contains("/admin/system/ssh"));
        assert!(body.contains("/admin/system/vnc"));
        assert!(body.contains("/admin/system/ai-images"));
        assert!(body.contains("\"effective_data_scope\":\"all\""));
        assert!(body.contains("\"tenant\":{"));
        assert!(body.contains("\"departments_enabled\":true"));
        assert!(body.contains("\"departments\""));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_access_admin_extensions() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        for path in [
            "/api/admin/logs",
            "/api/admin/settings",
            "/api/admin/http-client/config",
            "/api/admin/ai-images/configs",
            "/api/admin/ai-images/generations",
            "/api/admin/ssh/targets",
            "/api/admin/vnc/targets",
            "/api/admin/email-templates",
            "/api/admin/dict-types",
            "/api/admin/dict-types/1/items",
            "/api/admin/uploads",
            "/api/admin/uploads/browser",
            "/api/admin/files/roots",
            "/api/admin/files/browser?root_key=public-assets",
            "/api/admin/storage-profiles",
            "/api/admin/tenants",
            "/api/admin/departments",
            "/api/admin/data-scopes",
        ] {
            let response = request
                .get(path)
                .add_header(auth_key.clone(), auth_value.clone())
                .await;
            assert_eq!(response.status_code(), 200, "{path}");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_http_client_config() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let config_response = request
            .get("/api/admin/http-client/config")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(config_response.status_code(), 200);
        let config_body: serde_json::Value = serde_json::from_str(&config_response.text()).unwrap();
        assert_eq!(config_body["data"]["request_timeout_seconds"], 120);
        assert_eq!(config_body["data"]["connect_timeout_seconds"], 20);
        assert_eq!(config_body["data"]["proxy_enabled"], false);

        let update_response = request
            .put("/api/admin/http-client/config")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "enabled": true,
                "request_timeout_seconds": 30,
                "connect_timeout_seconds": 5,
                "pool_idle_timeout_seconds": 45,
                "proxy_enabled": true,
                "proxy_url": "http://127.0.0.1:8888",
                "danger_accept_invalid_certs": false,
                "user_agent": "gpt-images-request-test/1.0"
            }))
            .await;
        assert_eq!(update_response.status_code(), 200);
        assert!(update_response
            .text()
            .contains("gpt-images-request-test/1.0"));

        let updated_config = request
            .get("/api/admin/http-client/config")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(updated_config.status_code(), 200);
        let updated_body: serde_json::Value = serde_json::from_str(&updated_config.text()).unwrap();
        assert_eq!(updated_body["data"]["request_timeout_seconds"], 30);
        assert_eq!(updated_body["data"]["connect_timeout_seconds"], 5);
        assert_eq!(updated_body["data"]["pool_idle_timeout_seconds"], 45);
        assert_eq!(updated_body["data"]["proxy_enabled"], true);

        let invalid_timeout = request
            .put("/api/admin/http-client/config")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "enabled": true,
                "request_timeout_seconds": 0,
                "connect_timeout_seconds": 5,
                "pool_idle_timeout_seconds": 45,
                "proxy_enabled": false,
                "proxy_url": null,
                "danger_accept_invalid_certs": false,
                "user_agent": null
            }))
            .await;
        assert_eq!(invalid_timeout.status_code(), 400);

        let missing_proxy_url = request
            .put("/api/admin/http-client/config")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "enabled": true,
                "request_timeout_seconds": 30,
                "connect_timeout_seconds": 5,
                "pool_idle_timeout_seconds": 45,
                "proxy_enabled": true,
                "proxy_url": "",
                "danger_accept_invalid_certs": false,
                "user_agent": null
            }))
            .await;
        assert_eq!(missing_proxy_url.status_code(), 400);

        let invalid_test_url = request
            .post("/api/admin/http-client/test")
            .add_header(auth_key, auth_value)
            .json(&serde_json::json!({ "url": "ftp://example.test/file" }))
            .await;
        assert_eq!(invalid_test_url.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_ai_image_configs() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let list_response = request
            .get("/api/admin/ai-images/configs")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(list_response.status_code(), 200);
        assert!(list_response.text().contains("\"items\"") || list_response.text().contains("[]"));

        let invalid_create = request
            .post("/api/admin/ai-images/configs")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "key": "",
                "name": "",
                "base_url": "",
                "save_mode": "invalid"
            }))
            .await;
        assert_eq!(invalid_create.status_code(), 400);

        let create_response = request
            .post("/api/admin/ai-images/configs")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "key": "request-image",
                "name": "请求测试生图",
                "enabled": true,
                "base_url": "https://example.test/v1",
                "api_key": "secret-token",
                "model": "gpt-image-2",
                "size": "1024x1024",
                "quality": "high",
                "n": 1,
                "save_mode": "local",
                "local_output_dir": "storage/ai-images/request-tests",
                "storage_bucket_id": null,
                "storage_prefix": null,
                "description": "request test"
            }))
            .await;
        assert_eq!(create_response.status_code(), 200);
        let create_body = create_response.text();
        assert!(create_body.contains("request-image"));
        assert!(create_body.contains("\"api_key_configured\":true"));
        assert!(!create_body.contains("secret-token"));

        let setting = system_settings::Entity::find_by_id(20)
            .one(&ctx.db)
            .await
            .unwrap()
            .unwrap();
        assert!(setting.value.contains("request-image"));
        assert!(setting.value.contains("secret-token"));

        let update_response = request
            .put("/api/admin/ai-images/configs/request-image")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "key": "request-image",
                "name": "请求测试生图已更新",
                "enabled": true,
                "base_url": "https://example.test/v1",
                "api_key": null,
                "model": "gpt-image-2",
                "size": "1024x1536",
                "quality": "high",
                "n": 2,
                "save_mode": "local",
                "local_output_dir": "storage/ai-images/request-tests",
                "storage_bucket_id": null,
                "storage_prefix": null,
                "description": "updated"
            }))
            .await;
        assert_eq!(update_response.status_code(), 200);
        let update_body = update_response.text();
        assert!(update_body.contains("请求测试生图已更新"));
        assert!(!update_body.contains("secret-token"));

        let generations_response = request
            .get("/api/admin/ai-images/generations")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(generations_response.status_code(), 200);

        let delete_response = request
            .delete("/api/admin/ai-images/configs/request-image")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_response.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_configured_local_files() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let root = PathBuf::from("storage/file-manager/request-tests");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/hello.txt"), b"hello files").unwrap();
        fs::write(root.join("docs/duplicate.txt"), b"duplicate").unwrap();

        system_settings::ActiveModel {
            id: Set(18),
            value: Set(serde_json::json!([
                {
                    "key": "request-root",
                    "name": "请求测试文件",
                    "url_path": "/request-files",
                    "local_root": "storage/file-manager/request-tests",
                    "enabled": true
                }
            ])
            .to_string()),
            ..Default::default()
        }
        .update(&ctx.db)
        .await
        .unwrap();

        let roots = request
            .get("/api/admin/files/roots")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(roots.status_code(), 200);
        assert!(roots.text().contains("request-root"));

        let browser = request
            .get("/api/admin/files/browser?root_key=request-root&path=docs")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(browser.status_code(), 200);
        let browser_body = browser.text();
        assert!(browser_body.contains("hello.txt"));
        assert!(browser_body.contains("/request-files/docs/hello.txt"));

        let preview = request
            .get("/api/admin/files/preview?root_key=request-root&path=docs/hello.txt")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(preview.status_code(), 200);
        assert_eq!(preview.as_bytes().as_ref(), b"hello files");
        assert!(preview.content_type().contains("text/plain"));
        assert!(preview
            .header("content-disposition")
            .to_str()
            .unwrap()
            .contains("inline"));

        let download = request
            .get("/api/admin/files/download?root_key=request-root&path=docs/hello.txt")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(download.status_code(), 200);
        assert!(download
            .header("content-disposition")
            .to_str()
            .unwrap()
            .contains("attachment"));

        let create_folder = request
            .post("/api/admin/files/folders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "root_key": "request-root",
                "path": "docs/nested"
            }))
            .await;
        assert_eq!(create_folder.status_code(), 200);
        assert!(root.join("docs/nested").is_dir());

        let rename = request
            .put("/api/admin/files/rename")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "root_key": "request-root",
                "path": "docs/hello.txt",
                "name": "renamed.txt"
            }))
            .await;
        assert_eq!(rename.status_code(), 200);
        assert!(root.join("docs/renamed.txt").exists());
        assert!(!root.join("docs/hello.txt").exists());

        let duplicate_rename = request
            .put("/api/admin/files/rename")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "root_key": "request-root",
                "path": "docs/renamed.txt",
                "name": "duplicate.txt"
            }))
            .await;
        assert_eq!(duplicate_rename.status_code(), 400);

        let escaped = request
            .get("/api/admin/files/preview?root_key=request-root&path=../Cargo.toml")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(escaped.status_code(), 400);

        let delete_file = request
            .delete("/api/admin/files?root_key=request-root&path=docs/renamed.txt")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_file.status_code(), 200);
        assert!(!root.join("docs/renamed.txt").exists());

        let delete_folder = request
            .delete("/api/admin/files?root_key=request-root&path=docs/nested")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_folder.status_code(), 200);
        assert!(!root.join("docs/nested").exists());

        let file_logs = request
            .get("/api/admin/logs?module=files&keyword=renamed")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(file_logs.status_code(), 200);
        assert!(file_logs.text().contains("重命名文件"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_storage_profiles_and_browse_local_bucket() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let profiles = request
            .get("/api/admin/storage-profiles")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(profiles.status_code(), 200);
        assert!(profiles.text().contains("local-default"));

        let create_profile = request
            .post("/api/admin/storage-profiles")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "tenant_id": 1,
                "name": "请求测试本地存储",
                "code": "request-local",
                "provider": "local",
                "enabled": true,
                "is_default": false,
                "path_style": false,
                "description": "request test storage"
            }))
            .await;
        assert_eq!(create_profile.status_code(), 200);
        let profile_body: serde_json::Value = serde_json::from_str(&create_profile.text()).unwrap();
        let profile_id = profile_body["data"]["id"].as_i64().unwrap();

        let root = format!("storage/uploads/request-tests-{profile_id}");
        let root_path = PathBuf::from(&root);
        if root_path.exists() {
            fs::remove_dir_all(&root_path).unwrap();
        }
        let create_bucket = request
            .post(&format!("/api/admin/storage-profiles/{profile_id}/buckets"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "tenant_id": 1,
                "name": "请求测试桶",
                "bucket": "request-assets",
                "local_root": root,
                "enabled": true,
                "is_default": false
            }))
            .await;
        assert_eq!(create_bucket.status_code(), 200);
        let bucket_body: serde_json::Value = serde_json::from_str(&create_bucket.text()).unwrap();
        let bucket_id = bucket_body["data"]["id"].as_i64().unwrap();

        let test_profile = request
            .post(&format!("/api/admin/storage-profiles/{profile_id}/test"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(test_profile.status_code(), 200);

        let object_dir = root_path.join("docs");
        fs::create_dir_all(&object_dir).unwrap();
        fs::write(object_dir.join("hello.txt"), b"hello storage").unwrap();
        let file = upload_files::ActiveModel {
            storage: Set("local".to_string()),
            storage_profile_id: Set(Some(i32::try_from(profile_id).unwrap())),
            storage_bucket_id: Set(Some(i32::try_from(bucket_id).unwrap())),
            bucket: Set(Some("request-assets".to_string())),
            prefix: Set(Some("docs/".to_string())),
            object_key: Set("docs/hello.txt".to_string()),
            url: Set(String::new()),
            original_name: Set("hello.txt".to_string()),
            filename: Set("hello.txt".to_string()),
            extension: Set(Some("txt".to_string())),
            mime_type: Set(Some("text/plain".to_string())),
            size_bytes: Set(13),
            sha256: Set("request-sha".to_string()),
            category: Set(Some("docs".to_string())),
            tags: Set(Some("storage".to_string())),
            visibility: Set("private".to_string()),
            status: Set("active".to_string()),
            uploader_id: Set(Some(1)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await
        .unwrap();

        let browser = request
            .get(&format!(
                "/api/admin/uploads/browser?storage_bucket_id={bucket_id}&prefix=docs"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(browser.status_code(), 200);
        assert!(browser.text().contains("hello.txt"));

        let uploads = request
            .get(&format!(
                "/api/admin/uploads?storage_bucket_id={bucket_id}&prefix=docs"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(uploads.status_code(), 200);
        assert!(uploads.text().contains("request-assets"));

        let download = request
            .get(&format!("/api/admin/uploads/{}/download", file.id))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(download.status_code(), 200);
        assert_eq!(download.text(), "hello storage");
        assert!(download
            .header("content-disposition")
            .to_str()
            .unwrap()
            .contains("attachment"));

        let preview = request
            .get(&format!("/api/admin/uploads/{}/preview", file.id))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(preview.status_code(), 200);
        assert_eq!(preview.as_bytes().as_ref(), b"hello storage");
        assert_eq!(preview.content_type(), "text/plain");
        assert!(preview
            .header("content-disposition")
            .to_str()
            .unwrap()
            .contains("inline"));

        let create_folder = request
            .post("/api/admin/uploads/folders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "storage_bucket_id": bucket_id,
                "prefix": "docs/nested"
            }))
            .await;
        assert_eq!(create_folder.status_code(), 200);
        assert!(create_folder.text().contains("docs/nested/"));

        let nested_browser = request
            .get(&format!(
                "/api/admin/uploads/browser?storage_bucket_id={bucket_id}&prefix=docs"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(nested_browser.status_code(), 200);
        assert!(nested_browser.text().contains("docs/nested/"));

        fs::write(object_dir.join("external-a.txt"), b"external a").unwrap();
        fs::write(object_dir.join("external-b.txt"), b"external b").unwrap();
        let import_all = request
            .post("/api/admin/uploads/import-objects")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "storage_bucket_id": bucket_id,
                "prefix": "docs",
                "visibility": "private"
            }))
            .await;
        assert_eq!(import_all.status_code(), 200);
        let import_all_body: serde_json::Value = serde_json::from_str(&import_all.text()).unwrap();
        assert_eq!(import_all_body["data"]["imported"], 2);
        assert!(import_all.text().contains("external-a.txt"));

        let rename = request
            .put(&format!("/api/admin/uploads/{}/rename", file.id))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "original_name": "renamed.txt" }))
            .await;
        assert_eq!(rename.status_code(), 200);
        assert!(rename.text().contains("renamed.txt"));
        assert!(object_dir.join("renamed.txt").exists());
        assert!(!object_dir.join("hello.txt").exists());

        fs::write(object_dir.join("duplicate.txt"), b"duplicate").unwrap();
        let duplicate_rename = request
            .put(&format!("/api/admin/uploads/{}/rename", file.id))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "original_name": "duplicate.txt" }))
            .await;
        assert_eq!(duplicate_rename.status_code(), 400);

        let create_task = request
            .post("/api/admin/uploads/tasks")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "storage_bucket_id": bucket_id,
                "original_name": "chunked.txt",
                "mime_type": "text/plain",
                "size_bytes": 12,
                "chunk_size": 6,
                "total_chunks": 2,
                "prefix": "docs",
                "visibility": "private"
            }))
            .await;
        assert_eq!(create_task.status_code(), 200);
        assert!(create_task.text().contains("chunked.txt"));

        let task_body: serde_json::Value = serde_json::from_str(&create_task.text()).unwrap();
        let task_id = task_body["data"]["id"].as_i64().unwrap();
        let complete_empty_task = request
            .post(&format!("/api/admin/uploads/tasks/{task_id}/complete"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(complete_empty_task.status_code(), 400);

        let tasks = request
            .get("/api/admin/uploads/tasks")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(tasks.status_code(), 200);
        assert!(tasks.text().contains("chunked.txt"));

        let upload_logs = request
            .get("/api/admin/logs?module=uploads&keyword=hello.txt")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(upload_logs.status_code(), 200);
        assert!(upload_logs.text().contains("预览素材"));

        let delete_bucket = request
            .delete(&format!("/api/admin/storage-buckets/{bucket_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_bucket.status_code(), 400);

        let delete_profile = request
            .delete(&format!("/api/admin/storage-profiles/{profile_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_profile.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_tenants_and_role_data_scopes() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let tenants_response = request
            .get("/api/admin/tenants")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(tenants_response.status_code(), 200);
        assert!(tenants_response.text().contains("平台租户"));

        let data_scopes_response = request
            .get("/api/admin/data-scopes")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(data_scopes_response.status_code(), 200);
        assert!(data_scopes_response.text().contains("tenant"));

        let save_data_scopes_response = request
            .put("/api/admin/roles/2/data-scopes")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "data_scope_ids": [2, 3] }))
            .await;
        assert_eq!(save_data_scopes_response.status_code(), 200);

        let role_data_scopes_response = request
            .get("/api/admin/roles/2/data-scopes")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(role_data_scopes_response.status_code(), 200);
        assert!(role_data_scopes_response.text().contains("[2,3]"));

        let create_tenant_response = request
            .post("/api/admin/tenants")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "测试租户",
                "code": "test_tenant",
                "description": "请求测试创建",
                "enabled": true,
                "departments_enabled": true
            }))
            .await;
        assert_eq!(create_tenant_response.status_code(), 200);
        let created_tenant: serde_json::Value =
            serde_json::from_str(&create_tenant_response.text()).unwrap();
        let tenant_id = created_tenant["data"]["id"].as_i64().unwrap();

        let delete_tenant_response = request
            .delete(&format!("/api/admin/tenants/{tenant_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_tenant_response.status_code(), 200);

        let delete_system_tenant_response = request
            .delete("/api/admin/tenants/1")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_system_tenant_response.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_departments_and_current_department() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let list_response = request
            .get("/api/admin/departments")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(list_response.status_code(), 200);
        assert!(list_response.text().contains("平台总部"));

        let invalid_response = request
            .post("/api/admin/departments")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "tenant_id": 1,
                "name": "",
                "code": "",
                "enabled": true
            }))
            .await;
        assert_eq!(invalid_response.status_code(), 400);

        let create_response = request
            .post("/api/admin/departments")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "tenant_id": 1,
                "parent_id": 1,
                "name": "请求测试部门",
                "code": "request_test_department",
                "description": "请求测试部门管理",
                "sort_order": 30,
                "enabled": true
            }))
            .await;
        assert_eq!(create_response.status_code(), 200);
        let created: serde_json::Value = serde_json::from_str(&create_response.text()).unwrap();
        let department_id = created["data"]["id"].as_i64().unwrap();

        let get_response = request
            .get(&format!("/api/admin/departments/{department_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(get_response.status_code(), 200);
        assert!(get_response.text().contains("request_test_department"));

        let update_response = request
            .put(&format!("/api/admin/departments/{department_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "tenant_id": 1,
                "parent_id": 1,
                "name": "请求测试部门已更新",
                "code": "request_test_department",
                "description": "请求测试部门管理已更新",
                "sort_order": 31,
                "enabled": true
            }))
            .await;
        assert_eq!(update_response.status_code(), 200);
        assert!(update_response.text().contains("请求测试部门已更新"));

        let assigned_departments = request
            .get("/api/admin/users/3/departments")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(assigned_departments.status_code(), 200);
        assert!(assigned_departments.text().contains("platform_hq"));

        let assign_response = request
            .put("/api/admin/users/3/departments")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "department_ids": [1, department_id],
                "current_department_id": department_id
            }))
            .await;
        assert_eq!(assign_response.status_code(), 200);

        let cross_tenant_assign = request
            .put("/api/admin/users/3/departments")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "department_ids": [3],
                "current_department_id": 3
            }))
            .await;
        assert_eq!(cross_tenant_assign.status_code(), 400);

        let switch_response = request
            .post("/api/auth/current-department")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "department_id": department_id }))
            .await;
        assert_eq!(switch_response.status_code(), 200);

        let current_response = request
            .get("/api/auth/current")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(current_response.status_code(), 200);
        let current_body = current_response.text();
        assert!(current_body.contains("请求测试部门已更新"));
        assert!(current_body.contains("\"current_department\":{"));

        let forbidden_switch = request
            .post("/api/auth/current-department")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "department_id": 3 }))
            .await;
        assert_eq!(forbidden_switch.status_code(), 403);

        let clear_switch = request
            .post("/api/auth/current-department")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "department_id": null }))
            .await;
        assert_eq!(clear_switch.status_code(), 200);

        let reset_assignments = request
            .put("/api/admin/users/3/departments")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "department_ids": [1],
                "current_department_id": 1
            }))
            .await;
        assert_eq!(reset_assignments.status_code(), 200);

        let delete_system_response = request
            .delete("/api/admin/departments/1")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_system_response.status_code(), 400);

        let delete_response = request
            .delete(&format!("/api/admin/departments/{department_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_response.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn admin_extension_business_rules_are_enforced() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let builtin_setting = request
            .delete("/api/admin/settings/1")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(builtin_setting.status_code(), 400);

        let builtin_dict_type = request
            .delete("/api/admin/dict-types/1")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(builtin_dict_type.status_code(), 400);

        let default_dict_item = request
            .delete("/api/admin/dict-items/1")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(default_dict_item.status_code(), 400);

        for (key, value_type, value) in [
            ("test.invalid_json", "json", "{"),
            ("test.invalid_number", "number", "not-a-number"),
            ("test.invalid_boolean", "boolean", "yes"),
        ] {
            let response = request
                .post("/api/admin/settings")
                .add_header(auth_key.clone(), auth_value.clone())
                .json(&serde_json::json!({
                    "key": key,
                    "name": key,
                    "group_key": "test",
                    "value": value,
                    "value_type": value_type
                }))
                .await;
            assert_eq!(response.status_code(), 400, "{key}");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_email_templates() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let list_response = request
            .get("/api/admin/email-templates")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(list_response.status_code(), 200);
        assert!(list_response.text().contains("auth_welcome"));

        let invalid_variables = request
            .post("/api/admin/email-templates")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "code": "invalid_variables",
                "name": "无效变量模板",
                "template_type": "auth",
                "subject": "Hi {{name}}",
                "html_body": "<p>{{name}}</p>",
                "text_body": "{{name}}",
                "variables": "{",
                "enabled": true,
                "is_builtin": false
            }))
            .await;
        assert_eq!(invalid_variables.status_code(), 400);

        let create_response = request
            .post("/api/admin/email-templates")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "code": "request_test_template",
                "name": "请求测试模板",
                "template_type": "auth",
                "subject": "Hi {{ name }}",
                "html_body": "<p>Hello {{name}} from {{ domain }}</p>",
                "text_body": "Hello {{name}}",
                "variables": "[{\"name\":\"name\",\"description\":\"用户名称\"}]",
                "enabled": true,
                "is_builtin": false,
                "description": "请求测试"
            }))
            .await;
        assert_eq!(create_response.status_code(), 200);
        let created: serde_json::Value = serde_json::from_str(&create_response.text()).unwrap();
        let template_id = created["data"]["id"].as_i64().unwrap();

        let update_response = request
            .put(&format!("/api/admin/email-templates/{template_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "code": "request_test_template",
                "name": "请求测试模板已更新",
                "template_type": "auth",
                "subject": "Hi {{ name }}",
                "html_body": "<p>Hello {{name}} from {{ domain }}</p>",
                "text_body": "Hello {{name}} from {{domain}}",
                "variables": "[{\"name\":\"name\",\"description\":\"用户名称\"}]",
                "enabled": true,
                "is_builtin": false,
                "description": "请求测试"
            }))
            .await;
        assert_eq!(update_response.status_code(), 200);

        let preview_response = request
            .post(&format!("/api/admin/email-templates/{template_id}/preview"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "locals": {
                    "name": "Alice",
                    "domain": "https://example.test"
                }
            }))
            .await;
        assert_eq!(preview_response.status_code(), 200);
        let preview_body = preview_response.text();
        assert!(preview_body.contains("Hi Alice"));
        assert!(preview_body.contains("https://example.test"));

        let test_send_response = request
            .post(&format!(
                "/api/admin/email-templates/{template_id}/test-send"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "to": "admin@example.com",
                "locals": { "name": "Alice", "domain": "https://example.test" }
            }))
            .await;
        assert_eq!(test_send_response.status_code(), 200);

        let delete_builtin_response = request
            .delete("/api/admin/email-templates/1")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_builtin_response.status_code(), 400);

        let delete_response = request
            .delete(&format!("/api/admin/email-templates/{template_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_response.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_content() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let invalid_category_slug = request
            .post("/api/admin/content-categories")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "无效栏目",
                "slug": "Invalid Slug",
                "enabled": true
            }))
            .await;
        assert_eq!(invalid_category_slug.status_code(), 400);

        let create_category = request
            .post("/api/admin/content-categories")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试栏目",
                "slug": "request-test-category",
                "description": "请求测试内容栏目",
                "sort_order": 1,
                "enabled": true
            }))
            .await;
        assert_eq!(create_category.status_code(), 200);
        let category_body: serde_json::Value = serde_json::from_str(&create_category.text()).unwrap();
        let category_id = category_body["data"]["id"].as_i64().unwrap();

        let invalid_article_category = request
            .post("/api/admin/content-articles")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "category_id": 999_999,
                "title": "无效栏目文章",
                "slug": "invalid-category-article",
                "content": "正文",
                "status": "draft"
            }))
            .await;
        assert_eq!(invalid_article_category.status_code(), 400);

        let invalid_article_status = request
            .post("/api/admin/content-articles")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "category_id": category_id,
                "title": "无效状态文章",
                "slug": "invalid-status-article",
                "content": "正文",
                "status": "reviewing"
            }))
            .await;
        assert_eq!(invalid_article_status.status_code(), 400);

        let create_article = request
            .post("/api/admin/content-articles")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "category_id": category_id,
                "title": "请求测试文章",
                "slug": "request-test-article",
                "summary": "内容模块请求测试",
                "content": "这是一篇请求测试文章正文",
                "cover_image_url": "https://example.test/cover.png",
                "status": "draft",
                "is_featured": true,
                "seo_title": "请求测试文章 SEO",
                "seo_description": "请求测试文章 SEO 描述"
            }))
            .await;
        assert_eq!(create_article.status_code(), 200);
        let article_body: serde_json::Value = serde_json::from_str(&create_article.text()).unwrap();
        let article_id = article_body["data"]["id"].as_i64().unwrap();
        assert_eq!(article_body["data"]["status"], "draft");

        let list_articles = request
            .get(&format!(
                "/api/admin/content-articles?keyword=请求测试&category_id={category_id}&status=draft&is_featured=true"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(list_articles.status_code(), 200);
        assert!(list_articles.text().contains("request-test-article"));

        let update_article = request
            .put(&format!("/api/admin/content-articles/{article_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "category_id": category_id,
                "title": "请求测试文章已更新",
                "slug": "request-test-article",
                "summary": "内容模块请求测试已更新",
                "content": "更新后的请求测试文章正文",
                "status": "draft",
                "is_featured": false
            }))
            .await;
        assert_eq!(update_article.status_code(), 200);
        assert!(update_article.text().contains("请求测试文章已更新"));

        let publish_article = request
            .post(&format!("/api/admin/content-articles/{article_id}/publish"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(publish_article.status_code(), 200);
        let published_body: serde_json::Value = serde_json::from_str(&publish_article.text()).unwrap();
        assert_eq!(published_body["data"]["status"], "published");
        assert!(published_body["data"]["published_at"].is_string());

        let archive_article = request
            .post(&format!("/api/admin/content-articles/{article_id}/archive"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(archive_article.status_code(), 200);
        assert!(archive_article.text().contains("archived"));

        let delete_category_with_article = request
            .delete(&format!("/api/admin/content-categories/{category_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_category_with_article.status_code(), 400);

        let delete_article = request
            .delete(&format!("/api/admin/content-articles/{article_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_article.status_code(), 200);

        let delete_category = request
            .delete(&format!("/api/admin/content-categories/{category_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_category.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_access_operations_infrastructure() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        for path in [
            "/api/admin/notifications",
            "/api/admin/scheduled-tasks",
            "/api/admin/scheduled-task-runs",
            "/api/admin/backups",
            "/api/admin/rate-limits",
            "/api/admin/rate-limit-events",
            "/api/admin/monitoring/overview",
            "/api/admin/monitoring/server",
            "/api/admin/monitoring/processes",
            "/api/admin/commands",
            "/api/admin/command-workflows",
            "/api/admin/command-workflow-runs",
            "/api/admin/command-runs",
            "/api/admin/work-orders",
            "/api/admin/payment-channels",
            "/api/admin/payment-orders",
            "/api/admin/payment-callbacks",
            "/api/admin/payment-refunds",
            "/api/admin/content-categories",
            "/api/admin/content-articles",
        ] {
            let response = request
                .get(path)
                .add_header(auth_key.clone(), auth_value.clone())
                .await;
            assert_eq!(response.status_code(), 200, "{path}");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_view_server_monitoring() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let server = request
            .get("/api/admin/monitoring/server")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(server.status_code(), 200);
        let server_body: serde_json::Value = serde_json::from_str(&server.text()).unwrap();
        assert!(server_body["data"]["host"].is_object());
        assert!(server_body["data"]["cpu"].is_object());
        assert!(server_body["data"]["memory"].is_object());
        assert!(server_body["data"]["disks"].is_array());
        assert!(server_body["data"]["networks"].is_array());
        assert!(server_body["data"]["host"]["process_count"].is_number());

        let processes = request
            .get("/api/admin/monitoring/processes?page=1&page_size=10&sort=memory")
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(processes.status_code(), 200);
        let processes_body: serde_json::Value = serde_json::from_str(&processes.text()).unwrap();
        assert!(processes_body["data"]["items"].is_array());
        assert_eq!(processes_body["data"]["page"], 1);
        assert_eq!(processes_body["data"]["page_size"], 10);
        assert!(processes_body["data"]["total"].is_number());
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_command_runs() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);
        let cwd = std::env::current_dir().unwrap();
        let preview_dir = cwd.join("storage/command-preview-tests");
        fs::create_dir_all(&preview_dir).unwrap();
        let cwd = cwd.to_string_lossy().to_string();

        let invalid_template = request
            .post("/api/admin/commands")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "无效命令模板",
                "code": "invalid_command_template",
                "working_directory": cwd,
                "command": "printf invalid",
                "env_vars": "[1]",
                "enabled": true
            }))
            .await;
        assert_eq!(invalid_template.status_code(), 400);

        let create_template = request
            .post("/api/admin/commands")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试命令",
                "code": "request_test_command",
                "description": "请求测试命令模板",
                "working_directory": cwd,
                "command": "printf template-output",
                "default_args": "",
                "env_vars": "{\"REQUEST_TEST_ENV\":\"ok\"}",
                "setup_script": "",
                "timeout_seconds": 5,
                "enabled": true
            }))
            .await;
        assert_eq!(create_template.status_code(), 200);
        let template_body: serde_json::Value =
            serde_json::from_str(&create_template.text()).unwrap();
        let template_id = template_body["data"]["id"].as_i64().unwrap();

        let update_template = request
            .put(&format!("/api/admin/commands/{template_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试命令已更新",
                "code": "request_test_command",
                "description": "请求测试命令模板已更新",
                "working_directory": cwd,
                "command": "sh -c 'printf template-updated; printf preview-output > storage/command-preview-tests/template-{{random:6}}.txt'",
                "env_vars": "{\"REQUEST_TEST_ENV\":\"updated\"}",
                "timeout_seconds": 5,
                "preview_path_template": "storage/command-preview-tests/template-{{random:6}}.txt",
                "enabled": true
            }))
            .await;
        assert_eq!(update_template.status_code(), 200);
        let update_template_body: serde_json::Value =
            serde_json::from_str(&update_template.text()).unwrap();
        assert_eq!(update_template_body["data"]["name"], "请求测试命令已更新");
        assert_eq!(
            update_template_body["data"]["preview_path_template"],
            "storage/command-preview-tests/template-{{random:6}}.txt"
        );

        let run_template = request
            .post(&format!("/api/admin/commands/{template_id}/run"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({}))
            .await;
        assert_eq!(run_template.status_code(), 200);
        let run_body: serde_json::Value = serde_json::from_str(&run_template.text()).unwrap();
        let run_id = run_body["data"]["id"].as_i64().unwrap();
        let finished = wait_for_command_run(&request, &auth_key, &auth_value, run_id).await;
        assert_eq!(finished["data"]["status"], "success");
        assert_eq!(finished["data"]["exit_code"], 0);
        assert!(finished["data"]["effective_script"]
            .as_str()
            .unwrap_or_default()
            .contains("/opt/homebrew/bin"));
        assert!(finished["data"]["output_tail"]
            .as_str()
            .unwrap_or_default()
            .contains("template-updated"));
        assert_eq!(
            finished["data"]["preview_path_template"],
            "storage/command-preview-tests/template-{{random:6}}.txt"
        );
        let preview_path = finished["data"]["preview_path"].as_str().unwrap_or_default();
        assert!(preview_path.contains("storage/command-preview-tests/template-"));
        assert!(preview_path.ends_with(".txt"));
        assert!(!preview_path.contains("{{random"));
        let preview = request
            .get(&format!("/api/admin/command-runs/{run_id}/preview"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(preview.status_code(), 200);
        assert_eq!(preview.text(), "preview-output");

        let logs = request
            .get(&format!(
                "/api/admin/command-runs/{run_id}/logs?after_seq=0"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(logs.status_code(), 200);
        assert!(logs.text().contains("template-updated"));

        let invalid_workflow = request
            .post("/api/admin/command-workflows")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "无效请求测试编排",
                "code": "invalid_request_test_workflow",
                "steps": []
            }))
            .await;
        assert_eq!(invalid_workflow.status_code(), 400);

        let create_workflow = request
            .post("/api/admin/command-workflows")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试编排",
                "code": "request_test_workflow",
                "description": "请求测试编排描述",
                "enabled": true,
                "steps": [
                    {
                        "template_id": template_id,
                        "name": "第一步",
                        "sort_order": 1,
                        "args": "step-one-{{random:6}}",
                        "enabled": true
                    },
                    {
                        "template_id": template_id,
                        "name": "第二步",
                        "sort_order": 2,
                        "args": "step-two",
                        "working_directory": cwd,
                        "timeout_seconds": 5,
                        "enabled": true
                    }
                ]
            }))
            .await;
        assert_eq!(create_workflow.status_code(), 200);
        let workflow_body: serde_json::Value =
            serde_json::from_str(&create_workflow.text()).unwrap();
        let workflow_id = workflow_body["data"]["id"].as_i64().unwrap();
        assert_eq!(workflow_body["data"]["steps"].as_array().unwrap().len(), 2);

        let update_workflow = request
            .put(&format!("/api/admin/command-workflows/{workflow_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试编排已更新",
                "code": "request_test_workflow",
                "description": "请求测试编排描述已更新",
                "enabled": true,
                "steps": [
                    {
                        "template_id": template_id,
                        "name": "第一步",
                        "sort_order": 1,
                        "args": "step-one-{{random:6}}",
                        "enabled": true
                    },
                    {
                        "template_id": template_id,
                        "name": "第二步",
                        "sort_order": 2,
                        "args": "step-two",
                        "enabled": true
                    }
                ]
            }))
            .await;
        assert_eq!(update_workflow.status_code(), 200);
        assert!(update_workflow.text().contains("请求测试编排已更新"));

        let run_workflow = request
            .post(&format!("/api/admin/command-workflows/{workflow_id}/run"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "name": "请求测试编排运行" }))
            .await;
        assert_eq!(run_workflow.status_code(), 200);
        let workflow_run_body: serde_json::Value =
            serde_json::from_str(&run_workflow.text()).unwrap();
        let workflow_run_id = workflow_run_body["data"]["id"].as_i64().unwrap();
        let workflow_finished =
            wait_for_command_workflow_run(&request, &auth_key, &auth_value, workflow_run_id).await;
        assert_eq!(workflow_finished["data"]["status"], "success");
        let workflow_steps = workflow_finished["data"]["steps"].as_array().unwrap();
        assert_eq!(workflow_steps.len(), 2);
        assert!(workflow_steps
            .iter()
            .all(|step| step["status"] == "success"));
        let first_args = workflow_steps[0]["resolved_args"]
            .as_str()
            .unwrap_or_default();
        assert!(first_args.starts_with("step-one-{{random:6}}"));
        let first_command_run_id = workflow_steps[0]["command_run_id"].as_i64().unwrap();
        let first_command_run = request
            .get(&format!("/api/admin/command-runs/{first_command_run_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(first_command_run.status_code(), 200);
        let first_command_run_body: serde_json::Value =
            serde_json::from_str(&first_command_run.text()).unwrap();
        assert!(first_command_run_body["data"]["command_line"]
            .as_str()
            .unwrap_or_default()
            .contains("step-one-{{random:6}}"));
        assert!(first_command_run_body["data"]["effective_script"]
            .as_str()
            .unwrap_or_default()
            .contains("step-one-"));
        assert!(!first_command_run_body["data"]["effective_script"]
            .as_str()
            .unwrap_or_default()
            .contains("{{random"));

        let workflow_runs = request
            .get(&format!(
                "/api/admin/command-workflow-runs?workflow_id={workflow_id}&keyword=请求测试编排"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(workflow_runs.status_code(), 200);
        assert!(workflow_runs.text().contains("请求测试编排运行"));

        let workflow_list = request
            .get("/api/admin/command-workflows?keyword=请求测试编排")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(workflow_list.status_code(), 200);
        assert!(workflow_list.text().contains("请求测试编排已更新"));

        let delete_workflow = request
            .delete(&format!("/api/admin/command-workflows/{workflow_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_workflow.status_code(), 200);

        let ticket = request
            .post(&format!("/api/admin/command-runs/{run_id}/log-ticket"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(ticket.status_code(), 200);
        let ticket_body: serde_json::Value = serde_json::from_str(&ticket.text()).unwrap();
        assert!(ticket_body["data"]["ticket"].as_str().is_some());

        let rerun = request
            .post("/api/admin/command-runs")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "编辑重跑请求测试",
                "working_directory": cwd,
                "command_line": "sh -c 'printf rerun-output; printf rerun-preview > storage/command-preview-tests/rerun-{{random:6}}.txt'",
                "preview_path_template": "storage/command-preview-tests/rerun-{{random:6}}.txt",
                "timeout_seconds": 5
            }))
            .await;
        assert_eq!(rerun.status_code(), 200);
        let rerun_body: serde_json::Value = serde_json::from_str(&rerun.text()).unwrap();
        let rerun_id = rerun_body["data"]["id"].as_i64().unwrap();
        let rerun_finished = wait_for_command_run(&request, &auth_key, &auth_value, rerun_id).await;
        assert_eq!(rerun_finished["data"]["status"], "success");
        assert!(rerun_finished["data"]["command_line"]
            .as_str()
            .unwrap_or_default()
            .contains("rerun-output"));
        assert!(rerun_finished["data"]["command_line"]
            .as_str()
            .unwrap_or_default()
            .contains("{{random:6}}"));
        assert_eq!(
            rerun_finished["data"]["preview_path_template"],
            "storage/command-preview-tests/rerun-{{random:6}}.txt"
        );
        let rerun_preview_path = rerun_finished["data"]["preview_path"]
            .as_str()
            .unwrap_or_default();
        assert!(rerun_preview_path.contains("storage/command-preview-tests/rerun-"));
        assert!(!rerun_preview_path.contains("{{random"));
        assert!(!rerun_finished["data"]["effective_script"]
            .as_str()
            .unwrap_or_default()
            .contains("{{random"));
        let rerun_preview = request
            .get(&format!("/api/admin/command-runs/{rerun_id}/preview"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(rerun_preview.status_code(), 200);
        assert_eq!(rerun_preview.text(), "rerun-preview");

        let long_run = request
            .post("/api/admin/command-runs")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "取消请求测试",
                "working_directory": cwd,
                "command_line": "while true; do :; done",
                "timeout_seconds": 10
            }))
            .await;
        assert_eq!(long_run.status_code(), 200);
        let long_run_body: serde_json::Value = serde_json::from_str(&long_run.text()).unwrap();
        let long_run_id = long_run_body["data"]["id"].as_i64().unwrap();
        let active_runs = request
            .get("/api/admin/command-runs")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(active_runs.status_code(), 200);
        let active_body = active_runs.text();
        assert!(active_body.contains("取消请求测试"), "{active_body}");
        assert!(!active_body.contains("backend restarted"), "{active_body}");
        sleep(Duration::from_millis(100)).await;
        let cancel = request
            .post(&format!("/api/admin/command-runs/{long_run_id}/cancel"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(cancel.status_code(), 200);
        let cancel_body = cancel.text();
        assert!(cancel_body.contains("cancelled"), "{cancel_body}");
        let cancelled = wait_for_command_run(&request, &auth_key, &auth_value, long_run_id).await;
        assert_eq!(cancelled["data"]["status"], "cancelled");

        let list_runs = request
            .get("/api/admin/command-runs?keyword=请求测试")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(list_runs.status_code(), 200);
        assert!(list_runs.text().contains("请求测试"));

        let delete_template = request
            .delete(&format!("/api/admin/commands/{template_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_template.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_create_ssh_terminal_ticket() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let targets = request
            .get("/api/admin/ssh/targets")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(targets.status_code(), 200);
        assert!(targets.text().contains("local-shell"));

        let ticket = request
            .post("/api/admin/ssh/tickets")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "target_key": "local-shell",
                "cols": 100,
                "rows": 30
            }))
            .await;
        assert_eq!(ticket.status_code(), 200);
        let ticket_body: serde_json::Value = serde_json::from_str(&ticket.text()).unwrap();
        assert!(ticket_body["data"]["ticket"].as_str().is_some());
        assert!(ticket_body["data"]["expires_at"].as_str().is_some());

        let invalid_ticket = request
            .post("/api/admin/ssh/tickets")
            .add_header(auth_key, auth_value)
            .json(&serde_json::json!({ "target_key": "missing-target" }))
            .await;
        assert_eq!(invalid_ticket.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_create_vnc_session_ticket() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let targets = request
            .get("/api/admin/vnc/targets")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(targets.status_code(), 200);
        assert!(targets.text().contains("local-vnc"));

        let session = request
            .post("/api/admin/vnc/sessions")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "target_key": "local-vnc" }))
            .await;
        assert_eq!(session.status_code(), 200);
        let session_body: serde_json::Value = serde_json::from_str(&session.text()).unwrap();
        let session_id = session_body["data"]["id"].as_str().unwrap().to_string();
        assert_eq!(session_body["data"]["target_key"], "local-vnc");

        let sessions = request
            .get("/api/admin/vnc/sessions")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(sessions.status_code(), 200);
        assert!(sessions.text().contains(&session_id));

        let ticket = request
            .post(&format!("/api/admin/vnc/sessions/{session_id}/tickets"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(ticket.status_code(), 200);
        let ticket_body: serde_json::Value = serde_json::from_str(&ticket.text()).unwrap();
        assert!(ticket_body["data"]["ticket"].as_str().is_some());
        assert!(ticket_body["data"]["expires_at"].as_str().is_some());

        let invalid_session = request
            .post("/api/admin/vnc/sessions")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "target_key": "missing-target" }))
            .await;
        assert_eq!(invalid_session.status_code(), 400);

        let close = request
            .delete(&format!("/api/admin/vnc/sessions/{session_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(close.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_work_orders() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let invalid_create = request
            .post("/api/admin/work-orders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "title": "",
                "description": ""
            }))
            .await;
        assert_eq!(invalid_create.status_code(), 400);

        let invalid_metadata = request
            .post("/api/admin/work-orders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "title": "无效扩展数据工单",
                "description": "测试无效 JSON",
                "metadata": "{"
            }))
            .await;
        assert_eq!(invalid_metadata.status_code(), 400);

        let create_response = request
            .post("/api/admin/work-orders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "title": "请求测试工单",
                "description": "工单内容",
                "category": "technical",
                "priority": "normal",
                "assignee_id": 1,
                "metadata": "{\"source\":\"request-test\"}"
            }))
            .await;
        assert_eq!(create_response.status_code(), 200);
        let created: serde_json::Value = serde_json::from_str(&create_response.text()).unwrap();
        let work_order_id = created["data"]["id"].as_i64().unwrap();
        assert!(created["data"]["order_no"]
            .as_str()
            .unwrap()
            .starts_with("WO"));
        assert_eq!(created["data"]["status"], "assigned");
        assert_eq!(created["data"]["priority"], "normal");

        let list_response = request
            .get("/api/admin/work-orders?keyword=请求测试")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(list_response.status_code(), 200);
        assert!(list_response.text().contains("请求测试工单"));

        let update_response = request
            .put(&format!("/api/admin/work-orders/{work_order_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "title": "请求测试工单已更新",
                "description": "更新后的工单内容",
                "category": "account",
                "priority": "high",
                "assignee_id": 1,
                "metadata": "{\"source\":\"request-test\"}"
            }))
            .await;
        assert_eq!(update_response.status_code(), 200);
        assert!(update_response.text().contains("请求测试工单已更新"));

        let comment_response = request
            .post(&format!("/api/admin/work-orders/{work_order_id}/comments"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "body": "第一条处理备注" }))
            .await;
        assert_eq!(comment_response.status_code(), 200);

        let comments_response = request
            .get(&format!("/api/admin/work-orders/{work_order_id}/comments"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(comments_response.status_code(), 200);
        assert!(comments_response.text().contains("第一条处理备注"));

        let assign_response = request
            .post(&format!("/api/admin/work-orders/{work_order_id}/assign"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "assignee_id": 1,
                "note": "重新分配给管理员"
            }))
            .await;
        assert_eq!(assign_response.status_code(), 200);

        let transition_in_progress = request
            .post(&format!(
                "/api/admin/work-orders/{work_order_id}/transition"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "status": "in_progress",
                "comment": "开始处理"
            }))
            .await;
        assert_eq!(transition_in_progress.status_code(), 200);

        let transition_resolved = request
            .post(&format!(
                "/api/admin/work-orders/{work_order_id}/transition"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "status": "resolved" }))
            .await;
        assert_eq!(transition_resolved.status_code(), 200);

        let transition_closed = request
            .post(&format!(
                "/api/admin/work-orders/{work_order_id}/transition"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "status": "closed" }))
            .await;
        assert_eq!(transition_closed.status_code(), 200);

        let terminal_transition = request
            .post(&format!(
                "/api/admin/work-orders/{work_order_id}/transition"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "status": "in_progress" }))
            .await;
        assert_eq!(terminal_transition.status_code(), 400);

        let upload = upload_files::ActiveModel {
            storage: Set("local".to_string()),
            object_key: Set("request-tests/ticket.txt".to_string()),
            url: Set("/uploads/request-tests/ticket.txt".to_string()),
            original_name: Set("ticket.txt".to_string()),
            filename: Set("ticket.txt".to_string()),
            extension: Set(Some("txt".to_string())),
            mime_type: Set(Some("text/plain".to_string())),
            size_bytes: Set(12),
            sha256: Set("ticket-sha256".to_string()),
            category: Set(Some("ticket".to_string())),
            tags: Set(None),
            visibility: Set("private".to_string()),
            status: Set("active".to_string()),
            uploader_id: Set(Some(1)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await
        .unwrap();
        let upload_id = i64::from(upload.id);

        let attach_response = request
            .post(&format!(
                "/api/admin/work-orders/{work_order_id}/attachments"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "upload_file_id": upload_id,
                "description": "问题截图"
            }))
            .await;
        assert_eq!(attach_response.status_code(), 200);
        let attachment: serde_json::Value = serde_json::from_str(&attach_response.text()).unwrap();
        let attachment_id = attachment["data"]["id"].as_i64().unwrap();

        let detail_response = request
            .get(&format!("/api/admin/work-orders/{work_order_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(detail_response.status_code(), 200);
        let detail_body = detail_response.text();
        assert!(detail_body.contains("第一条处理备注"));
        assert!(detail_body.contains("ticket.txt"));

        let delete_attachment_response = request
            .delete(&format!(
                "/api/admin/work-orders/{work_order_id}/attachments/{attachment_id}"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_attachment_response.status_code(), 200);

        let delete_response = request
            .delete(&format!("/api/admin/work-orders/{work_order_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_response.status_code(), 200);

        let get_deleted = request
            .get(&format!("/api/admin/work-orders/{work_order_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(get_deleted.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_operations_records() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let create_notification = request
            .post("/api/admin/notifications")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "title": "请求测试通知",
                "content": "测试通知内容",
                "level": "info",
                "category": "system",
                "target_type": "all"
            }))
            .await;
        assert_eq!(create_notification.status_code(), 200);
        let notification_body: serde_json::Value =
            serde_json::from_str(&create_notification.text()).unwrap();
        let notification_id = notification_body["data"]["id"].as_i64().unwrap();

        let read_notification = request
            .put(&format!("/api/admin/notifications/{notification_id}/read"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(read_notification.status_code(), 200);

        let delete_notification = request
            .delete(&format!("/api/admin/notifications/{notification_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_notification.status_code(), 200);

        let create_task = request
            .post("/api/admin/scheduled-tasks")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试任务",
                "code": "request_test_cleanup",
                "task_type": "cleanup_logs",
                "cron_expr": "0 4 * * *",
                "payload": "{\"retention_days\":7}",
                "enabled": true,
                "status": "idle"
            }))
            .await;
        assert_eq!(create_task.status_code(), 200);
        let task_body: serde_json::Value = serde_json::from_str(&create_task.text()).unwrap();
        let task_id = task_body["data"]["id"].as_i64().unwrap();

        let update_task = request
            .put(&format!("/api/admin/scheduled-tasks/{task_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试任务已更新",
                "code": "request_test_cleanup",
                "task_type": "cleanup_logs",
                "cron_expr": "0 5 * * *",
                "payload": "{\"retention_days\":14}",
                "enabled": false,
                "status": "idle"
            }))
            .await;
        assert_eq!(update_task.status_code(), 200);

        let run_task = request
            .post(&format!("/api/admin/scheduled-tasks/{task_id}/run"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(run_task.status_code(), 200);
        assert!(run_task.text().contains("success"));

        let task_runs = request
            .get("/api/admin/scheduled-task-runs")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(task_runs.status_code(), 200);
        assert!(task_runs.text().contains("request_test_cleanup"));

        let invalid_rule = request
            .post("/api/admin/rate-limits")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "无效规则",
                "scope": "ip",
                "path_pattern": "/api/test",
                "method": "POST",
                "limit_count": 0,
                "window_seconds": 60,
                "enabled": false
            }))
            .await;
        assert_eq!(invalid_rule.status_code(), 400);

        let create_rule = request
            .post("/api/admin/rate-limits")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试限流",
                "scope": "ip",
                "path_pattern": "/api/request-test",
                "method": "GET",
                "limit_count": 3,
                "window_seconds": 60,
                "enabled": false,
                "description": "请求测试"
            }))
            .await;
        assert_eq!(create_rule.status_code(), 200);
        let rule_body: serde_json::Value = serde_json::from_str(&create_rule.text()).unwrap();
        let rule_id = rule_body["data"]["id"].as_i64().unwrap();

        let update_rule = request
            .put(&format!("/api/admin/rate-limits/{rule_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试限流已更新",
                "scope": "ip",
                "path_pattern": "/api/request-test",
                "method": "GET",
                "limit_count": 4,
                "window_seconds": 60,
                "enabled": false,
                "description": "请求测试"
            }))
            .await;
        assert_eq!(update_rule.status_code(), 200);

        let delete_rule = request
            .delete(&format!("/api/admin/rate-limits/{rule_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_rule.status_code(), 200);

        let create_backup = request
            .post("/api/admin/backups")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(create_backup.status_code(), 200);
        let backup_body: serde_json::Value = serde_json::from_str(&create_backup.text()).unwrap();
        let backup_id = backup_body["data"]["id"].as_i64().unwrap();
        assert_eq!(backup_body["data"]["status"], "success");
        assert!(backup_body["data"]["storage_path"]
            .as_str()
            .unwrap()
            .contains("storage/backups/"));
        assert!(backup_body["data"]["delivery_status"]
            .as_str()
            .unwrap()
            .contains("no delivery targets configured"));

        let unauth_restore = request
            .post(&format!("/api/admin/backups/{backup_id}/restore"))
            .json(&serde_json::json!({ "confirm_phrase": "RESTORE DATABASE" }))
            .await;
        assert_eq!(unauth_restore.status_code(), 401);

        let invalid_restore_confirmation = request
            .post(&format!("/api/admin/backups/{backup_id}/restore"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "confirm_phrase": "wrong" }))
            .await;
        assert_eq!(invalid_restore_confirmation.status_code(), 400);

        let failed_backup = database_backups::ActiveModel {
            filename: Set("failed-request-test.dump".to_string()),
            storage_path: Set("storage/backups/request-test/missing.dump".to_string()),
            size_bytes: Set(0),
            sha256: Set(None),
            status: Set("failed".to_string()),
            trigger_type: Set("manual".to_string()),
            started_at: Set(chrono::Local::now().into()),
            finished_at: Set(Some(chrono::Local::now().into())),
            duration_ms: Set(Some(1)),
            error_message: Set(Some("request test failed backup".to_string())),
            created_by: Set(Some(1)),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await
        .unwrap();

        let rejected_restore = request
            .post(&format!("/api/admin/backups/{}/restore", failed_backup.id))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({ "confirm_phrase": "RESTORE DATABASE" }))
            .await;
        assert_eq!(rejected_restore.status_code(), 400);
        assert!(rejected_restore.text().contains("only successful backups"));

        let restore_records = request
            .get(&format!("/api/admin/backups/{backup_id}/restores"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(restore_records.status_code(), 200);
        let restore_records_body: serde_json::Value =
            serde_json::from_str(&restore_records.text()).unwrap();
        assert_eq!(restore_records_body["data"].as_array().unwrap().len(), 0);

        let update_delivery_targets = request
            .put("/api/admin/settings/5")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "key": "backup.delivery_targets",
                "name": "备份推送目标",
                "group_key": "backup",
                "value": "[\"telegram\",\"wecom\",\"dingtalk\",\"custom\"]",
                "value_type": "json",
                "default_value": "[]",
                "description": "数据库备份完成后的通知目标列表",
                "is_public": false,
                "is_builtin": true,
                "is_encrypted": false,
                "sort_order": 50
            }))
            .await;
        assert_eq!(update_delivery_targets.status_code(), 200);

        let deliver_backup = request
            .post(&format!("/api/admin/backups/{backup_id}/deliver"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(deliver_backup.status_code(), 200);
        let delivered_backup: serde_json::Value =
            serde_json::from_str(&deliver_backup.text()).unwrap();
        let delivery_status = delivered_backup["data"]["delivery_status"]
            .as_str()
            .unwrap();
        assert!(delivery_status.contains("telegram"));
        assert!(delivery_status.contains("wecom"));
        assert!(delivery_status.contains("dingtalk"));
        assert!(delivery_status.contains("custom"));
        assert!(delivery_status.contains("missing"));

        let delete_backup = request
            .delete(&format!("/api/admin/backups/{backup_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(delete_backup.status_code(), 200);

        let delete_task = request
            .delete(&format!("/api/admin/scheduled-tasks/{task_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_task.status_code(), 200);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_manage_payments() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = admin_token(&request).await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let invalid_provider = request
            .post("/api/admin/payment-channels")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "无效通道",
                "provider": "unknown",
                "channel_code": "request_test_invalid",
                "currency": "CNY",
                "config": "{}"
            }))
            .await;
        assert_eq!(invalid_provider.status_code(), 400);

        let invalid_config = request
            .post("/api/admin/payment-channels")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "无效配置",
                "provider": "yipay",
                "channel_code": "request_test_invalid_json",
                "currency": "CNY",
                "config": "not-json"
            }))
            .await;
        assert_eq!(invalid_config.status_code(), 400);

        let create_channel = request
            .post("/api/admin/payment-channels")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试易支付",
                "provider": "yipay",
                "channel_code": "request_test_yipay",
                "currency": "CNY",
                "config": "{\"gateway\":\"https://pay.example.test\"}",
                "secret_config": "{\"key\":\"original-secret\"}",
                "enabled": true,
                "sort_order": 1,
                "description": "请求测试"
            }))
            .await;
        assert_eq!(create_channel.status_code(), 200);
        let channel_body: serde_json::Value = serde_json::from_str(&create_channel.text()).unwrap();
        let channel_id = channel_body["data"]["id"].as_i64().unwrap();
        assert_eq!(channel_body["data"]["secret_config"], "******");

        let update_channel = request
            .put(&format!("/api/admin/payment-channels/{channel_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "name": "请求测试易支付已更新",
                "provider": "yipay",
                "channel_code": "request_test_yipay",
                "currency": "CNY",
                "config": "{\"gateway\":\"https://pay2.example.test\"}",
                "secret_config": "******",
                "enabled": true,
                "sort_order": 2,
                "description": "请求测试更新"
            }))
            .await;
        assert_eq!(update_channel.status_code(), 200);
        let stored_channel = payment_channels::Entity::find_by_id(channel_id as i32)
            .one(&ctx.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored_channel.secret_config.as_deref(),
            Some("{\"key\":\"original-secret\"}")
        );

        let channels = request
            .get("/api/admin/payment-channels")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(channels.status_code(), 200);
        assert!(channels.text().contains("request_test_yipay"));

        let invalid_order = request
            .post("/api/admin/payment-orders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "subject": "",
                "amount": "0",
                "provider": "yipay"
            }))
            .await;
        assert_eq!(invalid_order.status_code(), 400);

        let create_order = request
            .post("/api/admin/payment-orders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "channel_id": channel_id,
                "merchant_order_no": "MERCHANT-REQUEST-001",
                "subject": "请求测试订单",
                "body": "请求测试支付订单",
                "amount": "12.30",
                "metadata": "{\"source\":\"request-test\"}"
            }))
            .await;
        assert_eq!(create_order.status_code(), 200);
        let order_body: serde_json::Value = serde_json::from_str(&create_order.text()).unwrap();
        let order_id = order_body["data"]["id"].as_i64().unwrap();
        assert_eq!(order_body["data"]["status"], "pending");
        assert_eq!(order_body["data"]["provider"], "yipay");
        assert!(order_body["data"]["order_no"]
            .as_str()
            .unwrap()
            .starts_with("PAY"));

        let orders = request
            .get("/api/admin/payment-orders?keyword=MERCHANT-REQUEST-001")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(orders.status_code(), 200);
        assert!(orders.text().contains("请求测试订单"));

        let mark_paid = request
            .post(&format!("/api/admin/payment-orders/{order_id}/mark-paid"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "trade_no": "TRADE-REQUEST-001",
                "payer_id": "payer-1",
                "payload": "{\"manual\":true}"
            }))
            .await;
        assert_eq!(mark_paid.status_code(), 200);
        assert!(mark_paid.text().contains("paid"));

        let cancel_paid = request
            .post(&format!("/api/admin/payment-orders/{order_id}/cancel"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(cancel_paid.status_code(), 400);

        let create_refund = request
            .post(&format!("/api/admin/payment-orders/{order_id}/refunds"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "amount": "5.00",
                "reason": "请求测试退款"
            }))
            .await;
        assert_eq!(create_refund.status_code(), 200);
        let refund_body: serde_json::Value = serde_json::from_str(&create_refund.text()).unwrap();
        let refund_id = refund_body["data"]["id"].as_i64().unwrap();
        assert_eq!(refund_body["data"]["status"], "pending");

        let approve_refund = request
            .post(&format!("/api/admin/payment-refunds/{refund_id}/approve"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(approve_refund.status_code(), 200);
        assert!(approve_refund.text().contains("approved"));

        let create_rejected_refund = request
            .post(&format!("/api/admin/payment-orders/{order_id}/refunds"))
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "amount": "1.00",
                "reason": "请求测试拒绝退款"
            }))
            .await;
        assert_eq!(create_rejected_refund.status_code(), 200);
        let rejected_refund_body: serde_json::Value =
            serde_json::from_str(&create_rejected_refund.text()).unwrap();
        let rejected_refund_id = rejected_refund_body["data"]["id"].as_i64().unwrap();

        let reject_refund = request
            .post(&format!(
                "/api/admin/payment-refunds/{rejected_refund_id}/reject"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(reject_refund.status_code(), 200);
        assert!(reject_refund.text().contains("rejected"));

        let succeed_refund = request
            .post(&format!(
                "/api/admin/payment-refunds/{refund_id}/mark-succeeded"
            ))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(succeed_refund.status_code(), 200);
        assert!(succeed_refund.text().contains("succeeded"));

        let callbacks = request
            .get("/api/admin/payment-callbacks")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(callbacks.status_code(), 200);
        let callbacks_body: serde_json::Value = serde_json::from_str(&callbacks.text()).unwrap();
        assert!(callbacks.text().contains("manual_record"));
        let callback_id = callbacks_body["data"]["items"][0]["id"].as_i64().unwrap();

        let callback_detail = request
            .get(&format!("/api/admin/payment-callbacks/{callback_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(callback_detail.status_code(), 200);
        assert!(callback_detail.text().contains("TRADE-REQUEST-001"));

        let refunds = request
            .get("/api/admin/payment-refunds")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(refunds.status_code(), 200);
        assert!(refunds.text().contains("请求测试退款"));

        let detail = request
            .get(&format!("/api/admin/payment-orders/{order_id}"))
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(detail.status_code(), 200);
        assert!(detail.text().contains("TRADE-REQUEST-001"));
        assert!(detail.text().contains("请求测试退款"));

        let delete_channel = request
            .delete(&format!("/api/admin/payment-channels/{channel_id}"))
            .add_header(auth_key, auth_value)
            .await;
        assert_eq!(delete_channel.status_code(), 400);
    })
    .await;
}
