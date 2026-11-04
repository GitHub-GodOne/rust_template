use std::sync::atomic::{AtomicUsize, Ordering};

use axum::http::{HeaderName, HeaderValue};
use gpt_images::{app::App, responses::ApiResponse, views::auth::LoginResponse};
use loco_rs::{testing::prelude::*, TestServer};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);
const BOUNDARY: &str = "im-upload-test-boundary";
const PASSWORD: &str = "Chat123pass";

type Auth = (HeaderName, HeaderValue);

async fn login(request: &TestServer, email: &str, password: &str) -> Auth {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let login = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.31.{ip_suffix}"))
        .json(&json!({ "email": email, "password": password }))
        .await;
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&login.text()).unwrap();
    prepare_data::auth_header(&login.data.unwrap().token)
}

/// Creates a user (optionally with roles), signs them in and returns their id.
async fn user(request: &TestServer, admin: &Auth, email: &str, role_ids: &[i32]) -> (i64, Auth) {
    let created = request
        .post("/api/admin/users")
        .add_header(admin.0.clone(), admin.1.clone())
        .json(&json!({ "name": email, "email": email, "password": PASSWORD }))
        .await;
    assert_eq!(created.status_code(), 200, "{}", created.text());
    let created: Value = serde_json::from_str(&created.text()).unwrap();
    let id = created["data"]["id"].as_i64().unwrap();
    if !role_ids.is_empty() {
        let roles = request
            .put(&format!("/api/admin/users/{id}/roles"))
            .add_header(admin.0.clone(), admin.1.clone())
            .json(&json!({ "role_ids": role_ids }))
            .await;
        assert_eq!(roles.status_code(), 200, "{}", roles.text());
    }
    (id, login(request, email, PASSWORD).await)
}

async fn get_json(request: &TestServer, path: &str, auth: &Auth) -> Value {
    let response = request
        .get(path)
        .add_header(auth.0.clone(), auth.1.clone())
        .await;
    assert_eq!(response.status_code(), 200, "{path}: {}", response.text());
    serde_json::from_str::<Value>(&response.text()).unwrap()["data"].clone()
}

async fn post_json(request: &TestServer, path: &str, auth: &Auth, body: Value) -> Value {
    let response = request
        .post(path)
        .add_header(auth.0.clone(), auth.1.clone())
        .json(&body)
        .await;
    assert_eq!(response.status_code(), 200, "{path}: {}", response.text());
    serde_json::from_str::<Value>(&response.text()).unwrap()["data"].clone()
}

fn conversation<'a>(list: &'a Value, id: i64) -> Option<&'a Value> {
    list.as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_i64() == Some(id))
}

#[tokio::test]
#[serial]
async fn direct_chats_track_unread_messages_and_stay_private() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = login(&request, "admin@example.com", "1234").await;
        let (alice_id, alice) = user(&request, &admin, "alice-im@example.com", &[1]).await;
        let (_, carol) = user(&request, &admin, "carol-im@example.com", &[1]).await;
        let (_, guest) = user(&request, &admin, "guest-im@example.com", &[]).await;

        // Chatting needs system:im:chat.
        let denied = request
            .get("/api/admin/im/conversations")
            .add_header(guest.0.clone(), guest.1.clone())
            .await;
        assert_eq!(denied.status_code(), 403);

        let contacts = get_json(&request, "/api/admin/im/contacts?keyword=alice-im", &admin).await;
        assert_eq!(contacts[0]["id"].as_i64(), Some(alice_id));

        let direct = post_json(
            &request,
            "/api/admin/im/conversations/direct",
            &admin,
            json!({ "user_id": alice_id }),
        )
        .await;
        let id = direct["id"].as_i64().unwrap();
        assert_eq!(direct["kind"], "direct");
        assert_eq!(direct["peer_id"].as_i64(), Some(alice_id));
        // Opening the same chat again reuses it.
        let again = post_json(
            &request,
            "/api/admin/im/conversations/direct",
            &admin,
            json!({ "user_id": alice_id }),
        )
        .await;
        assert_eq!(again["id"].as_i64(), Some(id));

        let sent = post_json(
            &request,
            &format!("/api/admin/im/conversations/{id}/messages"),
            &admin,
            json!({ "content": "你好 Alice" }),
        )
        .await;
        assert_eq!(sent["kind"], "text");

        let list = get_json(&request, "/api/admin/im/conversations", &alice).await;
        let chat = conversation(&list, id).unwrap();
        assert_eq!(chat["unread_count"], 1);
        assert_eq!(chat["last_message"]["content"], "你好 Alice");
        assert_eq!(chat["name"], "admin");
        let sender_list = get_json(&request, "/api/admin/im/conversations", &admin).await;
        assert_eq!(conversation(&sender_list, id).unwrap()["unread_count"], 0);

        let messages = get_json(
            &request,
            &format!("/api/admin/im/conversations/{id}/messages"),
            &alice,
        )
        .await;
        assert_eq!(messages.as_array().unwrap().len(), 1);
        post_json(
            &request,
            &format!("/api/admin/im/conversations/{id}/read"),
            &alice,
            json!({}),
        )
        .await;
        let list = get_json(&request, "/api/admin/im/conversations", &alice).await;
        assert_eq!(conversation(&list, id).unwrap()["unread_count"], 0);

        // Outsiders cannot read or post into someone else's chat.
        let peek = request
            .get(&format!("/api/admin/im/conversations/{id}/messages"))
            .add_header(carol.0.clone(), carol.1.clone())
            .await;
        assert_eq!(peek.status_code(), 404);
        let body: Value = serde_json::from_str(&peek.text()).unwrap();
        assert_eq!(body["code"], "IM_CONVERSATION_NOT_FOUND");
        let post = request
            .post(&format!("/api/admin/im/conversations/{id}/messages"))
            .add_header(carol.0.clone(), carol.1.clone())
            .json(&json!({ "content": "hi" }))
            .await;
        assert_eq!(post.status_code(), 404);

        let empty = request
            .post(&format!("/api/admin/im/conversations/{id}/messages"))
            .add_header(admin.0.clone(), admin.1.clone())
            .json(&json!({ "content": "   " }))
            .await;
        assert_eq!(empty.status_code(), 400);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn groups_share_files_and_only_owners_manage_members() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let admin = login(&request, "admin@example.com", "1234").await;
        let (alice_id, alice) = user(&request, &admin, "alice-group@example.com", &[1]).await;
        let (bob_id, bob) = user(&request, &admin, "bob-group@example.com", &[1]).await;
        let (carol_id, carol) = user(&request, &admin, "carol-group@example.com", &[1]).await;

        let group = post_json(
            &request,
            "/api/admin/im/conversations/group",
            &admin,
            json!({ "name": "项目群", "member_ids": [alice_id, bob_id] }),
        )
        .await;
        let id = group["id"].as_i64().unwrap();
        assert_eq!(group["kind"], "group");
        assert_eq!(group["member_count"], 3);
        let messages = get_json(
            &request,
            &format!("/api/admin/im/conversations/{id}/messages"),
            &alice,
        )
        .await;
        let created: Value =
            serde_json::from_str(messages[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(messages[0]["kind"], "system");
        assert_eq!(created["action"], "created");

        // Any member may invite; carol joins and sees the history.
        let invited = post_json(
            &request,
            &format!("/api/admin/im/conversations/{id}/members"),
            &alice,
            json!({ "user_ids": [carol_id] }),
        )
        .await;
        assert_eq!(invited["member_count"], 4);
        get_json(
            &request,
            &format!("/api/admin/im/conversations/{id}"),
            &carol,
        )
        .await;

        // Images are stored and served inline to members only.
        let body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"shot.png\"\r\nContent-Type: image/png\r\n\r\npng-bytes\r\n--{BOUNDARY}--\r\n"
        );
        let uploaded = request
            .post(&format!("/api/admin/im/conversations/{id}/files"))
            .add_header(bob.0.clone(), bob.1.clone())
            .content_type(&format!("multipart/form-data; boundary={BOUNDARY}"))
            .bytes(axum::body::Bytes::from(body))
            .await;
        assert_eq!(uploaded.status_code(), 200, "{}", uploaded.text());
        let uploaded: Value = serde_json::from_str(&uploaded.text()).unwrap();
        assert_eq!(uploaded["data"]["kind"], "image");
        assert_eq!(uploaded["data"]["file_name"], "shot.png");
        let message_id = uploaded["data"]["id"].as_i64().unwrap();
        let file = request
            .get(&format!("/api/admin/im/messages/{message_id}/file"))
            .add_header(alice.0.clone(), alice.1.clone())
            .await;
        assert_eq!(file.status_code(), 200);
        assert_eq!(file.text(), "png-bytes");
        assert_eq!(file.header("content-type"), "image/png");

        // Only the owner removes others; members may leave themselves.
        let kick = request
            .delete(&format!("/api/admin/im/conversations/{id}/members/{carol_id}"))
            .add_header(alice.0.clone(), alice.1.clone())
            .await;
        assert_eq!(kick.status_code(), 403);
        let kick = request
            .delete(&format!("/api/admin/im/conversations/{id}/members/{carol_id}"))
            .add_header(admin.0.clone(), admin.1.clone())
            .await;
        assert_eq!(kick.status_code(), 200, "{}", kick.text());
        let gone = request
            .get(&format!("/api/admin/im/messages/{message_id}/file"))
            .add_header(carol.0.clone(), carol.1.clone())
            .await;
        assert_eq!(gone.status_code(), 404);
        let leave = request
            .delete(&format!("/api/admin/im/conversations/{id}/members/{bob_id}"))
            .add_header(bob.0.clone(), bob.1.clone())
            .await;
        assert_eq!(leave.status_code(), 200, "{}", leave.text());

        let rename = request
            .put(&format!("/api/admin/im/conversations/{id}"))
            .add_header(alice.0.clone(), alice.1.clone())
            .json(&json!({ "name": "新群名" }))
            .await;
        assert_eq!(rename.status_code(), 200, "{}", rename.text());
        let detail = get_json(
            &request,
            &format!("/api/admin/im/conversations/{id}"),
            &admin,
        )
        .await;
        assert_eq!(detail["name"], "新群名");
        assert_eq!(detail["members"].as_array().unwrap().len(), 2);

        // Only the owner dissolves; the group goes to the recycle bin.
        let dissolve = request
            .delete(&format!("/api/admin/im/conversations/{id}"))
            .add_header(alice.0.clone(), alice.1.clone())
            .await;
        assert_eq!(dissolve.status_code(), 403);
        let dissolve = request
            .delete(&format!("/api/admin/im/conversations/{id}"))
            .add_header(admin.0.clone(), admin.1.clone())
            .await;
        assert_eq!(dissolve.status_code(), 200, "{}", dissolve.text());
        let list = get_json(&request, "/api/admin/im/conversations", &alice).await;
        assert!(conversation(&list, id).is_none());
        let bin = get_json(
            &request,
            "/api/admin/recycle-bin?resource=im_conversations",
            &admin,
        )
        .await;
        assert_eq!(bin["total"], 1);
    })
    .await;
}
