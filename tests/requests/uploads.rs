use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{
    app::App, models::_entities::storage_buckets, responses::ApiResponse,
    views::auth::LoginResponse,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, EntityTrait, IntoActiveModel, Set};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);
const BOUNDARY: &str = "finder-upload-test-boundary";

async fn admin_token(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.5.{ip_suffix}"))
        .json(&serde_json::json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

/// Multipart upload to bucket 1; `prefix: None` omits the field entirely.
async fn upload(
    request: &TestServer,
    token: &str,
    name: &str,
    prefix: Option<&str>,
) -> serde_json::Value {
    let mut body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"storage_bucket_id\"\r\n\r\n1\r\n"
    );
    if let Some(prefix) = prefix {
        body.push_str(&format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"prefix\"\r\n\r\n{prefix}\r\n"
        ));
    }
    body.push_str(&format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: text/plain\r\n\r\nfinder test\r\n--{BOUNDARY}--\r\n"
    ));
    let (key, value) = prepare_data::auth_header(token);
    let response = request
        .post("/api/admin/uploads")
        .add_header(key, value)
        .content_type(&format!("multipart/form-data; boundary={BOUNDARY}"))
        .bytes(axum::body::Bytes::from(body))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).unwrap();
    body["data"].clone()
}

async fn listed_names(request: &TestServer, token: &str, query: &str) -> Vec<String> {
    let (key, value) = prepare_data::auth_header(token);
    let response = request
        .get(&format!("/api/admin/uploads?storage_bucket_id=1&{query}"))
        .add_header(key, value)
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    let body: serde_json::Value = serde_json::from_str(&response.text()).unwrap();
    let mut names: Vec<String> = body["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["original_name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[tokio::test]
#[serial]
async fn uploads_respect_the_folder_being_browsed() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        // Keep test files out of the shared storage directory.
        let root = std::env::temp_dir().join(format!("finder-uploads-{}", uuid::Uuid::new_v4()));
        let bucket = storage_buckets::Entity::find_by_id(1)
            .one(&ctx.db)
            .await
            .unwrap()
            .unwrap();
        let mut bucket = bucket.into_active_model();
        bucket.local_root = Set(Some(root.display().to_string()));
        bucket.update(&ctx.db).await.unwrap();
        let token = admin_token(&request).await;

        let at_root = upload(&request, &token, "root.txt", Some("")).await;
        assert!(
            !at_root["object_key"].as_str().unwrap().contains('/'),
            "an explicit empty prefix stores at the bucket root: {at_root}"
        );
        let in_docs = upload(&request, &token, "doc.txt", Some("docs")).await;
        assert!(in_docs["object_key"].as_str().unwrap().starts_with("docs/"));
        let dated = upload(&request, &token, "dated.txt", None).await;
        let dated_key = dated["object_key"].as_str().unwrap();
        assert!(
            dated_key.starts_with(&chrono::Utc::now().format("%Y/%m/").to_string()),
            "omitting the prefix keeps the dated default: {dated_key}"
        );

        assert_eq!(
            listed_names(&request, &token, "").await,
            vec!["dated.txt", "doc.txt", "root.txt"],
            "without exact_prefix the whole bucket is listed"
        );
        assert_eq!(
            listed_names(&request, &token, "exact_prefix=true").await,
            vec!["root.txt"]
        );
        assert_eq!(
            listed_names(&request, &token, "prefix=docs&exact_prefix=true").await,
            vec!["doc.txt"]
        );

        let _ = std::fs::remove_dir_all(root);
    })
    .await;
}
