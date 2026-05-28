use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{testing::prelude::*, TestServer};
use serial_test::serial;

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
        for path in [
            "/api/admin/logs",
            "/api/admin/settings",
            "/api/admin/email-templates",
            "/api/admin/dict-types",
            "/api/admin/dict-types/1/items",
            "/api/admin/uploads",
            "/api/admin/tenants",
            "/api/admin/data-scopes",
            "/api/admin/notifications",
            "/api/admin/scheduled-tasks",
            "/api/admin/scheduled-task-runs",
            "/api/admin/backups",
            "/api/admin/rate-limits",
            "/api/admin/rate-limit-events",
            "/api/admin/monitoring/overview",
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
        assert!(body.contains("/admin/system/users"));
        assert!(body.contains("\"effective_data_scope\":\"all\""));
        assert!(body.contains("\"tenant\":{"));
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
            "/api/admin/email-templates",
            "/api/admin/dict-types",
            "/api/admin/dict-types/1/items",
            "/api/admin/uploads",
            "/api/admin/tenants",
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
                "enabled": true
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
        let backup_status = backup_body["data"]["status"].as_str().unwrap();
        assert!(matches!(backup_status, "success" | "failed"));
        assert!(backup_body["data"]["delivery_status"]
            .as_str()
            .unwrap()
            .contains("no delivery targets configured"));

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
