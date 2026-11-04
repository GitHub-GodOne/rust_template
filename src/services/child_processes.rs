//! Processes started for 命令管理 and 项目命令 runs end with the server.
//!
//! Ctrl+C does not reach them by itself: on Unix every run gets its own
//! process group, on Windows command shells have no console. So each run's
//! process is registered here while it runs, and a graceful stop (Ctrl+C,
//! `service stop`) kills every registered process tree from
//! `App::on_shutdown`.
//!
//! On Windows the processes also join a kill-on-close job object. Children
//! inherit the job, and the system ends all of them when the server exits in
//! any way, including a crash or `taskkill`. That also covers descendants
//! whose parent already exited, which `taskkill /T` cannot find.

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};

use tokio::process::Child;

static STOPPING: AtomicBool = AtomicBool::new(false);

fn live() -> &'static Mutex<HashSet<u32>> {
    static LIVE: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Registration of a running process; dropping it unregisters the process.
pub struct Tracked(Option<u32>);

impl Drop for Tracked {
    fn drop(&mut self) {
        if let Some(process_id) = self.0 {
            if let Ok(mut live) = live().lock() {
                live.remove(&process_id);
            }
        }
    }
}

/// Ties `child` and everything it starts to the server's lifetime. Keep the
/// result until the process exited.
#[must_use]
pub fn track(child: &Child) -> Tracked {
    #[cfg(windows)]
    job::assign(child);
    let process_id = child.id();
    if let (Some(process_id), Ok(mut live)) = (process_id, live().lock()) {
        live.insert(process_id);
    }
    Tracked(process_id)
}

/// Set once the server started shutting down: no new runs or retries.
pub fn stopping() -> bool {
    STOPPING.load(Ordering::SeqCst)
}

/// Kills every tracked process tree. Called when the server shuts down.
pub async fn terminate_all() {
    STOPPING.store(true, Ordering::SeqCst);
    let process_ids: Vec<u32> = live()
        .lock()
        .map(|live| live.iter().copied().collect())
        .unwrap_or_default();
    for process_id in process_ids {
        if let Err(error) = kill_tree(process_id).await {
            tracing::warn!(
                process_id,
                error,
                "failed to stop command process on shutdown"
            );
        }
    }
}

/// Kills the process group (Unix) or process tree (Windows) of a command
/// started by this server.
///
/// # Errors
///
/// When the signal or `taskkill` fails, e.g. because the process is gone.
pub async fn kill_tree(process_id: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
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
    {
        let output = tokio::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &process_id.to_string()])
            .output()
            .await
            .map_err(|error| format!("failed to terminate command process tree: {error}"))?;
        if !output.status.success() {
            return Err("failed to terminate command process tree".to_string());
        }
    }
    Ok(())
}

#[cfg(windows)]
mod job {
    use std::sync::OnceLock;

    use tokio::process::Child;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
    };

    struct Job(HANDLE);

    // SAFETY: a job handle can be used from any thread; it is never closed.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    /// The server holds the only handle, so the job closes when it exits.
    fn job() -> Option<&'static Job> {
        static JOB: OnceLock<Option<Job>> = OnceLock::new();
        JOB.get_or_init(|| {
            // SAFETY: null attributes and name create an anonymous job.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                let error = std::io::Error::last_os_error();
                tracing::warn!(%error, "failed to create command job object");
                return None;
            }
            // SAFETY: plain data; all-zero means "no limits".
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `limits` is the structure this information class expects.
            let configured = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                        .unwrap_or(u32::MAX),
                )
            };
            if configured == 0 {
                let error = std::io::Error::last_os_error();
                tracing::warn!(%error, "failed to configure command job object");
                // SAFETY: the handle was created above and is not used again.
                unsafe { CloseHandle(handle) };
                return None;
            }
            Some(Job(handle))
        })
        .as_ref()
    }

    /// Processes the child starts afterwards join the job too. The shells
    /// need far longer to start than this call takes, so nothing they run
    /// starts outside the job.
    pub(super) fn assign(child: &Child) {
        let (Some(job), Some(process)) = (job(), child.raw_handle()) else {
            return;
        };
        // SAFETY: both handles are open; the child is not reaped before `track` returns.
        if unsafe { AssignProcessToJobObject(job.0, process as HANDLE) } == 0 {
            let error = std::io::Error::last_os_error();
            tracing::warn!(%error, "failed to add command process to job object");
        }
    }

    #[cfg(test)]
    pub(super) fn current() -> HANDLE {
        job().map_or(std::ptr::null_mut(), |job| job.0)
    }
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use super::{live, track};

    fn long_running() -> tokio::process::Command {
        #[cfg(windows)]
        {
            let mut command = tokio::process::Command::new("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command"]);
            command.arg(
                "$p = Start-Process ping.exe -ArgumentList '-n','30','127.0.0.1' -WindowStyle Hidden -PassThru; $p.Id; Start-Sleep 30",
            );
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.args(["-c", "echo $$; sleep 30"]);
            command
        }
    }

    #[tokio::test]
    async fn tracking_ends_when_the_registration_is_dropped() {
        let mut child = long_running()
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let process_id = child.id().unwrap();
        let tracked = track(&child);
        assert!(live().lock().unwrap().contains(&process_id));
        drop(tracked);
        assert!(!live().lock().unwrap().contains(&process_id));
        child.kill().await.unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn processes_and_their_children_join_the_kill_on_close_job() {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{
                JobObjects::IsProcessInJob,
                Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
            },
        };

        fn in_job(process_id: u32) -> bool {
            // SAFETY: the handle is checked and closed before returning.
            unsafe {
                let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
                assert!(!process.is_null(), "process {process_id} must be running");
                let mut result = 0;
                let queried = IsProcessInJob(process, super::job::current(), &raw mut result);
                CloseHandle(process);
                assert_ne!(queried, 0);
                result != 0
            }
        }

        let mut child = long_running()
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let _tracked = track(&child);
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let grandchild: u32 = lines
            .next_line()
            .await
            .unwrap()
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let in_jobs = (in_job(child.id().unwrap()), in_job(grandchild));
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &grandchild.to_string()])
            .output();
        child.kill().await.unwrap();
        assert_eq!(in_jobs, (true, true));
    }
}
