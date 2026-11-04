//! Deleting moves records into the recycle bin; restoring brings back the
//! record with its cascaded rows and re-links what pointed at it.

use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{
    app::App,
    models::_entities::{recycle_bin, role_permissions, user_roles, users},
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data::{auth_header, confirm_password};

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn admin_token(request: &TestServer) -> String {
    let suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.10.{suffix}"))
        .json(&json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    let body: Value = serde_json::from_str(&response.text()).unwrap();
    body["data"]["token"].as_str().unwrap().to_string()
}

async fn call(
    request: &TestServer,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    let (key, value) = auth_header(token);
    let builder = match method {
        "GET" => request.get(path),
        "POST" => request.post(path),
        "PUT" => request.put(path),
        "DELETE" => request.delete(path),
        _ => unreachable!(),
    };
    let response = if method == "GET" || method == "DELETE" {
        builder.add_header(key, value).await
    } else {
        builder.add_header(key, value).json(&body).await
    };
    (
        response.status_code().as_u16(),
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

async fn bin_entry(request: &TestServer, token: &str, resource: &str) -> Value {
    let (status, body) = call(
        request,
        "GET",
        &format!("/api/admin/recycle-bin?resource={resource}"),
        token,
        Value::Null,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    body["data"]["items"][0].clone()
}

#[tokio::test]
#[serial]
async fn a_deleted_role_comes_back_with_its_grants_and_members() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = admin_token(&request).await;

        let (status, role) = call(
            &request,
            "POST",
            "/api/admin/roles",
            &token,
            json!({ "name": "审核员", "code": "auditor", "enabled": true }),
        )
        .await;
        assert_eq!(status, 200, "{role}");
        let role_id = role["data"]["id"].as_i64().unwrap();
        let (status, _) = call(
            &request,
            "PUT",
            &format!("/api/admin/roles/{role_id}/permissions"),
            &token,
            json!({ "permission_ids": [1, 2] }),
        )
        .await;
        assert_eq!(status, 200);
        let (status, _) = call(
            &request,
            "PUT",
            "/api/admin/users/1/roles",
            &token,
            json!({ "role_ids": [role_id] }),
        )
        .await;
        assert_eq!(status, 200);

        let (status, _) = call(
            &request,
            "DELETE",
            &format!("/api/admin/roles/{role_id}"),
            &token,
            Value::Null,
        )
        .await;
        assert_eq!(status, 200);
        let role_id_i32 = i32::try_from(role_id).unwrap();
        let grants = || {
            role_permissions::Entity::find()
                .filter(role_permissions::Column::RoleId.eq(role_id_i32))
                .count(&ctx.db)
        };
        assert_eq!(grants().await.unwrap(), 0);

        let entry = bin_entry(&request, &token, "roles").await;
        assert_eq!(entry["title"], "审核员");
        assert_eq!(entry["resource_label"], "角色");
        assert!(entry["item_count"].as_u64().unwrap() >= 4, "{entry}");
        assert_eq!(entry["deleted_by_name"].as_str().is_some(), true);

        let entry_id = entry["id"].as_i64().unwrap();
        let (status, body) = call(
            &request,
            "POST",
            &format!("/api/admin/recycle-bin/{entry_id}/restore"),
            &token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(grants().await.unwrap(), 2);
        let member = user_roles::Entity::find()
            .filter(user_roles::Column::RoleId.eq(role_id_i32))
            .filter(user_roles::Column::UserId.eq(1))
            .count(&ctx.db)
            .await
            .unwrap();
        assert_eq!(member, 1);
        let (status, role) = call(
            &request,
            "GET",
            &format!("/api/admin/roles/{role_id}"),
            &token,
            Value::Null,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(role["data"]["code"], "auditor");
        assert_eq!(recycle_bin::Entity::find().count(&ctx.db).await.unwrap(), 0);

        // New rows keep getting fresh ids after the restored one.
        let (status, newer) = call(
            &request,
            "POST",
            "/api/admin/roles",
            &token,
            json!({ "name": "新角色", "code": "newer", "enabled": true }),
        )
        .await;
        assert_eq!(status, 200, "{newer}");
        assert!(newer["data"]["id"].as_i64().unwrap() > role_id);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn restoring_reports_conflicts_and_purging_needs_confirmation() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = admin_token(&request).await;
        let (_, role) = call(
            &request,
            "POST",
            "/api/admin/roles",
            &token,
            json!({ "name": "临时", "code": "temp_role", "enabled": true }),
        )
        .await;
        let role_id = role["data"]["id"].as_i64().unwrap();
        call(
            &request,
            "DELETE",
            &format!("/api/admin/roles/{role_id}"),
            &token,
            Value::Null,
        )
        .await;
        // Something else took the unique code in the meantime.
        call(
            &request,
            "POST",
            "/api/admin/roles",
            &token,
            json!({ "name": "临时2", "code": "temp_role", "enabled": true }),
        )
        .await;

        let entry_id = bin_entry(&request, &token, "roles").await["id"]
            .as_i64()
            .unwrap();
        let (status, body) = call(
            &request,
            "POST",
            &format!("/api/admin/recycle-bin/{entry_id}/restore"),
            &token,
            json!({}),
        )
        .await;
        assert_eq!(status, 400);
        assert!(
            body["message"].as_str().unwrap().contains("无法恢复"),
            "{body}"
        );
        assert_eq!(
            recycle_bin::Entity::find().count(&ctx.db).await.unwrap(),
            1,
            "entry kept"
        );

        let purge = format!("/api/admin/recycle-bin/{entry_id}");
        let (status, body) = call(&request, "DELETE", &purge, &token, Value::Null).await;
        assert_eq!(status, 403);
        assert_eq!(body["code"], "REAUTH_REQUIRED");
        confirm_password(&request, &token, "1234").await;
        let (status, _) = call(&request, "DELETE", &purge, &token, Value::Null).await;
        assert_eq!(status, 200);
        assert_eq!(recycle_bin::Entity::find().count(&ctx.db).await.unwrap(), 0);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn a_deleted_user_can_sign_in_again_after_restore() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = admin_token(&request).await;
        let (status, created) = call(
            &request,
            "POST",
            "/api/admin/users",
            &token,
            json!({ "name": "临时用户", "email": "temp@example.com", "password": "Temp12345" }),
        )
        .await;
        assert_eq!(status, 200, "{created}");
        let user_id = created["data"]["id"].as_i64().unwrap();

        let (status, _) = call(
            &request,
            "DELETE",
            &format!("/api/admin/users/{user_id}"),
            &token,
            Value::Null,
        )
        .await;
        assert_eq!(status, 200);
        assert!(users::Entity::find_by_id(i32::try_from(user_id).unwrap())
            .one(&ctx.db)
            .await
            .unwrap()
            .is_none());

        let entry = bin_entry(&request, &token, "users").await;
        assert!(entry["title"]
            .as_str()
            .unwrap()
            .contains("temp@example.com"));
        let entry_id = entry["id"].as_i64().unwrap();
        let (status, body) = call(
            &request,
            "POST",
            &format!("/api/admin/recycle-bin/{entry_id}/restore"),
            &token,
            json!({}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let login = request
            .post("/api/auth/login")
            .add_header("x-forwarded-for", "127.0.10.250")
            .json(&json!({ "email": "temp@example.com", "password": "Temp12345" }))
            .await;
        assert_eq!(login.status_code(), 200, "{}", login.text());
    })
    .await;
}
