#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
};

use chrono::Local;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, Set,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{broadcast, oneshot},
    time::{sleep, Duration},
};

use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::{command_run_logs, command_runs},
};

const OUTPUT_TAIL_LIMIT: usize = 20_000;
const DEFAULT_COMMAND_PATH: &str =
    "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/local/sbin:/usr/bin:/bin:/usr/sbin:/sbin";

static RUN_CHANNELS: OnceLock<Mutex<HashMap<i32, broadcast::Sender<CommandLogEvent>>>> =
    OnceLock::new();
static RUN_CANCELLERS: OnceLock<Mutex<HashMap<i32, oneshot::Sender<()>>>> = OnceLock::new();
static RUN_CANCELLED: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct CommandLogEvent {
    pub run_id: i32,
    pub seq: i32,
    pub stream: String,
    pub chunk: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct CommandRunRequest {
    pub template_id: Option<i32>,
    pub name: String,
    pub working_directory: String,
    pub command_line: String,
    pub setup_script: Option<String>,
    pub python_venv_path: Option<String>,
    pub env_vars: Option<String>,
    pub timeout_seconds: Option<i32>,
    pub preview_path_template: Option<String>,
    pub triggered_by: String,
    pub created_by: Option<i32>,
}

struct CommandBuildResult {
    effective_script: String,
    preview_path_template: Option<String>,
    preview_path: Option<String>,
}

enum CommandWaitResult {
    Exited(Option<i32>),
    TimedOut(i32),
    Cancelled,
}

#[derive(Clone, Copy)]
enum FinishStatus {
    Success,
    Failed,
    Cancelled,
}

pub async fn start_command_run(
    db: DatabaseConnection,
    request: CommandRunRequest,
) -> ApiResult<command_runs::Model> {
    let working_directory = validate_directory(&request.working_directory)?;
    if let Some(venv_path) = request
        .python_venv_path
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        validate_venv_path(venv_path)?;
    }
    validate_json_object(request.env_vars.as_deref(), "env_vars")?;
    let build_result = build_command_run(&request, &working_directory)?;
    let run = command_runs::ActiveModel {
        template_id: Set(request.template_id),
        name: Set(request.name),
        working_directory: Set(working_directory.display().to_string()),
        command_line: Set(request.command_line),
        effective_script: Set(build_result.effective_script.clone()),
        status: Set("queued".to_string()),
        exit_code: Set(None),
        started_at: Set(None),
        finished_at: Set(None),
        duration_ms: Set(None),
        triggered_by: Set(request.triggered_by),
        created_by: Set(request.created_by),
        error_message: Set(None),
        output_tail: Set(None),
        preview_path_template: Set(build_result.preview_path_template),
        preview_path: Set(build_result.preview_path),
        ..Default::default()
    }
    .insert(&db)
    .await?;

    register_channel(run.id);
    let cancel_rx = register_canceller(run.id)?;
    tokio::spawn(run_command_task(
        db,
        run.id,
        build_result.effective_script,
        request.timeout_seconds,
        cancel_rx,
    ));
    Ok(run)
}

#[must_use]
pub fn subscribe_run(run_id: i32) -> broadcast::Receiver<CommandLogEvent> {
    register_channel(run_id).subscribe()
}

pub async fn cancel_run(db: &DatabaseConnection, run_id: i32) -> ApiResult<command_runs::Model> {
    let run = find_run(db, run_id).await?;
    if !matches!(run.status.as_str(), "queued" | "running") {
        return Ok(run);
    }
    mark_cancelled(run_id)?;
    if let Some(canceller) = take_canceller(run_id)? {
        let _ = canceller.send(());
        append_system_log(db, run_id, "命令已请求取消\n").await?;
    }
    let mut active = run.into_active_model();
    active.status = Set("cancelled".to_string());
    active.error_message = Set(Some("cancelled".to_string()));
    Ok(active.update(db).await?)
}

pub async fn mark_stale_runs_failed(db: &DatabaseConnection) -> ApiResult<()> {
    let runs = command_runs::Entity::find()
        .filter(command_runs::Column::Status.is_in(["queued", "running"]))
        .all(db)
        .await?;
    for run in runs {
        if has_canceller(run.id) {
            continue;
        }
        let _ = finish_run_with_status(
            db,
            run.id,
            FinishStatus::Failed,
            None,
            Some("backend restarted before command finished".to_string()),
        )
        .await;
    }
    Ok(())
}

async fn run_command_task(
    db: DatabaseConnection,
    run_id: i32,
    effective_script: String,
    timeout_seconds: Option<i32>,
    cancel_rx: oneshot::Receiver<()>,
) {
    let started_at = Local::now();
    if let Err(error) = mark_running(&db, run_id, started_at.into()).await {
        tracing::error!(
            run_id,
            error = error.message(),
            "failed to mark command run running"
        );
        return;
    }
    if let Err(error) = append_system_log(&db, run_id, "命令开始执行\n").await {
        tracing::error!(
            run_id,
            error = error.message(),
            "failed to append command start log"
        );
    }

    let mut child = match Command::new("/bin/bash")
        .arg("-lc")
        .arg(effective_script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = finish_run_with_status(
                &db,
                run_id,
                FinishStatus::Failed,
                None,
                Some(format!("failed to spawn command: {error}")),
            )
            .await;
            return;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_task =
        stdout.map(|stdout| tokio::spawn(read_stream(db.clone(), run_id, "stdout", stdout)));
    let stderr_task =
        stderr.map(|stderr| tokio::spawn(read_stream(db.clone(), run_id, "stderr", stderr)));

    let status_result = wait_for_child(child, timeout_seconds, cancel_rx).await;
    if let Some(task) = stdout_task {
        let _ = task.await;
    }
    if let Some(task) = stderr_task {
        let _ = task.await;
    }

    match status_result {
        Ok(CommandWaitResult::Exited(Some(code))) => {
            let status = if code == 0 {
                FinishStatus::Success
            } else {
                FinishStatus::Failed
            };
            let error_message = (code != 0).then(|| format!("command exited with code {code}"));
            let _ = finish_run_with_status(&db, run_id, status, Some(code), error_message).await;
        }
        Ok(CommandWaitResult::Exited(None)) => {
            let _ = finish_run_with_status(
                &db,
                run_id,
                FinishStatus::Failed,
                None,
                Some("command terminated by signal".to_string()),
            )
            .await;
        }
        Ok(CommandWaitResult::TimedOut(seconds)) => {
            let message = format!("command timed out after {seconds} seconds");
            let _ = append_system_log(&db, run_id, &format!("{message}\n")).await;
            let _ = finish_run_with_status(&db, run_id, FinishStatus::Failed, None, Some(message))
                .await;
        }
        Ok(CommandWaitResult::Cancelled) => {
            let _ = append_system_log(&db, run_id, "命令已取消\n").await;
            let _ = finish_run_with_status(
                &db,
                run_id,
                FinishStatus::Cancelled,
                None,
                Some("cancelled".to_string()),
            )
            .await;
        }
        Err(error) => {
            let _ = append_system_log(&db, run_id, &format!("{error}\n")).await;
            let _ =
                finish_run_with_status(&db, run_id, FinishStatus::Failed, None, Some(error)).await;
        }
    }
}

async fn wait_for_child(
    mut child: Child,
    timeout_seconds: Option<i32>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> Result<CommandWaitResult, String> {
    if let Some(seconds) = timeout_seconds.filter(|value| *value > 0) {
        let timeout = Duration::from_secs(u64::try_from(seconds).unwrap_or(0));
        tokio::select! {
            result = child.wait() => result
                .map(|status| CommandWaitResult::Exited(status.code()))
                .map_err(|error| format!("failed to wait for command: {error}")),
            () = sleep(timeout) => {
                let _ = child.kill().await;
                Ok(CommandWaitResult::TimedOut(seconds))
            }
            _ = &mut cancel_rx => {
                let _ = child.kill().await;
                Ok(CommandWaitResult::Cancelled)
            }
        }
    } else {
        tokio::select! {
            result = child.wait() => result
                .map(|status| CommandWaitResult::Exited(status.code()))
                .map_err(|error| format!("failed to wait for command: {error}")),
            _ = &mut cancel_rx => {
                let _ = child.kill().await;
                Ok(CommandWaitResult::Cancelled)
            }
        }
    }
}

async fn read_stream<T>(db: DatabaseConnection, run_id: i32, stream: &'static str, reader: T)
where
    T: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let chunk = format!("{line}\n");
                if let Err(error) = append_log(&db, run_id, stream, &chunk).await {
                    tracing::error!(
                        run_id,
                        stream,
                        error = error.message(),
                        "failed to append command log"
                    );
                    break;
                }
            }
            Ok(None) => break,
            Err(error) => {
                tracing::error!(
                    run_id,
                    stream,
                    error = error.to_string(),
                    "failed to read command stream"
                );
                break;
            }
        }
    }
}

async fn mark_running(
    db: &DatabaseConnection,
    run_id: i32,
    started_at: chrono::DateTime<chrono::FixedOffset>,
) -> ApiResult<command_runs::Model> {
    let run = find_run(db, run_id).await?;
    let mut active = run.into_active_model();
    active.status = Set("running".to_string());
    active.started_at = Set(Some(started_at));
    Ok(active.update(db).await?)
}

async fn finish_run_with_status(
    db: &DatabaseConnection,
    run_id: i32,
    status: FinishStatus,
    exit_code: Option<i32>,
    error_message: Option<String>,
) -> ApiResult<command_runs::Model> {
    let run = find_run(db, run_id).await?;
    let status = if is_cancelled(run_id) || run.status == "cancelled" {
        FinishStatus::Cancelled
    } else {
        status
    };
    let status = match status {
        FinishStatus::Success => "success",
        FinishStatus::Failed => "failed",
        FinishStatus::Cancelled => "cancelled",
    };
    finish_run(db, run, status, exit_code, error_message).await
}

async fn finish_run(
    db: &DatabaseConnection,
    run: command_runs::Model,
    status: &str,
    exit_code: Option<i32>,
    error_message: Option<String>,
) -> ApiResult<command_runs::Model> {
    let finished_at = Local::now();
    let duration_ms = run.started_at.map(|started_at| {
        i32::try_from((finished_at.fixed_offset() - started_at).num_milliseconds()).unwrap_or(0)
    });
    let run_id = run.id;
    let mut active = run.into_active_model();
    active.status = Set(status.to_string());
    active.exit_code = Set(exit_code);
    active.finished_at = Set(Some(finished_at.into()));
    active.duration_ms = Set(duration_ms);
    active.error_message = Set(error_message);
    let run = active.update(db).await?;
    drop_canceller(run_id)?;
    clear_cancelled(run_id)?;
    remove_channel_if_finished(run_id);
    Ok(run)
}

async fn append_system_log(db: &DatabaseConnection, run_id: i32, chunk: &str) -> ApiResult<()> {
    append_log(db, run_id, "system", chunk).await
}

async fn append_log(
    db: &DatabaseConnection,
    run_id: i32,
    stream: &str,
    chunk: &str,
) -> ApiResult<()> {
    let seq = next_seq(db, run_id).await?;
    let log = command_run_logs::ActiveModel {
        run_id: Set(run_id),
        seq: Set(seq),
        stream: Set(stream.to_string()),
        chunk: Set(chunk.to_string()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    update_tail(db, run_id, chunk).await?;
    let event = CommandLogEvent {
        run_id,
        seq,
        stream: log.stream,
        chunk: log.chunk,
        created_at: log.created_at.to_rfc3339(),
    };
    let _ = register_channel(run_id).send(event);
    Ok(())
}

async fn next_seq(db: &DatabaseConnection, run_id: i32) -> ApiResult<i32> {
    let last = command_run_logs::Entity::find()
        .filter(command_run_logs::Column::RunId.eq(run_id))
        .order_by_desc(command_run_logs::Column::Seq)
        .one(db)
        .await?;
    Ok(last.map_or(1, |log| log.seq + 1))
}

async fn update_tail(db: &DatabaseConnection, run_id: i32, chunk: &str) -> ApiResult<()> {
    let run = find_run(db, run_id).await?;
    let mut tail = run.output_tail.clone().unwrap_or_default();
    tail.push_str(chunk);
    if tail.len() > OUTPUT_TAIL_LIMIT {
        let keep_from = tail.len() - OUTPUT_TAIL_LIMIT;
        tail = tail[keep_from..].to_string();
    }
    let mut active = run.into_active_model();
    active.output_tail = Set(Some(tail));
    active.update(db).await?;
    Ok(())
}

async fn find_run(db: &DatabaseConnection, run_id: i32) -> ApiResult<command_runs::Model> {
    command_runs::Entity::find_by_id(run_id)
        .one(db)
        .await?
        .ok_or_else(|| ApiError::bad_request("command run not found"))
}

fn build_command_run(request: &CommandRunRequest, cwd: &Path) -> ApiResult<CommandBuildResult> {
    let mut resolver = RandomPlaceholderResolver::default();
    let mut lines = vec![
        format!("export PATH={}:$PATH", shell_quote(DEFAULT_COMMAND_PATH)),
        format!("cd {}", shell_quote(&cwd.display().to_string())),
    ];
    let env_vars = request
        .env_vars
        .as_deref()
        .map(|value| resolver.replace(value));
    if let Some(env_vars) = parse_env_vars(env_vars.as_deref())? {
        for (key, value) in env_vars {
            validate_env_key(&key)?;
            lines.push(format!("export {key}={}", shell_quote(&value)));
        }
    }
    if let Some(venv_path) = request
        .python_venv_path
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let activate = Path::new(venv_path).join("bin/activate");
        lines.push(format!(
            "source {}",
            shell_quote(&activate.display().to_string())
        ));
    }
    if let Some(setup_script) = request
        .setup_script
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(resolver.replace(setup_script));
    }
    lines.push(resolver.replace(&request.command_line));

    let preview_path_template = normalize_optional_text(request.preview_path_template.clone());
    let preview_path = preview_path_template
        .as_deref()
        .map(|value| resolve_preview_path(cwd, &resolver.replace(value)))
        .transpose()?;

    Ok(CommandBuildResult {
        effective_script: lines.join("\n"),
        preview_path_template,
        preview_path,
    })
}

fn resolve_preview_path(cwd: &Path, value: &str) -> ApiResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request("preview path is required"));
    }
    let path = PathBuf::from(trimmed);
    validate_safe_path(&path)?;
    let resolved = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let parent = resolved
        .parent()
        .ok_or_else(|| ApiError::bad_request("invalid preview path"))?;
    if parent.exists() {
        let canonical_parent = parent
            .canonicalize()
            .map_err(|_| ApiError::bad_request("preview path parent not found"))?;
        let name = resolved
            .file_name()
            .ok_or_else(|| ApiError::bad_request("invalid preview path"))?;
        Ok(canonical_parent.join(name).display().to_string())
    } else {
        Ok(resolved.display().to_string())
    }
}

fn validate_safe_path(path: &Path) -> ApiResult<()> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(ApiError::bad_request(
            "preview path must not contain traversal",
        ));
    }
    Ok(())
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn validate_directory(value: &str) -> ApiResult<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request("working directory is required"));
    }
    let path = PathBuf::from(trimmed);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|_| ApiError::internal("failed to resolve current directory"))?
            .join(path)
    };
    if !path.exists() || !path.is_dir() {
        return Err(ApiError::bad_request("working directory does not exist"));
    }
    Ok(path)
}

fn validate_venv_path(value: &str) -> ApiResult<()> {
    let activate = Path::new(value).join("bin/activate");
    if !activate.exists() || !activate.is_file() {
        return Err(ApiError::bad_request(
            "python venv activate script not found",
        ));
    }
    Ok(())
}

fn validate_json_object(value: Option<&str>, field: &str) -> ApiResult<()> {
    if let Some(raw) = value.filter(|value| !value.trim().is_empty()) {
        let parsed = serde_json::from_str::<serde_json::Value>(raw)
            .map_err(|_| ApiError::bad_request(format!("{field} must be JSON")))?;
        if !parsed.is_object() {
            return Err(ApiError::bad_request(format!(
                "{field} must be a JSON object"
            )));
        }
    }
    Ok(())
}

fn parse_env_vars(value: Option<&str>) -> ApiResult<Option<Vec<(String, String)>>> {
    let Some(raw) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let parsed = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(raw)
        .map_err(|_| ApiError::bad_request("env_vars must be a JSON object"))?;
    Ok(Some(
        parsed
            .into_iter()
            .map(|(key, value)| {
                let value = value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_string);
                (key, value)
            })
            .collect(),
    ))
}

fn validate_env_key(key: &str) -> ApiResult<()> {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return Err(ApiError::bad_request("env var key is required"));
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(ApiError::bad_request("invalid env var key"));
    }
    if chars.any(|char| !(char == '_' || char.is_ascii_alphanumeric())) {
        return Err(ApiError::bad_request("invalid env var key"));
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Default)]
struct RandomPlaceholderResolver {
    values: HashMap<String, String>,
}

impl RandomPlaceholderResolver {
    fn replace(&mut self, value: &str) -> String {
        let mut output = String::with_capacity(value.len());
        let mut remaining = value;
        while let Some(start) = remaining.find("{{random") {
            output.push_str(&remaining[..start]);
            let after_start = &remaining[start..];
            let Some(end) = after_start.find("}}") else {
                output.push_str(after_start);
                return output;
            };
            let token = &after_start[..end + 2];
            let random = self
                .values
                .entry(token.to_string())
                .or_insert_with(|| random_value(token));
            output.push_str(random);
            remaining = &after_start[end + 2..];
        }
        output.push_str(remaining);
        output
    }
}

fn random_value(token: &str) -> String {
    let digits = token
        .strip_prefix("{{random:")
        .and_then(|value| value.strip_suffix("}}"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8)
        .clamp(1, 18);
    let mut number = uuid::Uuid::new_v4().as_u128();
    let mut value = String::with_capacity(digits);
    for _ in 0..digits {
        value.push(char::from(b'0' + u8::try_from(number % 10).unwrap_or(0)));
        number /= 10;
    }
    value
}

fn channels() -> &'static Mutex<HashMap<i32, broadcast::Sender<CommandLogEvent>>> {
    RUN_CHANNELS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cancellers() -> &'static Mutex<HashMap<i32, oneshot::Sender<()>>> {
    RUN_CANCELLERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cancelled_runs() -> &'static Mutex<HashSet<i32>> {
    RUN_CANCELLED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn register_channel(run_id: i32) -> broadcast::Sender<CommandLogEvent> {
    let mut channels = channels().lock().expect("command channel lock poisoned");
    channels
        .entry(run_id)
        .or_insert_with(|| broadcast::channel(500).0)
        .clone()
}

fn remove_channel_if_finished(run_id: i32) {
    if let Ok(mut channels) = channels().lock() {
        channels.remove(&run_id);
    }
}

fn register_canceller(run_id: i32) -> ApiResult<oneshot::Receiver<()>> {
    let (sender, receiver) = oneshot::channel();
    cancellers()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command cancel registry"))?
        .insert(run_id, sender);
    Ok(receiver)
}

fn take_canceller(run_id: i32) -> ApiResult<Option<oneshot::Sender<()>>> {
    Ok(cancellers()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command cancel registry"))?
        .remove(&run_id))
}

fn has_canceller(run_id: i32) -> bool {
    cancellers()
        .lock()
        .is_ok_and(|cancellers| cancellers.contains_key(&run_id))
}

fn drop_canceller(run_id: i32) -> ApiResult<()> {
    cancellers()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command cancel registry"))?
        .remove(&run_id);
    Ok(())
}

fn mark_cancelled(run_id: i32) -> ApiResult<()> {
    cancelled_runs()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command cancel state"))?
        .insert(run_id);
    Ok(())
}

fn is_cancelled(run_id: i32) -> bool {
    cancelled_runs()
        .lock()
        .is_ok_and(|cancelled_runs| cancelled_runs.contains(&run_id))
}

fn clear_cancelled(run_id: i32) -> ApiResult<()> {
    cancelled_runs()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command cancel state"))?
        .remove(&run_id);
    Ok(())
}
