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
static RUN_CANCELLERS: OnceLock<Mutex<HashMap<i32, oneshot::Sender<CancelRequest>>>> =
    OnceLock::new();
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
    pub retry_policy: RetryPolicy,
    pub preview_path_template: Option<String>,
    pub triggered_by: String,
    pub created_by: Option<i32>,
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub enabled: bool,
    pub max_retries: i32,
    pub delay_seconds: i32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_retries: 3,
            delay_seconds: 5,
        }
    }
}

impl RetryPolicy {
    pub fn from_options(
        enabled: Option<bool>,
        max_retries: Option<i32>,
        delay_seconds: Option<i32>,
    ) -> ApiResult<Self> {
        let policy = Self {
            enabled: enabled.unwrap_or(false),
            max_retries: max_retries.unwrap_or(3),
            delay_seconds: delay_seconds.unwrap_or(5),
        };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(self) -> ApiResult<()> {
        if !(1..=10).contains(&self.max_retries) {
            return Err(ApiError::bad_request(
                "max_retries must be between 1 and 10",
            ));
        }
        if !(1..=3600).contains(&self.delay_seconds) {
            return Err(ApiError::bad_request(
                "retry_delay_seconds must be between 1 and 3600",
            ));
        }
        Ok(())
    }

    const fn should_retry(self, result: &Result<CommandWaitResult, String>, attempt: i32) -> bool {
        self.enabled
            && attempt <= self.max_retries
            && match result {
                Ok(CommandWaitResult::Exited(Some(code))) => *code != 0,
                Ok(CommandWaitResult::Exited(None)) => true,
                _ => false,
            }
    }
}

struct CommandBuildResult {
    effective_script: String,
    preview_path_template: Option<String>,
    preview_path: Option<String>,
}

struct CancelRequest {
    completion: oneshot::Sender<Result<(), String>>,
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
    request.retry_policy.validate()?;
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
        retry_on_failure: Set(request.retry_policy.enabled),
        max_retries: Set(request.retry_policy.max_retries),
        retry_delay_seconds: Set(request.retry_policy.delay_seconds),
        attempt_count: Set(0),
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
        working_directory,
        build_result.effective_script,
        request.timeout_seconds,
        request.retry_policy,
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
    if !mark_cancelled(run_id)? {
        return wait_for_cancel_completion(db, run_id).await;
    }
    if let Some(canceller) = take_canceller(run_id)? {
        let (completion, completed) = oneshot::channel();
        if canceller.send(CancelRequest { completion }).is_ok() {
            match tokio::time::timeout(Duration::from_secs(5), completed).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(error))) => {
                    clear_cancelled(run_id)?;
                    return Err(ApiError::internal(error));
                }
                Ok(Err(_)) => {
                    let latest = find_run(db, run_id).await?;
                    if !matches!(latest.status.as_str(), "queued" | "running") {
                        clear_cancelled(run_id)?;
                        return Ok(latest);
                    }
                    return Err(ApiError::internal("command cancellation was not confirmed"));
                }
                Err(_) => return Err(ApiError::internal("command cancellation timed out")),
            }
        } else {
            clear_cancelled(run_id)?;
            return wait_for_cancel_completion(db, run_id).await;
        }
    } else {
        clear_cancelled(run_id)?;
        let latest = find_run(db, run_id).await?;
        if !matches!(latest.status.as_str(), "queued" | "running") {
            return Ok(latest);
        }
        return Err(ApiError::bad_request(
            "command has no active process in this backend",
        ));
    }
    let mut active = run.into_active_model();
    active.status = Set("cancelled".to_string());
    active.error_message = Set(Some("cancelled".to_string()));
    Ok(active.update(db).await?)
}

async fn wait_for_cancel_completion(
    db: &DatabaseConnection,
    run_id: i32,
) -> ApiResult<command_runs::Model> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let run = find_run(db, run_id).await?;
            if !matches!(run.status.as_str(), "queued" | "running") {
                return Ok(run);
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| ApiError::internal("command cancellation was not confirmed"))?
}

pub async fn mark_stale_runs_failed(db: &DatabaseConnection) -> ApiResult<()> {
    let runs = command_runs::Entity::find()
        .filter(command_runs::Column::Status.is_in(["queued", "running"]))
        .all(db)
        .await?;
    for run in runs {
        if has_canceller(run.id) || is_cancelled(run.id) {
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
    working_directory: PathBuf,
    effective_script: String,
    timeout_seconds: Option<i32>,
    retry_policy: RetryPolicy,
    mut cancel_rx: oneshot::Receiver<CancelRequest>,
) {
    if cancel_before_spawn(&db, run_id, &mut cancel_rx).await {
        return;
    }
    let started_at = Local::now();
    if let Err(error) = mark_running(&db, run_id, started_at.into()).await {
        tracing::error!(
            run_id,
            error = error.message(),
            "failed to mark command run running"
        );
        return;
    }
    let start_log = command_start_log(&working_directory);
    if let Err(error) = append_system_log(&db, run_id, &start_log).await {
        tracing::error!(
            run_id,
            error = error.message(),
            "failed to append command start log"
        );
    }

    let status_result = execute_with_retries(
        &db,
        run_id,
        &working_directory,
        &effective_script,
        timeout_seconds,
        retry_policy,
        &mut cancel_rx,
    )
    .await;
    finish_command_result(&db, run_id, status_result).await;
}

async fn execute_with_retries(
    db: &DatabaseConnection,
    run_id: i32,
    working_directory: &Path,
    effective_script: &str,
    timeout_seconds: Option<i32>,
    retry_policy: RetryPolicy,
    cancel_rx: &mut oneshot::Receiver<CancelRequest>,
) -> Result<CommandWaitResult, String> {
    let mut attempt = 1;
    loop {
        if take_pending_cancel(run_id, cancel_rx).await {
            return Ok(CommandWaitResult::Cancelled);
        }
        let run = find_run(db, run_id)
            .await
            .map_err(|error| error.message().to_string())?;
        let mut active = run.into_active_model();
        active.attempt_count = Set(attempt);
        active.exit_code = Set(None);
        active.error_message = Set(None);
        active.update(db).await.map_err(|error| error.to_string())?;
        let _ = append_system_log(db, run_id, &format!("开始第 {attempt} 次执行\n")).await;
        if take_pending_cancel(run_id, cancel_rx).await {
            return Ok(CommandWaitResult::Cancelled);
        }
        let result = execute_attempt(
            db,
            run_id,
            working_directory,
            effective_script,
            timeout_seconds,
            cancel_rx,
        )
        .await;
        if matches!(&result, Ok(CommandWaitResult::Cancelled))
            || take_pending_cancel(run_id, cancel_rx).await
        {
            return Ok(CommandWaitResult::Cancelled);
        }
        if !retry_policy.should_retry(&result, attempt) {
            return result;
        }
        let reason = match &result {
            Ok(CommandWaitResult::Exited(Some(code))) => format!("退出码 {code}"),
            _ => "进程被信号异常终止".to_string(),
        };
        let _ = append_system_log(
            db,
            run_id,
            &format!(
                "执行出错（{reason}），{} 秒后从头重新运行；重试 {attempt}/{}\n",
                retry_policy.delay_seconds, retry_policy.max_retries,
            ),
        )
        .await;
        tokio::select! {
            request = &mut *cancel_rx => {
                if let Ok(request) = request { let _ = request.completion.send(Ok(())); }
                return Ok(CommandWaitResult::Cancelled);
            }
            () = sleep(Duration::from_secs(u64::try_from(retry_policy.delay_seconds).unwrap_or(5))) => {}
        }
        attempt += 1;
    }
}

async fn execute_attempt(
    db: &DatabaseConnection,
    run_id: i32,
    working_directory: &Path,
    effective_script: &str,
    timeout_seconds: Option<i32>,
    cancel_rx: &mut oneshot::Receiver<CancelRequest>,
) -> Result<CommandWaitResult, String> {
    let mut child = shell_command(working_directory, effective_script.to_string())
        .spawn()
        .map_err(|error| {
            format!(
                "failed to spawn command in {}: {error}",
                working_directory.display()
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .map(|stdout| tokio::spawn(read_stream(db.clone(), run_id, "stdout", stdout)));
    let stderr = child
        .stderr
        .take()
        .map(|stderr| tokio::spawn(read_stream(db.clone(), run_id, "stderr", stderr)));
    wait_for_child(child, timeout_seconds, cancel_rx, stdout, stderr).await
}

async fn finish_command_result(
    db: &DatabaseConnection,
    run_id: i32,
    status_result: Result<CommandWaitResult, String>,
) {
    match status_result {
        Ok(CommandWaitResult::Exited(Some(code))) => {
            let status = if code == 0 {
                FinishStatus::Success
            } else {
                FinishStatus::Failed
            };
            let error_message = (code != 0).then(|| format!("command exited with code {code}"));
            let _ = finish_run_with_status(db, run_id, status, Some(code), error_message).await;
        }
        Ok(CommandWaitResult::Exited(None)) => {
            let _ = finish_run_with_status(
                db,
                run_id,
                FinishStatus::Failed,
                None,
                Some("command terminated by signal".to_string()),
            )
            .await;
        }
        Ok(CommandWaitResult::TimedOut(seconds)) => {
            let message = format!("command timed out after {seconds} seconds");
            let _ = append_system_log(db, run_id, &format!("{message}\n")).await;
            let _ =
                finish_run_with_status(db, run_id, FinishStatus::Failed, None, Some(message)).await;
        }
        Ok(CommandWaitResult::Cancelled) => {
            let _ = append_system_log(db, run_id, "命令已取消\n").await;
            let _ = finish_run_with_status(
                db,
                run_id,
                FinishStatus::Cancelled,
                None,
                Some("cancelled".to_string()),
            )
            .await;
        }
        Err(error) => {
            let _ = clear_cancelled(run_id);
            let _ = append_system_log(db, run_id, &format!("{error}\n")).await;
            let _ =
                finish_run_with_status(db, run_id, FinishStatus::Failed, None, Some(error)).await;
        }
    }
}

async fn cancel_before_spawn(
    db: &DatabaseConnection,
    run_id: i32,
    cancel_rx: &mut oneshot::Receiver<CancelRequest>,
) -> bool {
    if !take_pending_cancel(run_id, cancel_rx).await {
        return false;
    }
    let _ = finish_run_with_status(
        db,
        run_id,
        FinishStatus::Cancelled,
        None,
        Some("cancelled".to_string()),
    )
    .await;
    true
}

async fn take_pending_cancel(
    run_id: i32,
    cancel_rx: &mut oneshot::Receiver<CancelRequest>,
) -> bool {
    let request = match cancel_rx.try_recv() {
        Ok(request) => Some(request),
        Err(oneshot::error::TryRecvError::Empty) if is_cancelled(run_id) => cancel_rx.await.ok(),
        Err(oneshot::error::TryRecvError::Closed) => return is_cancelled(run_id),
        _ => return false,
    };
    if let Some(request) = request {
        let _ = request.completion.send(Ok(()));
    }
    true
}

fn shell_command(working_directory: &Path, effective_script: String) -> Command {
    let mut command = Command::new("/bin/bash");
    command
        .arg("-lc")
        .arg(effective_script)
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    command
}

fn command_start_log(working_directory: &Path) -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "<未设置>".to_string());
    let home = std::env::var("HOME").unwrap_or_else(|_| "<未设置>".to_string());
    format!(
        "命令开始执行\n工作目录: {}\n运行环境: USER={user} HOME={home}\n",
        working_directory.display()
    )
}

async fn wait_for_child(
    mut child: Child,
    timeout_seconds: Option<i32>,
    cancel_rx: &mut oneshot::Receiver<CancelRequest>,
    mut stdout_task: Option<tokio::task::JoinHandle<()>>,
    mut stderr_task: Option<tokio::task::JoinHandle<()>>,
) -> Result<CommandWaitResult, String> {
    let process_id = child.id();
    let timeout = async {
        if let Some(seconds) = timeout_seconds.filter(|value| *value > 0) {
            sleep(Duration::from_secs(u64::try_from(seconds).unwrap_or(0))).await;
            seconds
        } else {
            std::future::pending().await
        }
    };
    // Keep cancellation active while descendants still hold the output pipes,
    // even when the shell has already exited.
    let result = tokio::select! {
        result = async {
            let status = child.wait().await
                .map_err(|error| format!("failed to wait for command: {error}"))?;
            if !status.success() {
                terminate_process_tree(&mut child, process_id).await?;
            }
            drain_output(&mut stdout_task, &mut stderr_task).await;
            Ok(CommandWaitResult::Exited(status.code()))
        } => result,
        seconds = timeout => {
            terminate_process_tree(&mut child, process_id).await
                .map(|()| CommandWaitResult::TimedOut(seconds))
        }
        request = &mut *cancel_rx => {
            let result = terminate_process_tree(&mut child, process_id).await;
            if let Ok(request) = request {
                let _ = request.completion.send(result.clone());
            }
            result.map(|()| CommandWaitResult::Cancelled)
        }
    };
    if !matches!(&result, Ok(CommandWaitResult::Exited(_))) {
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            drain_output(&mut stdout_task, &mut stderr_task),
        )
        .await;
        for task in [stdout_task, stderr_task].into_iter().flatten() {
            task.abort();
        }
    }
    result
}

async fn drain_output(
    stdout_task: &mut Option<tokio::task::JoinHandle<()>>,
    stderr_task: &mut Option<tokio::task::JoinHandle<()>>,
) {
    for task in [stdout_task, stderr_task] {
        if let Some(handle) = task {
            let _ = handle.await;
            *task = None;
        }
    }
}

pub(crate) async fn terminate_process_tree(
    child: &mut Child,
    process_id: Option<u32>,
) -> Result<(), String> {
    #[cfg(unix)]
    if let Some(process_id) = process_id {
        let group = i32::try_from(process_id)
            .ok()
            .filter(|id| *id > 0)
            .ok_or_else(|| "invalid command process group".to_string())?;
        // SAFETY: kill takes integer arguments only. Each command starts a new
        // process group whose ID is its positive PID; this never targets our group.
        if unsafe { libc::kill(-group, libc::SIGKILL) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(format!(
                    "failed to terminate command process group: {error}"
                ));
            }
        }
    }
    #[cfg(windows)]
    if let Some(process_id) = process_id {
        let output = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &process_id.to_string()])
            .output()
            .await
            .map_err(|error| format!("failed to terminate command process tree: {error}"))?;
        if !output.status.success()
            && child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_none()
        {
            return Err("failed to terminate command process tree".to_string());
        }
    }
    child
        .kill()
        .await
        .map_err(|error| format!("failed to reap command process: {error}"))
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
    tail = truncate_output_tail(tail);
    let mut active = run.into_active_model();
    active.output_tail = Set(Some(tail));
    active.update(db).await?;
    Ok(())
}

fn truncate_output_tail(mut tail: String) -> String {
    if tail.len() > OUTPUT_TAIL_LIMIT {
        let mut keep_from = tail.len() - OUTPUT_TAIL_LIMIT;
        while !tail.is_char_boundary(keep_from) {
            keep_from += 1;
        }
        tail.drain(..keep_from);
    }
    tail
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

fn cancellers() -> &'static Mutex<HashMap<i32, oneshot::Sender<CancelRequest>>> {
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

fn register_canceller(run_id: i32) -> ApiResult<oneshot::Receiver<CancelRequest>> {
    let (sender, receiver) = oneshot::channel();
    cancellers()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command cancel registry"))?
        .insert(run_id, sender);
    Ok(receiver)
}

fn take_canceller(run_id: i32) -> ApiResult<Option<oneshot::Sender<CancelRequest>>> {
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

fn mark_cancelled(run_id: i32) -> ApiResult<bool> {
    Ok(cancelled_runs()
        .lock()
        .map_err(|_| ApiError::internal("failed to lock command cancel state"))?
        .insert(run_id))
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

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, path::PathBuf};

    use super::shell_command;

    #[tokio::test]
    async fn shell_process_uses_configured_working_directory() {
        let directory =
            std::env::temp_dir().join(format!("gpt-images-command-cwd-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();

        let output = shell_command(&directory, "pwd".to_string())
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let actual = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        assert_eq!(
            actual.canonicalize().unwrap(),
            directory.canonicalize().unwrap()
        );

        fs::remove_dir(&directory).unwrap();
    }

    struct ProcessTreeFixture {
        directory: PathBuf,
        group: Option<i32>,
    }

    impl ProcessTreeFixture {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("gpt-images-cancel-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&directory).unwrap();
            Self {
                directory,
                group: None,
            }
        }

        fn spawn(&mut self, background: bool) -> tokio::process::Child {
            let suffix = if background { "" } else { "wait" };
            let script = format!(
                "/bin/bash -c 'echo $$ > child.pid; /bin/sleep 60 & echo $! > grandchild.pid; wait' & {suffix}"
            );
            let child = shell_command(&self.directory, script).spawn().unwrap();
            let group = i32::try_from(child.id().unwrap()).unwrap();
            // The process tree must be isolated from the test runner's group.
            assert_eq!(unsafe { libc::getpgid(group) }, group);
            assert_ne!(unsafe { libc::getpgrp() }, group);
            self.group = Some(group);
            child
        }

        async fn descendant_pids(&self) -> Vec<i32> {
            tokio::time::timeout(std::time::Duration::from_secs(3), async {
                loop {
                    let pids: Option<Vec<i32>> = ["child.pid", "grandchild.pid"]
                        .iter()
                        .map(|name| {
                            fs::read_to_string(self.directory.join(name))
                                .ok()?
                                .trim()
                                .parse()
                                .ok()
                        })
                        .collect();
                    if let Some(pids) = pids {
                        return pids;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("descendants must start")
        }
    }

    impl Drop for ProcessTreeFixture {
        fn drop(&mut self) {
            if let Some(group) = self.group {
                // This group contains only processes created by this fixture.
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
            }
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    async fn assert_stopped(pids: &[i32]) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let stopped = pids.iter().all(|pid| {
                    let output = std::process::Command::new("ps")
                        .args(["-p", &pid.to_string(), "-o", "stat="])
                        .output()
                        .unwrap();
                    let state = String::from_utf8_lossy(&output.stdout);
                    state.trim().is_empty() || state.trim().starts_with('Z')
                });
                if stopped {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancel must stop every descendant");
    }

    fn output_reader<T: tokio::io::AsyncRead + Unpin + Send + 'static>(
        mut reader: T,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            tokio::io::copy(&mut reader, &mut tokio::io::sink())
                .await
                .unwrap();
        })
    }

    #[tokio::test]
    async fn cancel_stops_shell_children_and_grandchildren() {
        let mut fixture = ProcessTreeFixture::new();
        let child = fixture.spawn(false);
        let pids = fixture.descendant_pids().await;
        let (sender, mut receiver) = tokio::sync::oneshot::channel();
        let (completion, completed) = tokio::sync::oneshot::channel();
        sender
            .send(super::CancelRequest { completion })
            .ok()
            .unwrap();
        let result = super::wait_for_child(child, None, &mut receiver, None, None)
            .await
            .unwrap();
        assert!(matches!(result, super::CommandWaitResult::Cancelled));
        completed.await.unwrap().unwrap();
        assert_stopped(&pids).await;
    }

    #[tokio::test]
    async fn timeout_stops_the_entire_process_tree() {
        let mut fixture = ProcessTreeFixture::new();
        let child = fixture.spawn(false);
        let pids = fixture.descendant_pids().await;
        let (_sender, mut receiver) = tokio::sync::oneshot::channel();
        let result = super::wait_for_child(child, Some(1), &mut receiver, None, None)
            .await
            .unwrap();
        assert!(matches!(result, super::CommandWaitResult::TimedOut(1)));
        assert_stopped(&pids).await;
    }

    #[tokio::test]
    async fn cancellation_remains_active_after_shell_exit_while_pipes_are_open() {
        let mut fixture = ProcessTreeFixture::new();
        let mut child = fixture.spawn(true);
        let stdout = child.stdout.take().map(output_reader);
        let stderr = child.stderr.take().map(output_reader);
        let (sender, mut receiver) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            super::wait_for_child(child, None, &mut receiver, stdout, stderr).await
        });
        let pids = fixture.descendant_pids().await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            !task.is_finished(),
            "background descendants still hold the output pipes"
        );
        let (completion, completed) = tokio::sync::oneshot::channel();
        sender
            .send(super::CancelRequest { completion })
            .ok()
            .unwrap();
        completed.await.unwrap().unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(result, super::CommandWaitResult::Cancelled));
        assert_stopped(&pids).await;
    }

    async fn command_test_database() -> sea_orm::DatabaseConnection {
        use migration::{Migrator, MigratorTrait, SchemaManager};
        use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE users (id INTEGER PRIMARY KEY)".to_string(),
        ))
        .await
        .unwrap();
        let manager = SchemaManager::new(&db);
        for migration in Migrator::migrations().into_iter().filter(|migration| {
            matches!(
                migration.name(),
                "m20260616_000015_command_management" | "m20261010_000025_command_retries"
            )
        }) {
            migration.up(&manager).await.unwrap();
        }
        // SQLite accepts one column per ALTER TABLE, unlike the PostgreSQL migration.
        for column in ["preview_path_template", "preview_path"] {
            db.execute_unprepared(&format!(
                "ALTER TABLE command_runs ADD COLUMN {column} TEXT"
            ))
            .await
            .unwrap();
        }
        db
    }

    fn test_request(directory: &std::path::Path, script: String) -> super::CommandRunRequest {
        super::CommandRunRequest {
            template_id: None,
            name: "cancel process regression".to_string(),
            working_directory: directory.display().to_string(),
            command_line: script,
            setup_script: None,
            python_venv_path: None,
            env_vars: None,
            timeout_seconds: Some(10),
            retry_policy: super::RetryPolicy::default(),
            preview_path_template: None,
            triggered_by: "test".to_string(),
            created_by: None,
        }
    }

    #[tokio::test]
    #[serial_test::serial(command_cancel)]
    async fn cancel_api_confirms_processes_stopped_and_finalizes_record() {
        let db = command_test_database().await;
        let mut fixture = ProcessTreeFixture::new();
        let script = "/bin/bash -c 'echo $$ > child.pid; /bin/sleep 60 & echo $! > grandchild.pid; wait' & wait";
        let run = super::start_command_run(
            db.clone(),
            test_request(&fixture.directory, script.to_string()),
        )
        .await
        .unwrap();
        let pids = fixture.descendant_pids().await;
        let group = unsafe { libc::getpgid(pids[0]) };
        assert!(group > 0);
        fixture.group = Some(group);
        // Exercise two concurrent UI/API cancellation requests for the same record.
        let (first, second) = tokio::join!(
            super::cancel_run(&db, run.id),
            super::cancel_run(&db, run.id)
        );
        assert_eq!(first.unwrap().status, "cancelled");
        assert_eq!(second.unwrap().status, "cancelled");
        assert_stopped(&pids).await;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let finished = super::find_run(&db, run.id).await.unwrap();
                if finished.finished_at.is_some() {
                    assert_eq!(finished.status, "cancelled");
                    assert!(finished.duration_ms.is_some());
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!super::has_canceller(run.id));
        assert!(!super::is_cancelled(run.id));
    }

    #[tokio::test]
    #[serial_test::serial(command_cancel)]
    async fn queued_cancellation_never_spawns_the_command() {
        use sea_orm::{ActiveModelTrait, Set};
        let db = command_test_database().await;
        let fixture = ProcessTreeFixture::new();
        let run = super::command_runs::ActiveModel {
            name: Set("queued cancellation".to_string()),
            working_directory: Set(fixture.directory.display().to_string()),
            command_line: Set("touch should-not-start".to_string()),
            effective_script: Set("touch should-not-start".to_string()),
            status: Set("queued".to_string()),
            triggered_by: Set("test".to_string()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let receiver = super::register_canceller(run.id).unwrap();
        let cancel_db = db.clone();
        let cancel = tokio::spawn(async move { super::cancel_run(&cancel_db, run.id).await });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !super::is_cancelled(run.id) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        super::run_command_task(
            db.clone(),
            run.id,
            fixture.directory.clone(),
            "touch should-not-start".to_string(),
            None,
            super::RetryPolicy::default(),
            receiver,
        )
        .await;
        assert_eq!(cancel.await.unwrap().unwrap().status, "cancelled");
        let finished = super::find_run(&db, run.id).await.unwrap();
        assert!(finished.started_at.is_none());
        assert!(finished.finished_at.is_some());
        assert!(!fixture.directory.join("should-not-start").exists());
        assert!(!super::has_canceller(run.id));
    }

    async fn wait_finished(
        db: &sea_orm::DatabaseConnection,
        run_id: i32,
    ) -> super::command_runs::Model {
        tokio::time::timeout(std::time::Duration::from_secs(12), async {
            loop {
                let run = super::find_run(db, run_id).await.unwrap();
                if run.finished_at.is_some() {
                    return run;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("command must finish")
    }

    fn retry_policy(max_retries: i32) -> super::RetryPolicy {
        super::RetryPolicy {
            enabled: true,
            max_retries,
            delay_seconds: 1,
        }
    }

    #[test]
    fn retry_policy_is_opt_in_bounded_and_excludes_cancel_timeout_and_success() {
        use super::{CommandWaitResult, RetryPolicy};
        let default = RetryPolicy::from_options(None, None, None).unwrap();
        assert!(!default.enabled);
        assert!(!default.should_retry(&Ok(CommandWaitResult::Exited(Some(1))), 1));
        let enabled = retry_policy(2);
        assert!(enabled.should_retry(&Ok(CommandWaitResult::Exited(Some(1))), 1));
        assert!(enabled.should_retry(&Ok(CommandWaitResult::Exited(None)), 2));
        assert!(!enabled.should_retry(&Ok(CommandWaitResult::Exited(Some(1))), 3));
        for outcome in [
            CommandWaitResult::Cancelled,
            CommandWaitResult::TimedOut(1),
            CommandWaitResult::Exited(Some(0)),
        ] {
            assert!(!enabled.should_retry(&Ok(outcome), 1));
        }
        assert!(!enabled.should_retry(&Err("unknown process state".to_string()), 1));
        assert!(RetryPolicy::from_options(Some(true), Some(11), None).is_err());
        assert!(RetryPolicy::from_options(Some(true), None, Some(0)).is_err());
    }

    #[test]
    fn output_tail_preserves_utf8_when_retry_logs_are_appended() {
        let tail = format!("{}\n重新执行\n", "中文日志".repeat(3000));
        let truncated = super::truncate_output_tail(tail);
        assert!(truncated.len() <= super::OUTPUT_TAIL_LIMIT);
        assert!(truncated.ends_with("重新执行\n"));
    }

    #[tokio::test]
    #[serial_test::serial(command_cancel)]
    async fn transient_command_failure_retries_until_success() {
        let db = command_test_database().await;
        let fixture = ProcessTreeFixture::new();
        let script = "count=$(cat attempts 2>/dev/null || echo 0); count=$((count+1)); echo $count > attempts; if [ $count -lt 3 ]; then exit 7; fi; echo recovered";
        let mut request = test_request(&fixture.directory, script.to_string());
        request.retry_policy = retry_policy(3);
        let run = super::start_command_run(db.clone(), request).await.unwrap();
        let finished = wait_finished(&db, run.id).await;
        assert_eq!(finished.status, "success");
        assert_eq!(finished.exit_code, Some(0));
        assert_eq!(finished.attempt_count, 3);
        assert_eq!(
            fs::read_to_string(fixture.directory.join("attempts"))
                .unwrap()
                .trim(),
            "3"
        );
        assert!(finished.output_tail.unwrap().contains("重试 2/3"));
        assert!(!super::has_canceller(run.id));
    }

    #[tokio::test]
    #[serial_test::serial(command_cancel)]
    async fn retry_limit_and_disabled_policy_stop_failed_scripts() {
        let db = command_test_database().await;
        let fixture = ProcessTreeFixture::new();
        let mut request = test_request(&fixture.directory, "exit 9".to_string());
        request.retry_policy = retry_policy(1);
        let run = super::start_command_run(db.clone(), request).await.unwrap();
        let finished = wait_finished(&db, run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(finished.exit_code, Some(9));
        assert_eq!(finished.attempt_count, 2);
        let run = super::start_command_run(
            db.clone(),
            test_request(&fixture.directory, "exit 9".to_string()),
        )
        .await
        .unwrap();
        let finished = wait_finished(&db, run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(finished.attempt_count, 1);
    }

    #[tokio::test]
    #[serial_test::serial(command_cancel)]
    async fn signal_termination_retries_but_success_does_not() {
        let db = command_test_database().await;
        let fixture = ProcessTreeFixture::new();
        let script = "if [ ! -e signalled ]; then touch signalled; kill -TERM $$; fi; exit 0";
        let mut request = test_request(&fixture.directory, script.to_string());
        request.retry_policy = retry_policy(2);
        let run = super::start_command_run(db.clone(), request).await.unwrap();
        let finished = wait_finished(&db, run.id).await;
        assert_eq!(finished.status, "success");
        assert_eq!(finished.attempt_count, 2);
        let mut request = test_request(&fixture.directory, "exit 0".to_string());
        request.retry_policy = retry_policy(3);
        let run = super::start_command_run(db.clone(), request).await.unwrap();
        assert_eq!(wait_finished(&db, run.id).await.attempt_count, 1);
    }

    #[tokio::test]
    #[serial_test::serial(command_cancel)]
    async fn timeout_stops_without_retrying_even_when_enabled() {
        let db = command_test_database().await;
        let fixture = ProcessTreeFixture::new();
        let mut request = test_request(&fixture.directory, "sleep 60".to_string());
        request.retry_policy = retry_policy(3);
        request.timeout_seconds = Some(1);
        let run = super::start_command_run(db.clone(), request).await.unwrap();
        let finished = wait_finished(&db, run.id).await;
        assert_eq!(finished.status, "failed");
        assert_eq!(finished.attempt_count, 1);
        assert!(finished.error_message.unwrap().contains("timed out"));
    }

    #[tokio::test]
    #[serial_test::serial(command_cancel)]
    async fn cancellation_during_retry_delay_prevents_a_new_attempt() {
        let db = command_test_database().await;
        let fixture = ProcessTreeFixture::new();
        let mut request = test_request(
            &fixture.directory,
            "echo attempted >> attempts; exit 4".to_string(),
        );
        request.retry_policy = super::RetryPolicy {
            enabled: true,
            max_retries: 3,
            delay_seconds: 5,
        };
        let run = super::start_command_run(db.clone(), request).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let current = super::find_run(&db, run.id).await.unwrap();
                if current
                    .output_tail
                    .as_deref()
                    .is_some_and(|tail| tail.contains("重试 1/3"))
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            super::cancel_run(&db, run.id).await.unwrap().status,
            "cancelled"
        );
        let finished = wait_finished(&db, run.id).await;
        assert_eq!(finished.status, "cancelled");
        assert_eq!(finished.attempt_count, 1);
        assert_eq!(
            fs::read_to_string(fixture.directory.join("attempts"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(!super::has_canceller(run.id));
    }
}
