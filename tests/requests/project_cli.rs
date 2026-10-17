use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use gpt_images::{
    app::App,
    models::_entities::{project_cli_run_logs, project_cli_runs, role_permissions, user_roles},
    responses::ApiResponse,
    services::project_cli::{self, ProcessSpec, ProjectCliRunStatus},
    views::auth::LoginResponse,
};
use loco_rs::{testing::prelude::*, TestServer};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde_json::{json, Value};
use serial_test::serial;

use super::prepare_data;

const CATALOG_PATH: &str = "/api/admin/project-cli";
const RUNS_PATH: &str = "/api/admin/project-cli/runs";
const VIEW_PERMISSION_ID: i32 = 158;
const OPERATOR_ROLE_ID: i32 = 2;

static LOGIN_IP_COUNTER: AtomicUsize = AtomicUsize::new(10);

async fn login_admin(request: &TestServer) -> String {
    let ip_suffix = LOGIN_IP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let response = request
        .post("/api/auth/login")
        .add_header("x-forwarded-for", format!("127.0.7.{ip_suffix}"))
        .json(&json!({ "email": "admin@example.com", "password": "1234" }))
        .await;
    assert_eq!(response.status_code(), 200, "{}", response.text());
    let login: ApiResponse<LoginResponse> = serde_json::from_str(&response.text()).unwrap();
    login.data.unwrap().token
}

async fn get_json(request: &TestServer, token: &str, path: &str) -> (u16, Value) {
    let (key, value) = prepare_data::auth_header(token);
    let response = request.get(path).add_header(key, value).await;
    (
        response.status_code().as_u16(),
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

async fn post_json(request: &TestServer, token: &str, path: &str, body: &Value) -> (u16, Value) {
    let (key, value) = prepare_data::auth_header(token);
    let response = request.post(path).add_header(key, value).json(body).await;
    (
        response.status_code().as_u16(),
        serde_json::from_str(&response.text()).unwrap_or(Value::Null),
    )
}

fn find_command<'a>(catalog: &'a Value, path: &[&str]) -> &'a Value {
    catalog["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["path"] == json!(path))
        .unwrap_or_else(|| panic!("command {path:?} missing from catalog"))
}

/// Polls a run until it finishes and returns its full output.
async fn wait_for_run(request: &TestServer, token: &str, id: u64) -> (Value, String) {
    let mut output = String::new();
    let mut after = 0;
    for _ in 0..300 {
        let (status, body) =
            get_json(request, token, &format!("{RUNS_PATH}/{id}?after={after}")).await;
        assert_eq!(status, 200, "{body}");
        for chunk in body["data"]["output"].as_array().unwrap() {
            output.push_str(chunk["text"].as_str().unwrap());
            after = chunk["seq"].as_u64().unwrap();
        }
        if body["data"]["run"]["status"] != "running" {
            return (body["data"]["run"].clone(), output);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("run {id} did not finish; output so far:\n{output}");
}

#[tokio::test]
#[serial]
async fn project_cli_requires_auth() {
    request::<App, _, _>(|request, _ctx| async move {
        assert_eq!(request.get(CATALOG_PATH).await.status_code(), 401);
        assert_eq!(
            request
                .post(RUNS_PATH)
                .json(&json!({ "path": ["version"] }))
                .await
                .status_code(),
            401
        );
    })
    .await;
}

#[tokio::test]
#[serial]
async fn project_cli_lists_every_command_source() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_admin(&request).await;

        let (status, body) = get_json(&request, &token, CATALOG_PATH).await;
        assert_eq!(status, 200, "{body}");
        let catalog = &body["data"];
        assert_eq!(catalog["warnings"], json!([]), "{catalog}");
        assert_eq!(catalog["environment"], "test");

        // Loco's built-in commands, read from the binary's help output.
        let start = find_command(catalog, &["start"]);
        assert_eq!(start["long_running"], true);
        assert!(start["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["long"] == "port" && arg["kind"] == "option"));
        assert_eq!(find_command(catalog, &["db"])["runnable"], false);
        assert_eq!(find_command(catalog, &["db", "reset"])["risk"], "danger");
        find_command(catalog, &["db", "migrate"]);
        // Custom commands, from their clap definitions.
        let install = find_command(catalog, &["service", "install"]);
        assert_eq!(install["source"], "service");
        find_command(catalog, &["data", "export"]);
        find_command(catalog, &["vnc", "status"]);
        // Registered tasks with their documented parameters.
        let recover = find_command(catalog, &["task", "admin.recover"]);
        let password = recover["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|arg| arg["id"] == "password")
            .unwrap();
        assert_eq!(password["kind"], "task_param");
        assert_eq!(password["secret"], true);
        assert!(catalog["commands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| command["path"].as_array().unwrap().last().unwrap() != "help"));
    })
    .await;
}

#[tokio::test]
#[serial]
async fn super_admin_can_run_a_project_command() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_admin(&request).await;

        let (status, body) =
            post_json(&request, &token, RUNS_PATH, &json!({ "path": ["version"] })).await;
        assert_eq!(status, 200, "{body}");
        let id = body["data"]["id"].as_u64().unwrap();
        assert!(body["data"]["command_line"]
            .as_str()
            .unwrap()
            .ends_with(" version"));

        let (run, output) = wait_for_run(&request, &token, id).await;
        assert_eq!(run["status"], "success", "{output}");
        assert_eq!(run["exit_code"], 0);
        assert!(output.contains(env!("CARGO_PKG_VERSION")), "{output}");

        let (status, body) = get_json(&request, &token, RUNS_PATH).await;
        assert_eq!(status, 200);
        assert_eq!(body["data"]["total"], 1);
        assert_eq!(body["data"]["items"][0]["id"], id);
        assert_eq!(
            body["data"]["items"][0]["created_by_name"]
                .as_str()
                .unwrap()
                .is_empty(),
            false
        );

        // Bad input is rejected before anything runs.
        for (payload, reason) in [
            (json!({ "path": ["no-such-command"] }), "unknown command"),
            (json!({ "path": ["db"] }), "group without a subcommand"),
            (
                json!({ "path": ["task", "admin.recover"], "values": { "email": "a@b.c" } }),
                "missing required password",
            ),
            (
                json!({ "path": ["version"], "values": { "bogus": true } }),
                "unknown argument",
            ),
        ] {
            let (status, body) = post_json(&request, &token, RUNS_PATH, &payload).await;
            assert_eq!(status, 400, "{reason}: {body}");
        }
    })
    .await;
}

#[tokio::test]
#[serial]
async fn viewing_project_commands_does_not_allow_running_them() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let user = prepare_data::init_user_login(&request, &ctx).await;
        let (status, _) = get_json(&request, &user.token, CATALOG_PATH).await;
        assert_eq!(status, 403);

        let now = chrono::Local::now();
        role_permissions::ActiveModel {
            role_id: Set(OPERATOR_ROLE_ID),
            permission_id: Set(VIEW_PERMISSION_ID),
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

        let (status, _) = get_json(&request, &user.token, CATALOG_PATH).await;
        assert_eq!(status, 200);
        let (status, _) = get_json(&request, &user.token, RUNS_PATH).await;
        assert_eq!(status, 200);
        let (status, _) = post_json(
            &request,
            &user.token,
            RUNS_PATH,
            &json!({ "path": ["version"] }),
        )
        .await;
        assert_eq!(status, 403);
        let (status, _) = post_json(
            &request,
            &user.token,
            &format!("{RUNS_PATH}/1/cancel"),
            &json!({}),
        )
        .await;
        assert_eq!(status, 403);
    })
    .await;
}

fn shell(script: &str) -> ProcessSpec {
    ProcessSpec {
        path: vec!["test".into()],
        binary: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        display_args: vec!["-c".into(), script.into()],
        display_program: "sh".into(),
        environment: "test".into(),
        working_directory: std::env::temp_dir(),
    }
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn runs_and_their_output_are_stored() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_admin(&request).await;

        let run = project_cli::spawn_run(&ctx.db, shell("echo hello; echo oops >&2; exit 3"), None)
            .await
            .unwrap();
        let (finished, output) =
            wait_for_run(&request, &token, u64::try_from(run.id).unwrap()).await;
        assert_eq!(finished["status"], "failed");
        assert_eq!(finished["exit_code"], 3);
        assert!(
            output.contains("hello") && output.contains("oops"),
            "{output}"
        );
        assert!(output.contains("退出码 3"), "{output}");

        let stored = project_cli_run_logs::Entity::find()
            .filter(project_cli_run_logs::Column::RunId.eq(run.id))
            .all(&ctx.db)
            .await
            .unwrap();
        assert!(stored
            .iter()
            .any(|row| row.stream == "stderr" && row.text.contains("oops")));
        let row = project_cli_runs::Entity::find_by_id(run.id)
            .one(&ctx.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "failed");
        assert!(row.finished_at.is_some() && row.duration_ms.is_some());
        assert_eq!(
            row.last_seq,
            stored.iter().map(|row| row.seq).max().unwrap()
        );

        // Stopping a long-running command.
        let run = project_cli::spawn_run(&ctx.db, shell("echo started; sleep 30"), None)
            .await
            .unwrap();
        let (key, value) = prepare_data::auth_header(&token);
        let response = request
            .post(&format!("{RUNS_PATH}/{}/cancel", run.id))
            .add_header(key, value)
            .await;
        assert_eq!(response.status_code(), 200);
        let (stopped, output) =
            wait_for_run(&request, &token, u64::try_from(run.id).unwrap()).await;
        assert_eq!(stopped["status"], "cancelled", "{output}");
        assert!(output.contains("已停止"), "{output}");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn runs_interrupted_by_a_restart_are_marked_failed() {
    request::<App, _, _>(|request, ctx| async move {
        seed::<App>(&ctx).await.unwrap();
        let token = login_admin(&request).await;
        let now = chrono::Local::now().fixed_offset();
        // A `running` row whose process no longer exists, as after a restart.
        let orphan = project_cli_runs::ActiveModel {
            path: Set(r#"["version"]"#.into()),
            args: Set(r#"["version"]"#.into()),
            command_line: Set("gpt_images-cli version".into()),
            environment: Set("test".into()),
            status: Set("running".into()),
            started_at: Set(now),
            created_by_name: Set(String::new()),
            last_seq: Set(0),
            output_bytes: Set(0),
            truncated: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&ctx.db)
        .await
        .unwrap();

        let (status, body) = get_json(&request, &token, RUNS_PATH).await;
        assert_eq!(status, 200);
        let item = &body["data"]["items"][0];
        assert_eq!(item["id"], orphan.id);
        assert_eq!(item["status"], "failed");
        assert!(item["error"].as_str().unwrap().contains("服务重启"));
        assert_eq!(
            project_cli::run_detail(&ctx.db, orphan.id, 0)
                .await
                .unwrap()
                .run
                .status,
            ProjectCliRunStatus::Failed
        );
    })
    .await;
}
