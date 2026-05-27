use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{testing::prelude::*, TestServer};
use serial_test::serial;

use super::prepare_data;

async fn admin_token(request: &TestServer) -> String {
    let response = request
        .post("/api/auth/login")
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
            "/api/admin/dict-types",
            "/api/admin/dict-types/1/items",
            "/api/admin/uploads",
            "/api/admin/tenants",
            "/api/admin/data-scopes",
        ] {
            let response = request.get(path).await;
            assert_eq!(response.status_code(), 401, "{path}");
        }
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
