//! Local RDP gateway (guacd) that runs without Docker.
//!
//! `rdp install` (see `crate::rdp_manager`) sets guacd up natively: inside a
//! small Alpine WSL distro on Windows, from the system packages on Linux.
//! While the configured gateway address is on this machine and nothing
//! answers there, the backend starts the installed guacd itself and restarts
//! it when it exits.

#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    process::Stdio,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::Serialize;
use tokio::{io::AsyncBufReadExt, net::TcpStream, process::Command};

pub const DEFAULT_ADDRESS: &str = "127.0.0.1:4822";
/// WSL distro that `rdp install` imports on Windows.
pub const WSL_DISTRO: &str = "gpt-images-guacd";

const CHECK_INTERVAL: Duration = Duration::from_secs(20);
const MIN_BACKOFF: Duration = Duration::from_secs(5);
const MAX_BACKOFF: Duration = Duration::from_secs(90);
/// A run this long resets the restart backoff.
const STABLE_RUN: Duration = Duration::from_secs(45);
const ALIAS_TTL: Duration = Duration::from_secs(90);
const MAX_LOG_LINES: usize = 60;

/// Address of guacd: `RDP_GUACD_ADDRESS`, or the local default.
#[must_use]
pub fn address() -> String {
    std::env::var("RDP_GUACD_ADDRESS")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_ADDRESS.to_string())
}

/// Host and port to bind guacd to when `address` is on this machine.
#[must_use]
pub fn local_bind(address: &str) -> Option<(String, u16)> {
    if let Ok(socket) = address.parse::<SocketAddr>() {
        return socket
            .ip()
            .is_loopback()
            .then(|| (socket.ip().to_string(), socket.port()));
    }
    let (host, port) = address.rsplit_once(':')?;
    let port = port.parse().ok()?;
    host.eq_ignore_ascii_case("localhost")
        .then(|| (Ipv4Addr::LOCALHOST.to_string(), port))
}

#[must_use]
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

pub async fn reachable(address: &str) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(address)).await,
        Ok(Ok(_))
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Runtime {
    /// A `guacd` binary on this machine.
    Native(PathBuf),
    /// guacd inside the WSL distro imported by `rdp install`.
    Wsl,
}

impl Runtime {
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Native(path) => path.display().to_string(),
            Self::Wsl => format!("WSL ({WSL_DISTRO})"),
        }
    }
}

/// Finds an installed gateway. Blocking: may run `wsl.exe`.
#[must_use]
pub fn detect_runtime() -> Option<Runtime> {
    #[cfg(windows)]
    {
        if wsl::distro_installed() {
            return Some(Runtime::Wsl);
        }
    }
    find_guacd().map(Runtime::Native)
}

#[must_use]
pub fn find_guacd() -> Option<PathBuf> {
    find_in_path(if cfg!(windows) { "guacd.exe" } else { "guacd" })
}

/// Looks for `name` on `PATH` and in the sbin directories that a service's
/// `PATH` often lacks.
#[must_use]
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    directories.extend(
        [
            "/usr/sbin",
            "/usr/local/sbin",
            "/usr/bin",
            "/usr/local/bin",
            "/opt/homebrew/sbin",
            "/opt/homebrew/bin",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    directories
        .into_iter()
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// Alpine ships guacd's protocol plugins only as `libguac-client-*.so.0`,
/// but guacd loads `libguac-client-<protocol>.so`.
pub const WSL_PLUGIN_LINKS: &str =
    r#"for f in /usr/lib/libguac-client-*.so.0; do ln -sf "${f##*/}" "${f%.0}"; done"#;

/// Runs guacd in the foreground of the gateway distro, replacing a guacd
/// that an earlier run left behind.
#[must_use]
pub fn wsl_guacd_script(port: u16) -> String {
    format!("{WSL_PLUGIN_LINKS}; pkill -x guacd; exec guacd -f -b 127.0.0.1 -l {port} -L info")
}

/// Decodes `wsl.exe` output, which older WSL versions write as UTF-16.
#[must_use]
pub fn decode_wsl_output(bytes: &[u8]) -> String {
    let zeros = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    let text = if bytes.len() >= 2 && zeros * 4 >= bytes.len() {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.replace(['\0', '\u{feff}'], "")
}

/// Gateway address from `ip route show default` output.
#[must_use]
pub fn parse_default_gateway(output: &str) -> Option<Ipv4Addr> {
    output.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next() == Some("default")).then_some(())?;
        words.skip_while(|word| *word != "via").nth(1)?.parse().ok()
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct AlpineRelease {
    pub file: String,
    pub sha256: String,
}

/// The mini root filesystem entry of an Alpine `latest-releases.yaml`.
#[must_use]
pub fn parse_minirootfs(index: &str) -> Option<AlpineRelease> {
    let entries: Vec<HashMap<String, serde_yaml::Value>> = serde_yaml::from_str(index).ok()?;
    let entry = entries.into_iter().find(|entry| {
        entry.get("flavor").and_then(serde_yaml::Value::as_str) == Some("alpine-minirootfs")
    })?;
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(serde_yaml::Value::as_str)
            .map(str::to_string)
    };
    let file = text("file")?;
    let sha256 = text("sha256")?;
    let safe_name = !file.is_empty()
        && file
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'));
    (safe_name && sha256.len() == 64).then_some(AlpineRelease { file, sha256 })
}

#[cfg(windows)]
pub mod wsl {
    use std::{os::windows::process::CommandExt, process::Command};

    use super::{decode_wsl_output, WSL_DISTRO};

    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// `wsl.exe` printing UTF-8; inherits the console for interactive output.
    #[must_use]
    pub fn command() -> Command {
        let mut command = Command::new("wsl.exe");
        command.env("WSL_UTF8", "1");
        command
    }

    /// `wsl.exe` that never opens a console window (for captured output).
    #[must_use]
    pub fn hidden() -> Command {
        let mut command = command();
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }

    /// Runs `script` as root inside the gateway distro. `--exec` hands it
    /// to `sh` verbatim instead of through the distro's login shell, which
    /// would expand `$` first.
    #[must_use]
    pub fn shell(mut command: Command, script: &str) -> Command {
        command.args(["-d", WSL_DISTRO, "-u", "root", "--exec", "sh", "-c", script]);
        command
    }

    #[must_use]
    pub fn available() -> bool {
        ["--version", "--status"].into_iter().any(|flag| {
            hidden()
                .arg(flag)
                .output()
                .is_ok_and(|output| output.status.success())
        })
    }

    #[must_use]
    pub fn distro_installed() -> bool {
        hidden()
            .args(["--list", "--quiet"])
            .output()
            .is_ok_and(|output| {
                decode_wsl_output(&output.stdout)
                    .lines()
                    .any(|name| name.trim().eq_ignore_ascii_case(WSL_DISTRO))
            })
    }

    /// WSL is per user; a service running as SYSTEM cannot reach the distro.
    #[must_use]
    pub fn running_as_system() -> bool {
        std::env::var("USERNAME")
            .is_ok_and(|name| name.eq_ignore_ascii_case("SYSTEM") || name.ends_with('$'))
    }
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, utoipa::ToSchema)]
pub struct SupervisorStatus {
    /// Installed gateway on this machine, e.g. `WSL (gpt-images-guacd)`.
    pub runtime: Option<String>,
    /// guacd started by this backend is running.
    pub running: bool,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub recent_logs: Vec<String>,
}

#[derive(Default)]
struct State {
    pid: Option<u32>,
    restarts: u32,
    last_error: Option<String>,
    logs: VecDeque<String>,
    loopback_alias: Option<(Option<String>, Instant)>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn update(change: impl FnOnce(&mut State)) {
    if let Ok(mut state) = state().lock() {
        change(&mut state);
    }
}

fn push_log(line: String) {
    update(|state| {
        if state.logs.len() >= MAX_LOG_LINES {
            state.logs.pop_front();
        }
        state.logs.push_back(line);
    });
}

pub async fn status() -> SupervisorStatus {
    let runtime = tokio::task::spawn_blocking(detect_runtime)
        .await
        .ok()
        .flatten()
        .map(|runtime| runtime.describe());
    state().lock().map_or_else(
        |_| SupervisorStatus::default(),
        |state| SupervisorStatus {
            runtime,
            running: state.pid.is_some(),
            pid: state.pid,
            restarts: state.restarts,
            last_error: state.last_error.clone(),
            recent_logs: state.logs.iter().cloned().collect(),
        },
    )
}

/// Keeps the installed guacd running for the lifetime of the process.
pub fn start() {
    tokio::spawn(supervise());
}

async fn supervise() {
    let mut backoff = MIN_BACKOFF;
    loop {
        let address = address();
        let bind = match local_bind(&address) {
            Some(bind) if !reachable(&address).await => bind,
            _ => {
                tokio::time::sleep(CHECK_INTERVAL).await;
                continue;
            }
        };
        let Some(runtime) = tokio::task::spawn_blocking(detect_runtime)
            .await
            .ok()
            .flatten()
        else {
            tokio::time::sleep(CHECK_INTERVAL).await;
            continue;
        };
        match spawn(&runtime, &bind.0, bind.1).await {
            Ok(mut child) => {
                let pid = child.id();
                update(|state| {
                    state.pid = pid;
                    state.last_error = None;
                });
                push_log(format!(
                    "guacd 已启动（{}，PID {}）",
                    runtime.describe(),
                    pid.unwrap_or(0)
                ));
                let started = Instant::now();
                let message = child.wait().await.map_or_else(
                    |error| format!("guacd 运行失败：{error}"),
                    |status| format!("guacd 已退出：{status}"),
                );
                push_log(message.clone());
                update(|state| {
                    state.pid = None;
                    state.last_error = Some(message);
                    state.restarts += 1;
                });
                if started.elapsed() > STABLE_RUN {
                    backoff = MIN_BACKOFF;
                }
            }
            Err(error) => {
                push_log(error.clone());
                update(|state| {
                    state.pid = None;
                    state.last_error = Some(error);
                });
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

async fn spawn(runtime: &Runtime, host: &str, port: u16) -> Result<tokio::process::Child, String> {
    let mut command = match runtime {
        Runtime::Native(path) => {
            let mut command = Command::new(path);
            command.args(["-f", "-b", host, "-l", &port.to_string(), "-L", "info"]);
            command
        }
        Runtime::Wsl => wsl_guacd(port).await?,
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("启动 guacd 失败：{error}"))?;
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(forward_lines(stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(forward_lines(stderr));
    }
    Ok(child)
}

#[cfg(windows)]
#[allow(clippy::unused_async)]
async fn wsl_guacd(port: u16) -> Result<Command, String> {
    if wsl::running_as_system() {
        return Err(
            "后台服务以 SYSTEM 账户运行，无法使用 WSL 中的 RDP 网关：请用 service install --user 安装为当前用户的服务"
                .to_string(),
        );
    }
    Ok(Command::from(wsl::shell(
        wsl::hidden(),
        &wsl_guacd_script(port),
    )))
}

#[cfg(not(windows))]
#[allow(clippy::unused_async)]
async fn wsl_guacd(_port: u16) -> Result<Command, String> {
    Err("WSL 网关只能在 Windows 上使用".to_string())
}

async fn forward_lines(stream: impl tokio::io::AsyncRead + Unpin) {
    let mut lines = tokio::io::BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        push_log(line);
    }
}

/// Starts guacd in the background for the `rdp start` command, independent
/// of this process.
pub fn spawn_detached(runtime: &Runtime, host: &str, port: u16) -> Result<(), String> {
    match runtime {
        // Without `-f` guacd forks into the background and the parent exits.
        Runtime::Native(path) => std::process::Command::new(path)
            .args(["-b", host, "-l", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("failed to start {}: {error}", path.display()))
            .and_then(|status| {
                status
                    .success()
                    .then_some(())
                    .ok_or_else(|| format!("guacd exited with {status}"))
            }),
        Runtime::Wsl => spawn_detached_wsl(port),
    }
}

#[cfg(windows)]
fn spawn_detached_wsl(port: u16) -> Result<(), String> {
    use std::os::windows::{io::AsRawHandle, process::CommandExt};
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    #[link(name = "kernel32")]
    extern "system" {
        fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    // Every inheritable handle leaks into children on Windows. The long-lived
    // wsl.exe would keep this command's output pipes open, so a caller
    // reading them (the project CLI page) would never see it finish.
    for handle in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        if !handle.is_null() {
            // SAFETY: the standard handles belong to this process and stay
            // open; only their inherit flag changes.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
    // WSL keeps the distro running only while a wsl.exe client is attached,
    // so guacd stays in the foreground of a long-lived wsl.exe.
    wsl::shell(wsl::hidden(), &wsl_guacd_script(port))
        .creation_flags(wsl::CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to start guacd in WSL: {error}"))
}

#[cfg(not(windows))]
fn spawn_detached_wsl(_port: u16) -> Result<(), String> {
    Err("the WSL gateway is only available on Windows".to_string())
}

/// Host guacd should dial for an RDP target. In WSL's NAT network
/// `127.0.0.1` is the Linux VM, so targets on this Windows machine are
/// reached through the Windows side of the virtual network instead.
pub async fn dial_host(host: &str) -> String {
    if !is_loopback_host(host) || local_bind(&address()).is_none() {
        return host.to_string();
    }
    let cached = state().lock().ok().and_then(|state| {
        state
            .loopback_alias
            .clone()
            .filter(|(_, at)| at.elapsed() < ALIAS_TTL)
    });
    let alias = if let Some((alias, _)) = cached {
        alias
    } else {
        let alias = tokio::task::spawn_blocking(windows_host_from_wsl)
            .await
            .ok()
            .flatten();
        update(|state| state.loopback_alias = Some((alias.clone(), Instant::now())));
        alias
    };
    alias.unwrap_or_else(|| host.to_string())
}

#[cfg(windows)]
fn windows_host_from_wsl() -> Option<String> {
    if !wsl::distro_installed() {
        return None;
    }
    let output = wsl::shell(wsl::hidden(), "ip route show default")
        .output()
        .ok()?;
    let gateway = parse_default_gateway(&decode_wsl_output(&output.stdout))?;
    // In NAT mode the gateway is Windows' own vEthernet address. Mirrored
    // networking shares Windows' addresses, where loopback already works.
    std::net::UdpSocket::bind((gateway, 0))
        .is_ok()
        .then(|| gateway.to_string())
}

#[cfg(not(windows))]
const fn windows_host_from_wsl() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_addresses_are_managed_locally() {
        assert_eq!(
            local_bind("127.0.0.1:4822"),
            Some(("127.0.0.1".to_string(), 4822))
        );
        assert_eq!(local_bind("[::1]:5000"), Some(("::1".to_string(), 5000)));
        assert_eq!(
            local_bind("localhost:4822"),
            Some(("127.0.0.1".to_string(), 4822))
        );
        assert_eq!(local_bind("192.168.1.5:4822"), None);
        assert_eq!(local_bind("gateway.lan:4822"), None);
    }

    #[test]
    fn recognizes_loopback_targets() {
        for host in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.0.1.1",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in ["192.168.1.10", "pc.lan", "localhost.example.com"] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }

    #[test]
    fn decodes_utf16_and_utf8_wsl_output() {
        let utf16: Vec<u8> = "\u{feff}docker-desktop\r\ngpt-images-guacd\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let text = decode_wsl_output(&utf16);
        assert!(
            text.lines().any(|line| line.trim() == WSL_DISTRO),
            "{text:?}"
        );
        assert_eq!(decode_wsl_output(b"Ubuntu\n"), "Ubuntu\n");
    }

    #[test]
    fn parses_default_gateway() {
        let output = "default via 172.22.16.1 dev eth0 proto kernel\n172.22.16.0/20 dev eth0 proto kernel scope link src 172.22.27.4\n";
        assert_eq!(
            parse_default_gateway(output),
            Some(Ipv4Addr::new(172, 22, 16, 1))
        );
        assert_eq!(parse_default_gateway("172.22.16.0/20 dev eth0\n"), None);
    }

    #[test]
    fn finds_the_minirootfs_release() {
        let index = "---\n-\n  title: \"Netboot\"\n  flavor: alpine-netboot\n  file: alpine-netboot-3.24.2-x86_64.tar.gz\n  sha256: 0000000000000000000000000000000000000000000000000000000000000000\n-\n  title: \"Mini root filesystem\"\n  desc: |\n    Minimal root filesystem.\n  branch: v3.24\n  version: 3.24.2\n  flavor: alpine-minirootfs\n  file: alpine-minirootfs-3.24.2-x86_64.tar.gz\n  date: 2026-09-17\n  time: 17:32:03\n  size: 3701382\n  sha256: c5ca053cfe1d85c5b96dff8b9bc57045f7f184a30ffb6b65776409ca90388677\n";
        assert_eq!(
            parse_minirootfs(index),
            Some(AlpineRelease {
                file: "alpine-minirootfs-3.24.2-x86_64.tar.gz".to_string(),
                sha256: "c5ca053cfe1d85c5b96dff8b9bc57045f7f184a30ffb6b65776409ca90388677"
                    .to_string(),
            })
        );
        let unsafe_name = index.replace(
            "file: alpine-minirootfs-3.24.2-x86_64.tar.gz",
            "file: ../evil.tar.gz",
        );
        assert_eq!(parse_minirootfs(&unsafe_name), None);
    }
}
