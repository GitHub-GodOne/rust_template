use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{
    app::App,
    models::_entities::{role_permissions, user_roles},
    responses::ApiResponse,
    views::auth::LoginResponse,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, Set};
use serial_test::serial;

use super::prepare_data;

const OVERVIEW_PATH: &str = "/api/admin/dashboard/overview";
const DASHBOARD_PERMISSION_ID: i32 = 1;
const OPERATOR_ROLE_ID: i32 = 2;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login(request: &TestServer, email: &str, password: &str) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.2.{ip_suffix}"))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());

    let login_response: ApiResponse<LoginResponse> =
        serde_json::from_str(&response.text()).unwrap();
    login_response.data.unwrap().token
}

async fn get_overview(request: &TestServer, token: &str) -> (u16, serde_json::Value) {
    let (auth_key, auth_value) = prepare_data::auth_header(token);
    let response = request
        .get(OVERVIEW_PATH)
        .add_header(auth_key, auth_value)
        .await;
    let status = response.status_code().as_u16();
    (status, serde_json::from_str(&response.text()).unwrap())
}

fn todo_count(body: &serde_json::Value, key: &str) -> u64 {
    body["data"]["todos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|todo| todo["key"] == key)
        .unwrap_or_else(|| panic!("todo {key} missing"))["count"]
        .as_u64()
        .unwrap()
}

#[tokio::test]
#[serial]
async fn dashboard_overview_requires_auth() {
    request::<App, _, _>(|request, _ctx| async move {
        let response = request.get(OVERVIEW_PATH).await;
        assert_eq!(response.status_code(), 401);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_dashboard_overview_reports_live_data() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let token = login(&request, "admin@example.com", "1234").await;
        let (auth_key, auth_value) = prepare_data::auth_header(&token);

        let current = request
            .get("/api/auth/current")
            .add_header(auth_key.clone(), auth_value.clone())
            .await;
        assert_eq!(current.status_code(), 200);
        let current: serde_json::Value = serde_json::from_str(&current.text()).unwrap();
        let home_menu = current["data"]["menus"]
            .as_array()
            .unwrap()
            .iter()
            .find(|menu| menu["path"] == "/admin/dashboard")
            .expect("home menu is granted to super admin");
        assert_eq!(home_menu["title"], "首页");
        assert_eq!(home_menu["icon"], "home");

        let (status, before) = get_overview(&request, &token).await;
        assert_eq!(status, 200);
        let users_before = before["data"]["stats"]["users"]["total"].as_u64().unwrap();

        let overdue = request
            .post("/api/admin/work-orders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "title": "首页逾期工单",
                "description": "指派给管理员且已超过截止时间",
                "priority": "high",
                "assignee_id": 3,
                "due_at": "2020-01-01T00:00:00+08:00"
            }))
            .await;
        assert_eq!(overdue.status_code(), 200, "{}", overdue.text());
        let unassigned = request
            .post("/api/admin/work-orders")
            .add_header(auth_key.clone(), auth_value.clone())
            .json(&serde_json::json!({
                "title": "首页待分派工单",
                "description": "尚未指派处理人"
            }))
            .await;
        assert_eq!(unassigned.status_code(), 200, "{}", unassigned.text());

        let (status, body) = get_overview(&request, &token).await;
        assert_eq!(status, 200);
        let data = &body["data"];

        assert_eq!(data["system"]["db_ok"], true);
        assert!(!data["system"]["version"].as_str().unwrap().is_empty());
        assert_eq!(data["stats"]["users"]["total"].as_u64(), Some(users_before));

        let work_orders = &data["stats"]["work_orders"];
        assert_eq!(work_orders["active"].as_u64(), Some(2));
        assert_eq!(work_orders["unassigned"].as_u64(), Some(1));
        assert_eq!(work_orders["assigned_to_me"].as_u64(), Some(1));
        assert_eq!(work_orders["overdue"].as_u64(), Some(1));
        assert_eq!(todo_count(&body, "unassigned_work_orders"), 1);
        assert_eq!(todo_count(&body, "my_work_orders"), 1);
        assert_eq!(todo_count(&body, "overdue_work_orders"), 1);
        todo_count(&body, "pending_refunds");
        todo_count(&body, "unread_notifications");
        todo_count(&body, "failed_task_runs");
        todo_count(&body, "locked_users");

        let my_work_orders = data["my_work_orders"].as_array().unwrap();
        assert_eq!(my_work_orders.len(), 2);
        assert!(my_work_orders
            .iter()
            .any(|item| item["title"] == "首页逾期工单" && item["status"] == "assigned"));

        assert!(data["stats"]["payments"].is_object());
        assert!(data["stats"]["notifications"]["total"].as_u64().is_some());
        assert!(data["recent_notifications"].is_array());

        let activity = &data["stats"]["activity"];
        assert!(activity["today_logins"].as_u64().unwrap() >= 1);
        let trend = data["activity_trend"].as_array().unwrap();
        assert_eq!(trend.len(), 7);
        assert!(trend.last().unwrap()["total"].as_u64().unwrap() >= 1);
        assert!(!data["recent_logs"].as_array().unwrap().is_empty());

        let logins = data["recent_logins"].as_array().unwrap();
        assert!(!logins.is_empty());
        assert_eq!(logins[0]["success"], true);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn dashboard_overview_hides_sections_without_permissions() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();

        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (status, _) = get_overview(&request, &user.token).await;
        assert_eq!(status, 403, "users without dashboard:view are rejected");

        let now = chrono::Local::now();
        role_permissions::ActiveModel {
            role_id: Set(OPERATOR_ROLE_ID),
            permission_id: Set(DASHBOARD_PERMISSION_ID),
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

        let (status, body) = get_overview(&request, &user.token).await;
        assert_eq!(status, 200);
        let data = &body["data"];
        assert_eq!(data["system"]["db_ok"], true);
        for section in [
            "users",
            "work_orders",
            "payments",
            "notifications",
            "activity",
        ] {
            assert!(data["stats"][section].is_null(), "{section} must be hidden");
        }
        assert!(data["todos"].as_array().unwrap().is_empty());
        assert!(data["my_work_orders"].is_null());
        assert!(data["recent_notifications"].is_null());
        assert!(data["recent_logs"].is_null());
        assert!(data["activity_trend"].is_null());
        assert!(data["recent_logins"].is_array());
    })
    .await;
}
