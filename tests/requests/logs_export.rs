use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Duration, Local, TimeZone};
use gpt_images::{
    app::App,
    models::_entities::{operation_logs, role_permissions, user_roles},
    responses::ApiResponse,
    views::auth::LoginResponse,
};
use loco_rs::{app::AppContext, testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data;

const LOGS_PATH: &str = "/api/admin/logs";
const EXPORT_PATH: &str = "/api/admin/logs/export";
const OPERATOR_ROLE_ID: i32 = 2;
const LOG_LIST_PERMISSION_ID: i32 = 21;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login_admin(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.10.{ip_suffix}"))
        .json(&json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

/// Three `system` logs: two days ago, yesterday and today (local noon).
async fn insert_logs(ctx: &AppContext) {
    let noon = Local::now().date_naive().and_hms_opt(12, 0, 0).unwrap();
    for (days_ago, message) in [(2, "两天前"), (1, "昨天, 含逗号"), (0, "=今天")] {
        let at = Local
            .from_local_datetime(&(noon - Duration::days(days_ago)))
            .unwrap()
            .fixed_offset();
        operation_logs::ActiveModel {
            log_type: Set("system".into()),
            level: Set("info".into()),
            module: Set("users".into()),
            action: Set("test".into()),
            message: Set(message.into()),
            operator: Set(Some("tester".into())),
            created_at: Set(at),
            updated_at: Set(at),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await
        .unwrap();
    }
}

fn day_bounds(days_ago: i64) -> (String, String) {
    let day = Local::now().date_naive() - Duration::days(days_ago);
    let start = Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .unwrap();
    let end = Local
        .from_local_datetime(&day.and_hms_opt(23, 59, 59).unwrap())
        .unwrap();
    (start.to_rfc3339(), end.to_rfc3339())
}

async fn get(
    request: &TestServer,
    token: &str,
    path: &str,
    query: &[(&str, &str)],
) -> (u16, String) {
    let (key, value) = prepare_data::auth_header(token);
    let mut builder = request.get(path).add_header(key, value);
    for (name, value) in query {
        builder = builder.add_query_param(name, value);
    }
    let response = builder.await;
    (response.status_code().as_u16(), response.text())
}

#[tokio::test]
#[serial]
async fn logs_can_be_filtered_by_time_range() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        insert_logs(&ctx).await;
        let token = login_admin(&request).await;

        let (start, end) = day_bounds(1);
        let (status, body) = get(
            &request,
            &token,
            LOGS_PATH,
            &[
                ("log_type", "system"),
                ("start_time", &start),
                ("end_time", &end),
            ],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["data"]["total"], 1);
        assert_eq!(body["data"]["items"][0]["message"], "昨天, 含逗号");

        let (since_yesterday, _) = day_bounds(1);
        let (_, body) = get(
            &request,
            &token,
            LOGS_PATH,
            &[("log_type", "system"), ("start_time", &since_yesterday)],
        )
        .await;
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["data"]["total"], 2, "open-ended range");

        let (status, _) = get(&request, &token, LOGS_PATH, &[("start_time", "yesterday")]).await;
        assert_eq!(status, 400);
        let (status, _) = get(
            &request,
            &token,
            LOGS_PATH,
            &[("start_time", &end), ("end_time", &start)],
        )
        .await;
        assert_eq!(status, 400, "start after end");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn logs_export_as_csv_with_the_same_filters() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        insert_logs(&ctx).await;
        let token = login_admin(&request).await;

        let (key, value) = prepare_data::auth_header(&token);
        let (since_yesterday, _) = day_bounds(1);
        let response = request
            .get(EXPORT_PATH)
            .add_header(key, value)
            // Browsers ask for compression; the streamed CSV must still arrive whole.
            .add_header("accept-encoding", "gzip, deflate, br, zstd")
            .add_query_param("log_type", "system")
            .add_query_param("start_time", &since_yesterday)
            .await;
        assert_eq!(response.status_code(), 200, "{}", response.text());
        let headers = response.headers();
        assert!(headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/csv"));
        assert!(headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("operation-logs-"));
        assert_eq!(headers["x-total-count"], "2");
        assert!(
            headers.get("content-encoding").is_none(),
            "streamed exports are sent uncompressed"
        );

        let csv = response.text();
        assert!(csv.starts_with('\u{feff}'), "BOM so Excel reads UTF-8");
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert!(lines[0].starts_with("ID,时间,级别,类型,功能"));
        assert_eq!(
            lines.len(),
            4,
            "header + 2 rows + trailing empty: {lines:?}"
        );
        assert!(
            lines[1].contains(",'=今天,"),
            "newest first, formula-safe: {}",
            lines[1]
        );
        assert!(lines[2].contains("\"昨天, 含逗号\""), "{}", lines[2]);
        assert!(lines[1].contains(",用户管理,"), "feature shown by name");
        assert!(!csv.contains("两天前"));

        // Ordinary JSON responses are still compressed.
        let (key, value) = prepare_data::auth_header(&token);
        let listed = request
            .get(LOGS_PATH)
            .add_header(key, value)
            .add_header("accept-encoding", "gzip")
            .await;
        assert_eq!(listed.headers()["content-encoding"], "gzip");

        let exports = operation_logs::Entity::find()
            .filter(operation_logs::Column::Action.eq("export"))
            .all(&ctx.db)
            .await
            .unwrap();
        assert_eq!(exports.len(), 1, "each export is itself logged");
        assert!(exports[0].message.contains("导出日志 2 条"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn exporting_logs_needs_the_export_permission() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let now = Local::now();
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

        let (status, _) = get(&request, &user.token, LOGS_PATH, &[]).await;
        assert_eq!(status, 200);
        let (status, _) = get(&request, &user.token, EXPORT_PATH, &[]).await;
        assert_eq!(status, 403);
    })
    .await;
}
