use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Duration, Local};
use gpt_images::{
    app::App,
    models::_entities::{operation_logs, role_permissions, user_roles},
    responses::ApiResponse,
    views::auth::LoginResponse,
};
use loco_rs::{app::AppContext, testing::prelude::*, TestServer};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, Set,
    Statement,
};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data;

const STATUS_PATH: &str = "/api/admin/logs/retention";
const RUN_PATH: &str = "/api/admin/logs/retention/run";
const OPERATOR_ROLE_ID: i32 = 2;
const LOG_LIST_PERMISSION_ID: i32 = 21;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login_admin(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.11.{ip_suffix}"))
        .json(&json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

async fn call(
    request: &TestServer,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let (key, value) = prepare_data::auth_header(token);
    let builder = match method {
        "GET" => request.get(path),
        "PUT" => request.put(path),
        _ => request.post(path),
    }
    .add_header(key, value);
    let response = match body {
        Some(body) => builder.json(&body).await,
        None => builder.await,
    };
    (
        response.status_code().as_u16(),
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

async fn insert_log(ctx: &AppContext, days_ago: i64, message: &str) {
    let at = (Local::now() - Duration::days(days_ago)).fixed_offset();
    operation_logs::ActiveModel {
        log_type: Set("system".into()),
        level: Set("info".into()),
        module: Set("users".into()),
        action: Set("test".into()),
        message: Set(message.into()),
        created_at: Set(at),
        updated_at: Set(at),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await
    .unwrap();
}

async fn count_messages(ctx: &AppContext, message: &str) -> u64 {
    operation_logs::Entity::find()
        .filter(operation_logs::Column::Message.eq(message))
        .count(&ctx.db)
        .await
        .unwrap()
}

#[tokio::test]
#[serial]
async fn old_logs_are_removed_by_the_configured_retention() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        // 6000 logs past the default 90 days: more than one delete batch.
        ctx.db
            .execute(Statement::from_string(
                ctx.db.get_database_backend(),
                "INSERT INTO operation_logs (log_type, level, module, action, message, created_at, updated_at) \
                 SELECT 'system', 'info', 'users', 'test', 'ancient', now() - interval '120 days', now() \
                 FROM generate_series(1, 6000)"
                    .to_string(),
            ))
            .await
            .unwrap();
        insert_log(&ctx, 30, "month old").await;
        insert_log(&ctx, 0, "today").await;
        let token = login_admin(&request).await;

        let (status, body) = call(&request, &token, "GET", STATUS_PATH, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["retention_days"], 90);
        assert_eq!(body["data"]["expired_count"], 6000);

        let (status, body) = call(&request, &token, "POST", RUN_PATH, None).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["deleted"], 6000);
        assert_eq!(body["data"]["trigger"], "manual");
        assert_eq!(count_messages(&ctx, "ancient").await, 0);
        assert_eq!(count_messages(&ctx, "month old").await, 1);
        assert_eq!(count_messages(&ctx, "手动清理 6000 条超过 90 天的日志").await, 1);

        let (_, body) = call(&request, &token, "GET", STATUS_PATH, None).await;
        assert_eq!(body["data"]["expired_count"], 0);
        assert_eq!(body["data"]["last_cleanup"]["deleted"], 6000);

        // A shorter retention takes the month-old log too.
        let (status, body) = call(
            &request,
            &token,
            "PUT",
            STATUS_PATH,
            Some(json!({ "retention_days": 20 })),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["expired_count"], 1);
        call(&request, &token, "POST", RUN_PATH, None).await;
        assert_eq!(count_messages(&ctx, "month old").await, 0);
        assert_eq!(count_messages(&ctx, "today").await, 1);

        // 0 keeps everything.
        insert_log(&ctx, 400, "very old").await;
        call(&request, &token, "PUT", STATUS_PATH, Some(json!({ "retention_days": 0 }))).await;
        let (_, body) = call(&request, &token, "POST", RUN_PATH, None).await;
        assert_eq!(body["data"]["deleted"], 0);
        assert_eq!(count_messages(&ctx, "very old").await, 1);

        for days in [-1, 5000] {
            let (status, _) = call(
                &request,
                &token,
                "PUT",
                STATUS_PATH,
                Some(json!({ "retention_days": days })),
            )
            .await;
            assert_eq!(status, 400, "{days} days");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn changing_retention_and_cleaning_need_permissions() {
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

        let (status, _) = call(&request, &user.token, "GET", STATUS_PATH, None).await;
        assert_eq!(status, 200);
        let (status, _) = call(
            &request,
            &user.token,
            "PUT",
            STATUS_PATH,
            Some(json!({ "retention_days": 1 })),
        )
        .await;
        assert_eq!(status, 403);
        let (status, _) = call(&request, &user.token, "POST", RUN_PATH, None).await;
        assert_eq!(status, 403);
    })
    .await;
}
