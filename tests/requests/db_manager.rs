use std::sync::atomic::{AtomicUsize, Ordering};

use gpt_images::{
    app::App, models::_entities::db_connections, responses::ApiResponse, views::auth::LoginResponse,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{sqlx, ActiveModelTrait, Set};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login_token(request: &TestServer, email: &str, password: &str) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let login = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.8.{ip_suffix}"))
        .json(&json!({ "email": email, "password": password }))
        .await;
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&login.text()).unwrap();
    login.data.unwrap().token
}

struct Client<'a> {
    request: &'a TestServer,
    token: String,
}

impl Client<'_> {
    async fn send(&self, method: &str, path: &str, body: Option<Value>, expected: u16) -> Value {
        let (key, value) = prepare_data::auth_header(&self.token);
        let builder = match method {
            "GET" => self.request.get(path),
            "POST" => self.request.post(path),
            "PUT" => self.request.put(path),
            _ => self.request.delete(path),
        }
        .add_header(key, value);
        let response = match body {
            Some(body) => builder.json(&body).await,
            None => builder.await,
        };
        assert_eq!(
            response.status_code(),
            expected,
            "{method} {path}: {}",
            response.text()
        );
        let body: Value = serde_json::from_str(&response.text()).unwrap_or(Value::Null);
        if expected == 200 {
            body["data"].clone()
        } else {
            body
        }
    }

    async fn get(&self, path: &str) -> Value {
        self.send("GET", path, None, 200).await
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        self.send("POST", path, Some(body), 200).await
    }
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

async fn sqlite_fixture() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("db-manager-{}.sqlite", uuid::Uuid::new_v4()));
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    for statement in [
        "CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE books (id INTEGER PRIMARY KEY, author_id INTEGER REFERENCES authors(id) ON DELETE CASCADE, title TEXT NOT NULL, price REAL, cover BLOB)",
        "CREATE INDEX idx_books_title ON books (title)",
        "CREATE VIEW cheap_books AS SELECT * FROM books WHERE price < 10",
        "CREATE TABLE notes (body TEXT)",
        "INSERT INTO authors (id, name) VALUES (1, 'Lu Xun'), (2, 'Lao She')",
        "INSERT INTO books (author_id, title, price, cover) VALUES (1, 'Call to Arms', 9.5, X'CAFE'), (1, 'Wandering', 12, NULL), (2, 'Teahouse', 8, NULL)",
        "INSERT INTO notes (body) VALUES ('no key here')",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;
    path
}

#[tokio::test]
#[serial]
async fn sqlite_connections_browse_edit_and_run_sql() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_token(&request, "admin@example.com", "1234").await;
        prepare_data::confirm_password(&request, &token, "1234").await;
        let admin = Client {
            request: &request,
            token: token.clone(),
        };
        let file = sqlite_fixture().await;

        let created = admin
            .post(
                "/api/admin/db-manager/connections",
                json!({
                    "name": "本地 SQLite",
                    "db_type": "sqlite",
                    "database_name": file.display().to_string(),
                    "password": "never-shown"
                }),
            )
            .await;
        let id = created["id"].as_i64().unwrap();
        assert_eq!(created["db_type"], "sqlite");
        assert_eq!(created["password_saved"], true);
        assert!(created.get("password").is_none());
        let base = format!("/api/admin/db-manager/connections/{id}");

        let tested = admin
            .post(
                "/api/admin/db-manager/connections/test",
                json!({ "id": id, "name": "x", "db_type": "sqlite", "database_name": file.display().to_string() }),
            )
            .await;
        assert!(!tested["version"].as_str().unwrap().is_empty());
        admin
            .send(
                "POST",
                "/api/admin/db-manager/connections/test",
                Some(json!({ "name": "x", "db_type": "sqlite", "database_name": "missing.db" })),
                400,
            )
            .await;

        let connected = admin.post(&format!("{base}/connect"), json!({})).await;
        assert_eq!(connected["databases"], json!(["main"]));
        assert_eq!(connected["has_schemas"], false);

        let objects = admin.get(&format!("{base}/objects?database=main")).await;
        let names: Vec<(&str, &str)> = objects
            .as_array()
            .unwrap()
            .iter()
            .map(|object| (object["name"].as_str().unwrap(), object["kind"].as_str().unwrap()))
            .collect();
        assert_eq!(
            names,
            [
                ("authors", "table"),
                ("books", "table"),
                ("cheap_books", "view"),
                ("notes", "table")
            ]
        );

        let structure = admin.get(&format!("{base}/structure?table=books")).await;
        assert_eq!(structure["primary_key"], json!(["id"]));
        let columns = structure["columns"].as_array().unwrap();
        assert_eq!(columns.len(), 5);
        assert_eq!(columns[0]["auto_increment"], true);
        assert_eq!(columns[4]["binary"], true);
        assert!(structure["indexes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|index| index["name"] == "idx_books_title"));
        assert_eq!(structure["foreign_keys"][0]["referenced_table"], "authors");
        assert_eq!(structure["foreign_keys"][0]["on_delete"], "CASCADE");
        assert!(structure["ddl"]
            .as_str()
            .unwrap()
            .contains("CREATE INDEX idx_books_title"));
        admin
            .send("GET", &format!("{base}/structure?table=missing"), None, 404)
            .await;

        // Filters are bound, sorting and paging are applied.
        let filters = encode(r#"[{"column":"title","op":"contains","value":"a"}]"#);
        let sort = encode(r#"[{"column":"price","desc":true}]"#);
        let page = admin
            .get(&format!(
                "{base}/rows?table=books&page=1&page_size=2&filters={filters}&sort={sort}"
            ))
            .await;
        assert_eq!(page["total"], 3);
        assert_eq!(page["rows"].as_array().unwrap().len(), 2);
        assert_eq!(page["rows"][0][2], "Wandering");
        assert_eq!(page["rows"][1][4], "0xcafe");
        let injected = encode(r#"[{"column":"title","op":"eq","value":"x' OR '1'='1"}]"#);
        let none = admin
            .get(&format!("{base}/rows?table=books&filters={injected}"))
            .await;
        assert_eq!(none["total"], 0);
        let unknown = encode(r#"[{"column":"nope","op":"eq","value":"1"}]"#);
        admin
            .send("GET", &format!("{base}/rows?table=books&filters={unknown}"), None, 400)
            .await;
        let raw_where = admin
            .get(&format!(
                "{base}/rows?table=books&where={}",
                encode("price < 10")
            ))
            .await;
        assert_eq!(raw_where["total"], 2);
        admin
            .send(
                "GET",
                &format!(
                    "{base}/rows?table=books&where={}",
                    encode("1 = 1; DELETE FROM books")
                ),
                None,
                400,
            )
            .await;

        // Grid edits run in one transaction.
        let summary = admin
            .post(
                &format!("{base}/changes"),
                json!({
                    "table": "books",
                    "changes": [
                        { "kind": "update", "key": { "id": 1 }, "values": { "title": "呐喊", "price": 10.5 } },
                        { "kind": "insert", "values": { "author_id": 2, "title": "Rickshaw Boy" } },
                        { "kind": "delete", "key": { "id": 3 } }
                    ]
                }),
            )
            .await;
        assert_eq!(summary, json!({ "inserted": 1, "updated": 1, "deleted": 1 }));
        let failed = admin
            .send(
                "POST",
                &format!("{base}/changes"),
                Some(json!({
                    "table": "books",
                    "changes": [
                        { "kind": "delete", "key": { "id": 2 } },
                        { "kind": "update", "key": { "id": 999 }, "values": { "title": "x" } }
                    ]
                })),
                400,
            )
            .await;
        assert_eq!(failed["code"], "DB_QUERY_FAILED");
        let after = admin.get(&format!("{base}/rows?table=books")).await;
        assert_eq!(after["total"], 3, "the failed batch must roll back");
        assert_eq!(after["rows"][0][2], "呐喊");
        admin
            .send(
                "POST",
                &format!("{base}/changes"),
                Some(json!({
                    "table": "notes",
                    "changes": [{ "kind": "delete", "key": { "body": "no key here" } }]
                })),
                400,
            )
            .await;

        // Editor sessions keep transactions between runs.
        let run = |sql: &str| {
            json!({ "session_key": "tab-1", "sql": sql, "max_rows": 2 })
        };
        let result = admin
            .post(
                &format!("{base}/execute"),
                run("SELECT id, title FROM books ORDER BY id; SELECT * FROM books WHERE 1 = 0; -- done"),
            )
            .await;
        assert_eq!(result["statement_count"], 2);
        assert_eq!(result["statements"][0]["rows"].as_array().unwrap().len(), 2);
        assert_eq!(result["statements"][0]["truncated"], true);
        assert_eq!(result["statements"][1]["returns_rows"], true);
        assert_eq!(result["statements"][1]["columns"].as_array().unwrap().len(), 5);
        assert_eq!(result["in_transaction"], false);

        let result = admin
            .post(
                &format!("{base}/execute"),
                run("BEGIN; UPDATE authors SET name = 'Zhou Shuren' WHERE id = 1"),
            )
            .await;
        assert_eq!(result["in_transaction"], true);
        assert_eq!(result["statements"][1]["affected_rows"], 1);
        let rolled_back = admin
            .post(
                &format!("{base}/execute"),
                run("ROLLBACK; SELECT name FROM authors WHERE id = 1"),
            )
            .await;
        assert_eq!(rolled_back["in_transaction"], false);
        assert_eq!(rolled_back["statements"][1]["rows"][0][0], "Lu Xun");

        let errored = admin
            .post(
                &format!("{base}/execute"),
                run("SELECT * FROM nowhere; SELECT 1"),
            )
            .await;
        assert_eq!(errored["statements"].as_array().unwrap().len(), 1);
        assert!(errored["statements"][0]["error"]
            .as_str()
            .unwrap()
            .contains("nowhere"));
        admin
            .send(
                "POST",
                &format!("{base}/execute"),
                Some(json!({ "session_key": "bad:key", "sql": "SELECT 1" })),
                400,
            )
            .await;
        admin
            .send("DELETE", &format!("{base}/sessions/tab-1"), None, 200)
            .await;

        let history = admin
            .get(&format!("/api/admin/db-manager/history?connection_id={id}"))
            .await;
        assert_eq!(history["total"], 4);
        assert_eq!(history["items"][0]["status"], "failed");
        assert_eq!(history["items"][1]["status"], "success");

        let saved = admin
            .post(
                "/api/admin/db-manager/saved-queries",
                json!({ "connection_id": id, "name": "全部图书", "sql_text": "SELECT * FROM books" }),
            )
            .await;
        let saved_id = saved["id"].as_i64().unwrap();
        let listed = admin
            .get(&format!("/api/admin/db-manager/saved-queries?connection_id={id}"))
            .await;
        assert_eq!(listed[0]["name"], "全部图书");
        admin
            .send(
                "DELETE",
                &format!("/api/admin/db-manager/saved-queries/{saved_id}"),
                None,
                200,
            )
            .await;

        // Exports keep binary values whole.
        let (key, value) = prepare_data::auth_header(&token);
        let export = request
            .get(&format!("{base}/export?table=books&format=sql"))
            .add_header(key.clone(), value.clone())
            .await;
        assert_eq!(export.status_code(), 200, "{}", export.text());
        let script = export.text();
        assert!(script.contains("INSERT INTO \"books\" (\"id\", \"author_id\", \"title\", \"price\", \"cover\") VALUES (1, 1, '呐喊', 10.5, X'cafe');"));
        let csv = request
            .get(&format!("{base}/export?table=authors&format=csv"))
            .add_header(key, value)
            .await;
        assert!(csv.text().contains("id,name"));

        // A read-only connection refuses writes.
        let mut updated = created.clone();
        updated["read_only"] = json!(true);
        admin
            .send("PUT", &base, Some(updated), 200)
            .await;
        let refused = admin
            .send(
                "POST",
                &format!("{base}/changes"),
                Some(json!({ "table": "books", "changes": [{ "kind": "delete", "key": { "id": 1 } }] })),
                400,
            )
            .await;
        assert_eq!(refused["code"], "DB_READ_ONLY");
        let write = admin
            .post(
                &format!("{base}/execute"),
                json!({ "session_key": "tab-2", "sql": "DELETE FROM books" }),
            )
            .await;
        assert!(write["statements"][0]["error"].as_str().is_some());

        // A user without a role sees nothing, and nobody sees others' connections.
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let outsider = Client {
            request: &request,
            token: user.token,
        };
        outsider
            .send("GET", "/api/admin/db-manager/connections", None, 403)
            .await;
        outsider
            .send("POST", &format!("{base}/execute"), Some(run("SELECT 1")), 403)
            .await;
        let foreign = db_connections::ActiveModel {
            name: Set("别人的".to_string()),
            db_type: Set("sqlite".to_string()),
            database_name: Set(Some(file.display().to_string())),
            read_only: Set(false),
            sort_order: Set(0),
            user_id: Set(user.user.id),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await
        .unwrap();
        let hidden = admin
            .send(
                "GET",
                &format!("/api/admin/db-manager/connections/{}/objects", foreign.id),
                None,
                404,
            )
            .await;
        assert_eq!(hidden["code"], "DB_CONNECTION_NOT_FOUND");
        let list = admin.get("/api/admin/db-manager/connections").await;
        assert!(list.as_array().unwrap().iter().all(|item| item["id"] != foreign.id));

        admin.send("DELETE", &base, None, 200).await;
        admin
            .send("GET", &format!("{base}/objects"), None, 404)
            .await;
        let _ = std::fs::remove_file(&file);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn postgres_connections_read_the_catalog() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_token(&request, "admin@example.com", "1234").await;
        prepare_data::confirm_password(&request, &token, "1234").await;
        let admin = Client {
            request: &request,
            token,
        };
        let uri = url::Url::parse(&ctx.config.database.uri).unwrap();
        let password = url::form_urlencoded::parse(format!("p={}", uri.password().unwrap_or_default()).as_bytes())
            .next()
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default();
        let database = uri.path().trim_start_matches('/').to_string();
        let created = admin
            .post(
                "/api/admin/db-manager/connections",
                json!({
                    "name": "测试库",
                    "db_type": "postgres",
                    "host": uri.host_str().unwrap(),
                    "port": uri.port().unwrap_or(5432),
                    "username": uri.username(),
                    "password": password,
                    "database_name": database,
                    "ssl_mode": "disable",
                    "read_only": true
                }),
            )
            .await;
        let base = format!("/api/admin/db-manager/connections/{}", created["id"]);

        let connected = admin.post(&format!("{base}/connect"), json!({})).await;
        assert_eq!(connected["has_schemas"], true);
        assert_eq!(connected["default_database"], database.as_str());
        assert!(connected["databases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == database.as_str()));
        let schemas = admin
            .get(&format!("{base}/schemas?database={database}"))
            .await;
        assert_eq!(schemas[0], "public");
        let objects = admin
            .get(&format!("{base}/objects?database={database}&schema=public"))
            .await;
        assert!(objects
            .as_array()
            .unwrap()
            .iter()
            .any(|object| object["name"] == "users" && object["kind"] == "table"));

        let structure = admin
            .get(&format!("{base}/structure?schema=public&table=users"))
            .await;
        assert_eq!(structure["primary_key"], json!(["id"]));
        assert!(structure["ddl"]
            .as_str()
            .unwrap()
            .starts_with("CREATE TABLE \"public\".\"users\" ("));

        let filters = encode(r#"[{"column":"email","op":"eq","value":"admin@example.com"}]"#);
        let rows = admin
            .get(&format!("{base}/rows?schema=public&table=users&filters={filters}"))
            .await;
        assert_eq!(rows["total"], 1);
        assert_eq!(rows["read_only"], true);
        let email = rows["columns"]
            .as_array()
            .unwrap()
            .iter()
            .position(|column| column["name"] == "email")
            .unwrap();
        assert_eq!(rows["rows"][0][email], "admin@example.com");

        let result = admin
            .post(
                &format!("{base}/execute"),
                json!({
                    "session_key": "pg-tab",
                    "sql": "SELECT 1::int8 AS n, 'a'::text AS t, NULL::int AS z, '{1,2}'::int4[] AS arr, '2026-01-02'::date AS d, $$x;y$$ AS q; BEGIN; SELECT 2"
                }),
            )
            .await;
        let first = &result["statements"][0];
        assert_eq!(first["rows"][0], json!([1, "a", null, "{1,2}", "2026-01-02", "x;y"]));
        assert_eq!(result["in_transaction"], true);
        let result = admin
            .post(
                &format!("{base}/execute"),
                json!({ "session_key": "pg-tab", "sql": "UPDATE users SET name = name; ROLLBACK" }),
            )
            .await;
        assert!(result["statements"][0]["error"]
            .as_str()
            .unwrap()
            .contains("read-only"));
        assert_eq!(result["in_transaction"], true);
        let result = admin
            .post(
                &format!("{base}/execute"),
                json!({ "session_key": "pg-tab", "sql": "ROLLBACK" }),
            )
            .await;
        assert_eq!(result["in_transaction"], false);
        admin
            .post(&format!("{base}/sessions/pg-tab/cancel"), json!({}))
            .await;

        let completion = admin
            .get(&format!("{base}/completion?schema=public"))
            .await;
        assert!(completion["tables"]["users"]
            .as_array()
            .unwrap()
            .iter()
            .any(|column| column == "email"));
        admin.post(&format!("{base}/disconnect"), json!({})).await;
    })
    .await;
}

/// Needs a MySQL server: `DB_MANAGER_MYSQL_URL=mysql://user:password@127.0.0.1:3306/some_db
/// cargo test --all-features --test mod mysql_connections -- --ignored`. The
/// test creates and drops its own table in that database.
#[tokio::test]
#[serial]
#[ignore = "needs DB_MANAGER_MYSQL_URL"]
async fn mysql_connections_browse_edit_and_run_sql() {
    let Ok(mysql_url) = std::env::var("DB_MANAGER_MYSQL_URL") else {
        return;
    };
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_token(&request, "admin@example.com", "1234").await;
        prepare_data::confirm_password(&request, &token, "1234").await;
        let admin = Client {
            request: &request,
            token,
        };
        let uri = url::Url::parse(&mysql_url).unwrap();
        let password: String =
            url::form_urlencoded::parse(format!("p={}", uri.password().unwrap_or_default()).as_bytes())
                .next()
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default();
        let database = uri.path().trim_start_matches('/').to_string();
        let created = admin
            .post(
                "/api/admin/db-manager/connections",
                json!({
                    "name": "MySQL",
                    "db_type": "mysql",
                    "host": uri.host_str().unwrap(),
                    "port": uri.port().unwrap_or(3306),
                    "username": uri.username(),
                    "password": password,
                    "database_name": database,
                }),
            )
            .await;
        let base = format!("/api/admin/db-manager/connections/{}", created["id"]);
        let connected = admin.post(&format!("{base}/connect"), json!({})).await;
        assert_eq!(connected["has_schemas"], false);
        assert!(connected["databases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == database.as_str()));

        let setup = admin
            .post(
                &format!("{base}/execute"),
                json!({
                    "database": database,
                    "session_key": "mysql-tab",
                    "sql": "DROP TABLE IF EXISTS db_manager_probe;\n\
                            CREATE TABLE db_manager_probe (id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(50) NOT NULL COMMENT '名字', amount DECIMAL(10,2), big BIGINT UNSIGNED, born DATE, flag BIT(1), data BLOB) COMMENT '探针';\n\
                            INSERT INTO db_manager_probe (name, amount, big, born, flag, data) VALUES ('a', 1.50, 18446744073709551615, '2026-01-02', b'1', x'CAFE'), ('b', NULL, 1, NULL, b'0', NULL);\n\
                            DELIMITER //\n\
                            SELECT 1 AS one //\n\
                            DELIMITER ;\n\
                            SELECT * FROM db_manager_probe ORDER BY id"
                }),
            )
            .await;
        let statements = setup["statements"].as_array().unwrap();
        assert!(statements.iter().all(|statement| statement["error"].is_null()), "{setup}");
        let rows = &statements[4]["rows"];
        assert_eq!(rows[0], json!([1, "a", "1.50", "18446744073709551615", "2026-01-02", 1, "0xcafe"]));
        assert_eq!(rows[1][2], Value::Null);

        let objects = admin.get(&format!("{base}/objects?database={database}")).await;
        assert!(objects
            .as_array()
            .unwrap()
            .iter()
            .any(|object| object["name"] == "db_manager_probe" && object["comment"] == "探针"));
        let structure = admin
            .get(&format!("{base}/structure?database={database}&table=db_manager_probe"))
            .await;
        assert_eq!(structure["primary_key"], json!(["id"]));
        assert_eq!(structure["columns"][0]["auto_increment"], true);
        assert_eq!(structure["columns"][1]["comment"], "名字");
        assert!(structure["ddl"].as_str().unwrap().starts_with("CREATE TABLE `db_manager_probe`"));

        let summary = admin
            .post(
                &format!("{base}/changes"),
                json!({
                    "database": database,
                    "table": "db_manager_probe",
                    "changes": [
                        { "kind": "update", "key": { "id": 2 }, "values": { "name": "b", "amount": "3.25" } },
                        { "kind": "insert", "values": { "name": "c" } },
                        { "kind": "delete", "key": { "id": 1 } }
                    ]
                }),
            )
            .await;
        assert_eq!(summary, json!({ "inserted": 1, "updated": 1, "deleted": 1 }));
        let filters = encode(r#"[{"column":"amount","op":"gt","value":"3"}]"#);
        let page = admin
            .get(&format!(
                "{base}/rows?database={database}&table=db_manager_probe&filters={filters}"
            ))
            .await;
        assert_eq!(page["total"], 1);
        assert_eq!(page["rows"][0][1], "b");

        let transaction = admin
            .post(
                &format!("{base}/execute"),
                json!({ "database": database, "session_key": "mysql-tab", "sql": "START TRANSACTION; DELETE FROM db_manager_probe" }),
            )
            .await;
        assert_eq!(transaction["in_transaction"], true);
        let rolled_back = admin
            .post(
                &format!("{base}/execute"),
                json!({ "database": database, "session_key": "mysql-tab", "sql": "ROLLBACK; SELECT COUNT(*) FROM db_manager_probe; DROP TABLE db_manager_probe" }),
            )
            .await;
        assert_eq!(rolled_back["in_transaction"], false);
        assert_eq!(rolled_back["statements"][1]["rows"][0][0], 2);
        admin.post(&format!("{base}/disconnect"), json!({})).await;
    })
    .await;
}
