use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use gpt_images::{
    app::App,
    models::_entities::{operation_logs, role_permissions, user_roles},
    responses::ApiResponse,
    services::audit,
    views::auth::LoginResponse,
};
use loco_rs::{app::Hooks, testing::prelude::*, TestServer};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data;

const RULES_PATH: &str = "/api/admin/audit-rules";
const DOCK_PATH: &str = "/api/user/preferences/dock";
const OPERATOR_ROLE_ID: i32 = 2;
const LOG_LIST_PERMISSION_ID: i32 = 21;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login_admin(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.9.{ip_suffix}"))
        .json(&json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

async fn send(
    request: &TestServer,
    token: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> (u16, Value) {
    let (key, value) = prepare_data::auth_header(token);
    let builder = match method {
        "GET" => request.get(path),
        "PUT" => request.put(path),
        "POST" => request.post(path),
        _ => unreachable!(),
    }
    .add_header(key, value)
    .add_header("x-forwarded-for", "198.51.100.7");
    let response = match body {
        Some(body) => builder.json(body).await,
        None => builder.await,
    };
    (
        response.status_code().as_u16(),
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

/// Audit rows are written in the background; wait for them to land.
async fn logs_matching(
    db: &DatabaseConnection,
    log_type: &str,
    path_prefix: &str,
    expected: usize,
) -> Vec<operation_logs::Model> {
    let mut rows = Vec::new();
    for _ in 0..40 {
        rows = operation_logs::Entity::find()
            .filter(operation_logs::Column::LogType.eq(log_type))
            .filter(operation_logs::Column::Path.starts_with(path_prefix))
            .order_by_asc(operation_logs::Column::Id)
            .all(db)
            .await
            .unwrap();
        if rows.len() >= expected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    rows
}

fn rule_body(enabled: bool, features: &[(&str, bool, bool, bool)]) -> Value {
    json!({
        "enabled": enabled,
        "features": features
            .iter()
            .map(|(key, enabled, log_reads, log_body)| json!({
                "key": key, "enabled": enabled, "log_reads": log_reads, "log_body": log_body
            }))
            .collect::<Vec<_>>(),
    })
}

#[tokio::test]
#[serial]
async fn every_api_route_belongs_to_an_audit_feature() {
    request::<App, _, _>(|_request, ctx| async move {
        let api_routes: Vec<String> = App::routes(&ctx)
            .collect()
            .into_iter()
            .map(|route| route.uri)
            .filter(|uri| uri.starts_with("/api/"))
            .collect();
        assert!(
            api_routes.len() > 150,
            "only {} routes listed",
            api_routes.len()
        );
        let unmapped: Vec<&String> = api_routes
            .iter()
            .filter(|uri| audit::feature_for_path(uri).key == audit::OTHER_FEATURE)
            .collect();
        assert!(
            unmapped.is_empty(),
            "add these routes to services::audit::FEATURES: {unmapped:?}"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn writes_are_recorded_with_masked_bodies_and_reads_are_not() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        audit::invalidate_rules();
        let token = login_admin(&request).await;

        let body = json!({ "value": { "position": "bottom", "apiToken": "secret-123" } });
        let (status, _) = send(&request, &token, "PUT", DOCK_PATH, Some(&body)).await;
        assert_eq!(status, 200);
        let (status, _) = send(&request, &token, "GET", DOCK_PATH, None).await;
        assert_eq!(status, 200);

        let rows = logs_matching(&ctx.db, "request", DOCK_PATH, 1).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let rows_after = logs_matching(&ctx.db, "request", DOCK_PATH, 1).await;
        assert_eq!(rows_after.len(), 1, "the GET is not recorded by default");
        let row = &rows[0];
        assert_eq!(row.module, "user_preferences");
        assert_eq!(row.method.as_deref(), Some("PUT"));
        assert_eq!(row.status, Some(200));
        assert_eq!(row.ip.as_deref(), Some("198.51.100.7"));
        assert!(row
            .operator
            .as_deref()
            .unwrap()
            .contains("admin@example.com"));
        assert!(row.user_id.is_some() && row.duration_ms.is_some());
        let summary = row.request_summary.as_deref().unwrap();
        assert!(
            summary.contains("bottom") && !summary.contains("secret-123"),
            "{summary}"
        );

        // The login handler writes its own log; the middleware does not add a second one.
        let login_requests = logs_matching(&ctx.db, "request", "/api/auth/login", 0).await;
        assert!(login_requests.is_empty());
        let logins = logs_matching(&ctx.db, "login", "/api/auth/login", 1).await;
        assert!(!logins.is_empty());
        assert!(
            logins[0].ip.is_some(),
            "handler logs now carry the client IP"
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn per_feature_switches_control_what_is_recorded() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        audit::invalidate_rules();
        let token = login_admin(&request).await;

        let (status, body) = send(&request, &token, "GET", RULES_PATH, None).await;
        assert_eq!(status, 200);
        let dock = body["data"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|feature| feature["key"] == "user_preferences")
            .unwrap()
            .clone();
        assert_eq!(dock["enabled"], true);
        assert_eq!(dock["log_reads"], false);

        // Reads on, request bodies off.
        let rules = rule_body(true, &[("user_preferences", true, true, false)]);
        let (status, _) = send(&request, &token, "PUT", RULES_PATH, Some(&rules)).await;
        assert_eq!(status, 200);
        send(&request, &token, "GET", DOCK_PATH, None).await;
        let body = json!({ "value": { "position": "left" } });
        send(&request, &token, "PUT", DOCK_PATH, Some(&body)).await;
        let rows = logs_matching(&ctx.db, "request", DOCK_PATH, 2).await;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].method.as_deref(), Some("GET"));
        assert!(rows[1].request_summary.is_none(), "bodies are switched off");

        // Feature off: nothing more from it.
        let rules = rule_body(true, &[("user_preferences", false, true, true)]);
        send(&request, &token, "PUT", RULES_PATH, Some(&rules)).await;
        send(&request, &token, "PUT", DOCK_PATH, Some(&body)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            logs_matching(&ctx.db, "request", DOCK_PATH, 2).await.len(),
            2
        );

        // Global switch off silences every feature, including the rules API itself.
        let rules = rule_body(false, &[("user_preferences", true, true, true)]);
        send(&request, &token, "PUT", RULES_PATH, Some(&rules)).await;
        let before = logs_matching(&ctx.db, "request", "/api/", 0).await.len();
        send(&request, &token, "PUT", DOCK_PATH, Some(&body)).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            logs_matching(&ctx.db, "request", "/api/", 0).await.len(),
            before
        );

        let unknown = rule_body(true, &[("nope", true, false, false)]);
        let (status, _) = send(&request, &token, "PUT", RULES_PATH, Some(&unknown)).await;
        assert_eq!(status, 400);
        audit::invalidate_rules();
    })
    .await;
}

#[tokio::test]
#[serial]
async fn changing_audit_rules_needs_the_config_permission() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let now = chrono::Local::now();
        role_permissions::ActiveModel {
            role_id: Set(OPERATOR_ROLE_ID),
            permission_id: Set(LOG_LIST_PERMISSION_ID),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        }
        .insert(&ctx.db)
        .await
        .unwrap();
        user_roles::ActiveModel {
            user_id: Set(user.user.id),
            role_id: Set(OPERATOR_ROLE_ID),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        }
        .insert(&ctx.db)
        .await
        .unwrap();

        let (status, _) = send(&request, &user.token, "GET", RULES_PATH, None).await;
        assert_eq!(status, 200, "viewing needs only system:log:list");
        let rules = rule_body(false, &[]);
        let (status, _) = send(&request, &user.token, "PUT", RULES_PATH, Some(&rules)).await;
        assert_eq!(status, 403);
    })
    .await;
}
