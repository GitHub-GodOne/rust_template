//! Supervises the local `cloudflared` connector for "tunnel" domain mappings.
//!
//! The tunnel is remotely managed: ingress rules live in Cloudflare and the
//! connector only needs the tunnel token, passed through `TUNNEL_TOKEN` so it
//! never shows up in the process list.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use serde::Serialize;
use tokio::{io::AsyncBufReadExt, process::Command, sync::watch, task::JoinHandle};

const MAX_LOG_LINES: usize = 80;
// Keep compatibility with stable Rust versions before duration_constructors.
#[allow(clippy::duration_suboptimal_units)]
const STABLE_RUN: Duration = Duration::from_secs(60);
#[allow(clippy::duration_suboptimal_units)]
const MAX_BACKOFF: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Debug, Default, Serialize, utoipa::ToSchema)]
pub struct TunnelStatus {
    /// Path of the `cloudflared` binary, when one was found.
    pub binary: Option<String>,
    pub running: bool,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub recent_logs: Vec<String>,
}

struct Supervisor {
    token: String,
    binary: PathBuf,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct RuntimeState {
    pid: Option<u32>,
    restarts: u32,
    last_error: Option<String>,
    logs: VecDeque<String>,
}

fn supervisor() -> &'static tokio::sync::Mutex<Option<Supervisor>> {
    static SUPERVISOR: OnceLock<tokio::sync::Mutex<Option<Supervisor>>> = OnceLock::new();
    SUPERVISOR.get_or_init(|| tokio::sync::Mutex::new(None))
}

fn runtime() -> &'static Mutex<RuntimeState> {
    static RUNTIME: OnceLock<Mutex<RuntimeState>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(RuntimeState::default()))
}

fn update_runtime(change: impl FnOnce(&mut RuntimeState)) {
    if let Ok(mut state) = runtime().lock() {
        change(&mut state);
    }
}

fn push_log(line: String) {
    update_runtime(|state| {
        if state.logs.len() >= MAX_LOG_LINES {
            state.logs.pop_front();
        }
        state.logs.push_back(line);
    });
}

/// Finds `cloudflared`: the configured path, then every directory on `PATH`.
#[must_use]
pub fn find_binary(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        return path.is_file().then_some(path);
    }
    let names: &[&str] = if cfg!(windows) {
        &["cloudflared.exe", "cloudflared"]
    } else {
        &["cloudflared"]
    };
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    // Common install locations that are often missing from a service's PATH.
    directories.extend(
        ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"]
            .into_iter()
            .map(PathBuf::from),
    );
    directories
        .iter()
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Starts, restarts or stops the connector so it runs with `desired`.
// The supervisor lock is held throughout on purpose: it serializes start/stop.
#[allow(clippy::significant_drop_tightening)]
pub async fn apply(desired: Option<(PathBuf, String)>) {
    let mut current = supervisor().lock().await;
    let unchanged = match (&*current, &desired) {
        (Some(running), Some((binary, token))) => {
            running.token == *token && running.binary == *binary && !running.task.is_finished()
        }
        (None, None) => true,
        _ => false,
    };
    if unchanged {
        return;
    }
    if let Some(running) = current.take() {
        let _ = running.stop.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(10), running.task).await;
    }
    if let Some((binary, token)) = desired {
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(run(binary.clone(), token.clone(), stopped));
        *current = Some(Supervisor {
            token,
            binary,
            stop,
            task,
        });
    } else {
        update_runtime(|state| state.pid = None);
    }
}

/// Forces a restart of the connector with its current configuration.
pub async fn restart() -> bool {
    let current = supervisor().lock().await.take();
    let Some(running) = current else {
        return false;
    };
    let _ = running.stop.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(10), running.task).await;
    apply(Some((running.binary, running.token))).await;
    true
}

pub async fn status(configured_binary: &str) -> TunnelStatus {
    let running = supervisor()
        .lock()
        .await
        .as_ref()
        .is_some_and(|supervisor| !supervisor.task.is_finished());
    let binary = find_binary(configured_binary).map(|path| path.display().to_string());
    runtime().lock().map_or_else(
        |_| TunnelStatus::default(),
        |state| TunnelStatus {
            binary,
            running: running && state.pid.is_some(),
            pid: state.pid,
            restarts: state.restarts,
            last_error: state.last_error.clone(),
            recent_logs: state.logs.iter().cloned().collect(),
        },
    )
}

async fn run(binary: PathBuf, token: String, mut stopped: watch::Receiver<bool>) {
    let mut backoff = Duration::from_secs(5);
    loop {
        match spawn(&binary, &token) {
            Ok(mut child) => {
                update_runtime(|state| {
                    state.pid = child.id();
                    state.last_error = None;
                });
                push_log(format!(
                    "cloudflared 已启动（PID {}）",
                    child.id().unwrap_or(0)
                ));
                let started = tokio::time::Instant::now();
                tokio::select! {
                    status = child.wait() => {
                        let message = status.map_or_else(
                            |error| format!("cloudflared 运行失败：{error}"),
                            |status| format!("cloudflared 已退出：{status}"),
                        );
                        push_log(message.clone());
                        update_runtime(|state| {
                            state.pid = None;
                            state.last_error = Some(message);
                            state.restarts += 1;
                        });
                        if started.elapsed() > STABLE_RUN {
                            backoff = Duration::from_secs(5);
                        }
                    }
                    _ = stopped.changed() => {
                        let _ = child.kill().await;
                        update_runtime(|state| state.pid = None);
                        push_log("cloudflared 已停止".to_string());
                        return;
                    }
                }
            }
            Err(error) => {
                push_log(error.clone());
                update_runtime(|state| {
                    state.pid = None;
                    state.last_error = Some(error);
                });
            }
        }
        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            _ = stopped.changed() => return,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

fn spawn(binary: &Path, token: &str) -> Result<tokio::process::Child, String> {
    let mut child = Command::new(binary)
        .args(["tunnel", "--no-autoupdate", "run"])
        .env("TUNNEL_TOKEN", token)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("启动 {} 失败：{error}", binary.display()))?;
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(forward_lines(stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(forward_lines(stderr));
    }
    Ok(child)
}

async fn forward_lines(stream: impl tokio::io::AsyncRead + Unpin) {
    let mut lines = tokio::io::BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        push_log(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_binary_must_exist() {
        assert!(find_binary("/definitely/not/here/cloudflared").is_none());
        let file = std::env::temp_dir().join(format!("cloudflared-{}", uuid::Uuid::new_v4()));
        std::fs::write(&file, b"").unwrap();
        assert_eq!(find_binary(file.to_str().unwrap()), Some(file.clone()));
        std::fs::remove_file(file).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_runs_and_stops_the_connector() {
        use std::os::unix::fs::PermissionsExt;
        // A stand-in connector that proves it received the token via the environment.
        let script =
            std::env::temp_dir().join(format!("fake-cloudflared-{}", uuid::Uuid::new_v4()));
        std::fs::write(
            &script,
            "#!/bin/sh\necho \"token=$TUNNEL_TOKEN args=$*\"\nsleep 30\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        apply(Some((script.clone(), "secret-token".to_string()))).await;
        let mut seen = false;
        for _ in 0..50 {
            let status = status(script.to_str().unwrap()).await;
            if status.running
                && status
                    .recent_logs
                    .iter()
                    .any(|line| line == "token=secret-token args=tunnel --no-autoupdate run")
            {
                seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(seen, "{:?}", status(script.to_str().unwrap()).await);

        apply(None).await;
        assert!(!status(script.to_str().unwrap()).await.running);
        std::fs::remove_file(script).unwrap();
    }
}
