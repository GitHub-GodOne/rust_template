//! SSH 管理 shells that can outlive the server.
//!
//! Each session's PTY lives in a small detached process: this binary started
//! again with [`HOST_ARG`] and the session directory
//! `storage/ssh-sessions/<session id>/`. The server writes `spec.json` there
//! (command, size, access token, session metadata), the host answers with
//! `host.json` (its PID and loopback port), and the two talk line-delimited
//! JSON over that port: [`HostRequest`] in, [`HostEvent`] out.
//!
//! The host keeps the recent output, so a server that starts again attaches
//! to every directory still there and the terminal continues where it was.
//! Whether a host waits for that is `persist`
//! (`ssh_manager.keep_sessions_on_restart`): without it the host ends the
//! shell as soon as the server's connection drops, however the server
//! stopped. The host removes its directory when the shell exits.

use std::{
    collections::VecDeque,
    env,
    ffi::OsStr,
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

use crate::services::command_runner::simplify_verbatim_path;

/// First argument that turns this binary into a session host.
pub const HOST_ARG: &str = "__ssh-session-host";
pub const SESSIONS_DIR: &str = "storage/ssh-sessions";
const SPEC_FILE: &str = "spec.json";
const INFO_FILE: &str = "host.json";
/// Windows copies of the binary hosts run from, see [`host_executable`].
#[cfg(windows)]
const EXECUTABLES_DIR: &str = "storage/ssh-sessions/bin";
/// Most terminal lines kept (`ssh_manager.scrollback_lines` = 0 means this):
/// a browser terminal needs about 1.4 KB per line, so far more would
/// exhaust the page's memory.
pub const MAX_SCROLLBACK_LINES: usize = 100_000;
/// Output without line breaks (progress bars redrawn with `\r`) is bounded
/// as if each line held this many bytes.
const BYTES_PER_LINE: usize = 256;
/// Replayed output is sent in pieces of about this size.
const SNAPSHOT_PIECE_BYTES: usize = 64 * 1024;
const START_TIMEOUT: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 5 });
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a restarted server waits for the previous one to let go.
const BUSY_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a dropped connection is retried before the session counts as
/// ended.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A host nobody attached to by then ends its shell unless it persists.
#[allow(clippy::duration_suboptimal_units)]
const FIRST_ATTACH_TIMEOUT: Duration = Duration::from_secs(60);
/// Lets the last output reach the server before the exit event.
const EXIT_FLUSH_DELAY: Duration = Duration::from_millis(300);

/// Lines kept for `ssh_manager.scrollback_lines`: 0 (or less) means the
/// maximum.
#[must_use]
pub fn scrollback_lines(setting: i64) -> usize {
    usize::try_from(setting)
        .ok()
        .filter(|lines| *lines > 0)
        .map_or(MAX_SCROLLBACK_LINES, |lines| {
            lines.min(MAX_SCROLLBACK_LINES)
        })
}

/// Recent terminal output in the chunks it arrived in, trimmed from the
/// front to a number of lines.
#[derive(Debug, Default)]
pub struct OutputHistory {
    chunks: VecDeque<String>,
    lines: usize,
    bytes: usize,
}

fn line_breaks(data: &str) -> usize {
    data.bytes().filter(|byte| *byte == b'\n').count()
}

impl OutputHistory {
    #[must_use]
    pub fn from_chunks(chunks: Vec<String>, limit: usize) -> Self {
        let mut history = Self::default();
        for chunk in chunks {
            history.push(chunk, limit);
        }
        history
    }

    pub fn push(&mut self, data: String, limit: usize) {
        self.lines += line_breaks(&data);
        self.bytes += data.len();
        self.chunks.push_back(data);
        self.trim(limit);
    }

    /// Drops the oldest chunks beyond `limit` lines; the newest chunk stays.
    pub fn trim(&mut self, limit: usize) {
        let max_bytes = limit.saturating_mul(BYTES_PER_LINE);
        while (self.lines > limit || self.bytes > max_bytes) && self.chunks.len() > 1 {
            let Some(chunk) = self.chunks.pop_front() else {
                break;
            };
            self.lines -= line_breaks(&chunk);
            self.bytes -= chunk.len();
        }
    }

    /// The kept output joined into pieces of about 64 KB, for replaying it
    /// in few messages.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        let mut pieces: Vec<String> = Vec::new();
        for chunk in &self.chunks {
            match pieces.last_mut() {
                Some(piece) if piece.len() + chunk.len() <= SNAPSHOT_PIECE_BYTES => {
                    piece.push_str(chunk);
                }
                _ => pieces.push(chunk.clone()),
            }
        }
        pieces
    }
}

/// Program the PTY runs, kept as data so the host can rebuild it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl ShellCommand {
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    pub fn arg(&mut self, value: impl Into<String>) -> &mut Self {
        self.args.push(value.into());
        self
    }

    pub fn env(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.env.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn builder(&self) -> CommandBuilder {
        let mut builder = CommandBuilder::new(&self.program);
        builder.args(&self.args);
        for (key, value) in &self.env {
            builder.env(key, value);
        }
        builder
    }
}

/// `spec.json`: everything a host needs, plus the server's own `session`
/// metadata to restore the session from after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSpec {
    pub token: String,
    pub command: ShellCommand,
    pub cols: u16,
    pub rows: u16,
    pub persist: bool,
    /// Lines of output kept for replaying (see [`scrollback_lines`]).
    #[serde(default = "default_history_lines")]
    pub history_lines: usize,
    pub session: serde_json::Value,
}

const fn default_history_lines() -> usize {
    MAX_SCROLLBACK_LINES
}

/// `host.json`, written once the host listens or failed to start.
#[derive(Debug, Serialize, Deserialize)]
struct HostInfo {
    pid: u32,
    port: Option<u16>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequest {
    /// First line of every connection.
    Attach {
        token: String,
    },
    Input {
        data: Vec<u8>,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Whether the shell outlives a lost connection.
    Persist {
        value: bool,
    },
    /// Lines of output kept for the next server that attaches.
    HistoryLimit {
        lines: usize,
    },
    Close,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostEvent {
    /// Answer to `Attach`, with the output kept so far.
    Hello {
        cols: u16,
        rows: u16,
        last_activity_ms: i64,
        history: Vec<String>,
    },
    Output {
        data: String,
    },
    Exit,
    /// Another server is attached (e.g. a second instance starting while the
    /// first still runs); it keeps the session.
    Busy,
}

/// Runs the session host when this process was started as one.
#[must_use = "the process must exit when this returns true"]
pub fn run_if_requested() -> bool {
    let mut arguments = env::args_os().skip(1);
    if arguments.next().as_deref() != Some(OsStr::new(HOST_ARG)) {
        return false;
    }
    let Some(directory) = arguments.next().map(PathBuf::from) else {
        return true;
    };
    if let Err(error) = host_main(&directory) {
        let _ = write_info(
            &directory,
            &HostInfo {
                pid: std::process::id(),
                port: None,
                error: Some(error),
            },
        );
    }
    true
}

fn host_main(directory: &Path) -> Result<(), String> {
    let spec: HostSpec = fs::read(directory.join(SPEC_FILE))
        .map_err(|error| format!("failed to read session spec: {error}"))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| format!("invalid session spec: {error}"))
        })?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|error| format!("failed to listen: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("failed to read listen address: {error}"))?
        .port();
    let (host, child, reader) = Host::start(&spec)?;
    let info = HostInfo {
        pid: std::process::id(),
        port: Some(port),
        error: None,
    };
    if let Err(error) = write_info(directory, &info) {
        host.kill();
        return Err(error);
    }
    host.serve(listener, child, reader);
    let _ = fs::remove_dir_all(directory);
    Ok(())
}

fn write_info(directory: &Path, info: &HostInfo) -> Result<(), String> {
    let bytes = serde_json::to_vec(info).map_err(|error| error.to_string())?;
    let partial = directory.join(format!("{INFO_FILE}.partial"));
    fs::write(&partial, bytes)
        .and_then(|()| fs::rename(&partial, directory.join(INFO_FILE)))
        .map_err(|error| format!("failed to write host info: {error}"))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn write_line(stream: &mut TcpStream, message: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
    line.push(b'\n');
    stream.write_all(&line)
}

struct Host {
    token: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    state: Mutex<HostState>,
}

struct HostState {
    history: OutputHistory,
    history_lines: usize,
    /// The attached server, tagged so an old connection that ends late
    /// cannot detach its successor.
    client: Option<(u64, TcpStream)>,
    next_client: u64,
    attached_once: bool,
    cols: u16,
    rows: u16,
    last_activity_ms: i64,
    persist: bool,
    exited: bool,
}

impl HostState {
    /// Sends to the attached server; a failed write detaches it.
    fn send(&mut self, event: &HostEvent) -> bool {
        let Some((_, stream)) = self.client.as_mut() else {
            return true;
        };
        if write_line(stream, event).is_ok() {
            return true;
        }
        if let Some((_, stream)) = self.client.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        false
    }
}

type Started = (
    Arc<Host>,
    Box<dyn Child + Send + Sync>,
    Box<dyn Read + Send>,
);

impl Host {
    fn start(spec: &HostSpec) -> Result<Started, String> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("failed to open PTY: {error}"))?;
        let mut child = pair
            .slave
            .spawn_command(spec.command.builder())
            .map_err(|error| format!("failed to start the terminal: {error}"))?;
        drop(pair.slave);
        let streams = pair
            .master
            .try_clone_reader()
            .and_then(|reader| Ok((reader, pair.master.take_writer()?)));
        let (reader, writer) = match streams {
            Ok(streams) => streams,
            Err(error) => {
                let _ = child.kill();
                return Err(format!("failed to open the terminal streams: {error}"));
            }
        };
        let host = Arc::new(Self {
            token: spec.token.clone(),
            killer: Mutex::new(child.clone_killer()),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            state: Mutex::new(HostState {
                history: OutputHistory::default(),
                history_lines: spec.history_lines.clamp(1, MAX_SCROLLBACK_LINES),
                client: None,
                next_client: 0,
                attached_once: false,
                cols: spec.cols,
                rows: spec.rows,
                last_activity_ms: now_ms(),
                persist: spec.persist,
                exited: false,
            }),
        });
        Ok((host, child, reader))
    }

    /// Blocks until the shell exited.
    fn serve(
        self: &Arc<Self>,
        listener: TcpListener,
        mut child: Box<dyn Child + Send + Sync>,
        reader: Box<dyn Read + Send>,
    ) {
        let host = Arc::clone(self);
        thread::spawn(move || host.pump_output(reader));
        let host = Arc::clone(self);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let host = Arc::clone(&host);
                thread::spawn(move || host.handle_client(stream));
            }
        });
        let host = Arc::clone(self);
        thread::spawn(move || {
            thread::sleep(FIRST_ATTACH_TIMEOUT);
            let orphaned = host
                .state
                .lock()
                .is_ok_and(|state| !state.attached_once && !state.persist);
            if orphaned {
                host.kill();
            }
        });
        let _ = child.wait();
        thread::sleep(EXIT_FLUSH_DELAY);
        if let Ok(mut state) = self.state.lock() {
            state.exited = true;
            state.send(&HostEvent::Exit);
            if let Some((_, stream)) = state.client.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
    }

    fn kill(&self) {
        if let Ok(mut killer) = self.killer.lock() {
            let _ = killer.kill();
        }
    }

    fn pump_output(&self, mut reader: Box<dyn Read + Send>) {
        let mut buffer = [0_u8; 8192];
        loop {
            let size = match reader.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(size) => size,
            };
            let data = String::from_utf8_lossy(&buffer[..size]).into_owned();
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.last_activity_ms = now_ms();
            let limit = state.history_lines;
            state.history.push(data.clone(), limit);
            let delivered = state.send(&HostEvent::Output { data });
            let abandon = !delivered && !state.persist;
            drop(state);
            if abandon {
                self.kill();
            }
        }
    }

    fn handle_client(&self, stream: TcpStream) {
        let Some((id, reader)) = self.accept_client(stream) else {
            return;
        };
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            match serde_json::from_str::<HostRequest>(&line) {
                Ok(HostRequest::Input { data }) => {
                    if let Ok(mut writer) = self.writer.lock() {
                        let _ = writer.write_all(&data);
                        let _ = writer.flush();
                    }
                    self.touch();
                }
                Ok(HostRequest::Resize { cols, rows }) => self.resize(cols, rows),
                Ok(HostRequest::Persist { value }) => {
                    if let Ok(mut state) = self.state.lock() {
                        state.persist = value;
                    }
                }
                Ok(HostRequest::HistoryLimit { lines }) => {
                    if let Ok(mut state) = self.state.lock() {
                        state.history_lines = lines.clamp(1, MAX_SCROLLBACK_LINES);
                        let limit = state.history_lines;
                        state.history.trim(limit);
                    }
                }
                Ok(HostRequest::Close) => self.kill(),
                Ok(HostRequest::Attach { .. }) | Err(_) => {}
            }
        }
        let abandon = self.state.lock().is_ok_and(|mut state| {
            if state
                .client
                .as_ref()
                .is_some_and(|(client, _)| *client == id)
            {
                state.client = None;
                !state.persist
            } else {
                false
            }
        });
        if abandon {
            self.kill();
        }
    }

    /// Checks the token and makes this connection the attached server.
    fn accept_client(&self, stream: TcpStream) -> Option<(u64, BufReader<TcpStream>)> {
        // Timeouts belong to the handle they are set on (a clone has its
        // own), so the handshake timeout goes on the reading handle and is
        // lifted there: otherwise a quiet terminal would drop after it.
        let mut reader = BufReader::new(stream.try_clone().ok()?);
        reader
            .get_ref()
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        match serde_json::from_str::<HostRequest>(&line) {
            Ok(HostRequest::Attach { token }) if token == self.token => {}
            _ => return None,
        }
        reader.get_ref().set_read_timeout(None).ok()?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT)).ok()?;
        let _ = stream.set_nodelay(true);
        let mut state = self.state.lock().ok()?;
        let mut writer = stream;
        if state.exited {
            let _ = write_line(&mut writer, &HostEvent::Exit);
            return None;
        }
        if state.client.is_some() {
            let _ = write_line(&mut writer, &HostEvent::Busy);
            return None;
        }
        let hello = HostEvent::Hello {
            cols: state.cols,
            rows: state.rows,
            last_activity_ms: state.last_activity_ms,
            history: state.history.snapshot(),
        };
        write_line(&mut writer, &hello).ok()?;
        let id = state.next_client;
        state.next_client += 1;
        state.attached_once = true;
        state.client = Some((id, writer));
        drop(state);
        Some((id, reader))
    }

    fn touch(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.last_activity_ms = now_ms();
        }
    }

    fn resize(&self, cols: u16, rows: u16) {
        if let Ok(master) = self.master.lock() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        if let Ok(mut state) = self.state.lock() {
            state.cols = cols;
            state.rows = rows;
            state.last_activity_ms = now_ms();
        }
    }
}

/// The server's end of a host connection.
pub struct HostConnection {
    pid: u32,
    stream: Mutex<TcpStream>,
    /// Where to attach again when the connection drops.
    directory: PathBuf,
    token: String,
}

/// Events after the `Hello`; ends when the host exits or goes away.
pub struct HostEvents(BufReader<TcpStream>);

impl Iterator for HostEvents {
    type Item = HostEvent;

    fn next(&mut self) -> Option<HostEvent> {
        loop {
            let mut line = String::new();
            match self.0.read_line(&mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    if let Ok(event) = serde_json::from_str(&line) {
                        return Some(event);
                    }
                }
            }
        }
    }
}

/// An attached host: its connection, greeting and event stream.
pub struct Attached {
    pub connection: HostConnection,
    pub cols: u16,
    pub rows: u16,
    pub last_activity_ms: i64,
    pub history: Vec<String>,
    pub events: HostEvents,
}

impl HostConnection {
    /// # Errors
    ///
    /// When the host is gone.
    pub fn send(&self, request: &HostRequest) -> io::Result<()> {
        let mut stream = self
            .stream
            .lock()
            .map_err(|_| io::Error::other("host connection poisoned"))?;
        write_line(&mut stream, request)
    }

    /// Ends the shell; kills the host when it no longer listens.
    pub fn close(&self) {
        if self.send(&HostRequest::Close).is_err() {
            terminate(self.pid);
        }
    }

    /// Attaches again after the connection dropped without the shell
    /// exiting, and returns the new event stream; `None` when the host is
    /// gone. Input sent meanwhile is lost.
    #[must_use]
    pub fn reconnect(&self) -> Option<HostEvents> {
        if let Ok(stream) = self.stream.lock() {
            // Lets the host see the old connection end, so it is not busy.
            let _ = stream.shutdown(Shutdown::Both);
        }
        let deadline = Instant::now() + RECONNECT_TIMEOUT;
        loop {
            match attach(&self.directory, &self.token) {
                Ok(attached) => {
                    let stream = attached.connection.stream.into_inner().ok()?;
                    *self.stream.lock().ok()? = stream;
                    return Some(attached.events);
                }
                // A host removes its directory when the shell exits.
                Err(_) if Instant::now() < deadline && self.directory.is_dir() => {
                    thread::sleep(Duration::from_millis(250));
                }
                Err(_) => return None,
            }
        }
    }
}

fn sessions_dir() -> PathBuf {
    PathBuf::from(SESSIONS_DIR)
}

/// Replaces the server's `session` metadata in the spec of session `id`, so
/// a restarted server restores the change (e.g. a renamed tab). The host
/// itself only reads the spec when it starts.
///
/// # Errors
///
/// When the session directory is gone or the spec cannot be rewritten.
pub fn save_session_metadata(id: &str, session: serde_json::Value) -> Result<(), String> {
    let directory = sessions_dir().join(id);
    let path = directory.join(SPEC_FILE);
    let mut spec: HostSpec = fs::read(&path)
        .map_err(|error| format!("failed to read session spec: {error}"))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| format!("invalid session spec: {error}"))
        })?;
    spec.session = session;
    let bytes = serde_json::to_vec(&spec).map_err(|error| error.to_string())?;
    let partial = directory.join(format!("{SPEC_FILE}.partial"));
    fs::write(&partial, bytes)
        .and_then(|()| fs::rename(&partial, &path))
        .map_err(|error| format!("failed to write session spec: {error}"))
}

/// Starts a host for session `id` and attaches to it.
///
/// # Errors
///
/// When the host cannot be started or did not come up.
pub fn spawn(id: &str, spec: &HostSpec) -> Result<Attached, String> {
    let directory = sessions_dir().join(id);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("failed to create the session directory: {error}"))?;
    let result = start_host(&directory, spec).and_then(|()| {
        attach(&directory, &spec.token).map_err(|error| match error {
            AttachError::Gone(error) => error,
            AttachError::Busy => "the session host is in use".to_string(),
        })
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}

fn start_host(directory: &Path, spec: &HostSpec) -> Result<(), String> {
    let bytes = serde_json::to_vec(spec).map_err(|error| error.to_string())?;
    fs::write(directory.join(SPEC_FILE), bytes)
        .map_err(|error| format!("failed to write the session spec: {error}"))?;
    let directory = directory
        .canonicalize()
        .map(|directory| simplify_verbatim_path(&directory))
        .map_err(|error| format!("failed to resolve the session directory: {error}"))?;
    let mut child = spawn_detached(&host_executable()?, &directory)
        .map_err(|error| format!("failed to start the session host: {error}"))?;
    let deadline = Instant::now() + START_TIMEOUT;
    let result = loop {
        if let Some(info) = read_info(&directory) {
            break info.error.map_or(Ok(()), Err);
        }
        if let Ok(Some(status)) = child.try_wait() {
            break Err(format!("the session host exited with {status}"));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break Err("the session host did not start in time".to_string());
        }
        thread::sleep(Duration::from_millis(30));
    };
    // Reaps the host once it exits (Unix would keep a zombie otherwise).
    thread::spawn(move || {
        let _ = child.wait();
    });
    result
}

fn read_info(directory: &Path) -> Option<HostInfo> {
    serde_json::from_slice(&fs::read(directory.join(INFO_FILE)).ok()?).ok()
}

#[derive(Debug)]
pub enum AttachError {
    /// The host is not running (any more).
    Gone(String),
    Busy,
}

impl From<String> for AttachError {
    fn from(error: String) -> Self {
        Self::Gone(error)
    }
}

impl From<&str> for AttachError {
    fn from(error: &str) -> Self {
        Self::Gone(error.to_string())
    }
}

fn attach(directory: &Path, token: &str) -> Result<Attached, AttachError> {
    let info = read_info(directory).ok_or("the session host is not running")?;
    let port = info.port.ok_or("the session host is not listening")?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, HANDSHAKE_TIMEOUT)
        .map_err(|error| format!("failed to reach the session host: {error}"))?;
    let _ = stream.set_nodelay(true);
    stream
        .set_write_timeout(Some(WRITE_TIMEOUT))
        .map_err(|error| error.to_string())?;
    write_line(
        &mut stream,
        &HostRequest::Attach {
            token: token.to_string(),
        },
    )
    .map_err(|error| format!("failed to attach to the session host: {error}"))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("failed to attach to the session host: {error}"))?,
    );
    // Only the greeting may time out; the events that follow can be far
    // apart (see `Host::accept_client`).
    reader
        .get_ref()
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|error| format!("the session host did not answer: {error}"))?;
    let (cols, rows, last_activity_ms, history) = match serde_json::from_str(&line) {
        Ok(HostEvent::Hello {
            cols,
            rows,
            last_activity_ms,
            history,
        }) => (cols, rows, last_activity_ms, history),
        Ok(HostEvent::Busy) => return Err(AttachError::Busy),
        _ => return Err("the session has ended".into()),
    };
    reader
        .get_ref()
        .set_read_timeout(None)
        .map_err(|error| error.to_string())?;
    Ok(Attached {
        connection: HostConnection {
            pid: info.pid,
            stream: Mutex::new(stream),
            directory: directory.to_path_buf(),
            token: token.to_string(),
        },
        cols,
        rows,
        last_activity_ms,
        history,
        events: HostEvents(reader),
    })
}

/// A host left by an earlier server run.
pub struct Leftover {
    pub id: String,
    pub spec: HostSpec,
    directory: PathBuf,
}

impl Leftover {
    /// Attaches, waiting a moment for a server that just stopped to let go.
    ///
    /// # Errors
    ///
    /// When the host is gone (its directory is removed then) or another
    /// server keeps it.
    pub fn attach(&self) -> Result<Attached, AttachError> {
        let deadline = Instant::now() + BUSY_TIMEOUT;
        loop {
            match attach(&self.directory, &self.spec.token) {
                Err(AttachError::Busy) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(200));
                }
                Err(AttachError::Gone(error)) => {
                    // Not killed by its recorded PID, which may be reused.
                    let _ = fs::remove_dir_all(&self.directory);
                    return Err(AttachError::Gone(error));
                }
                result => return result,
            }
        }
    }

    /// Ends the host's shell (if it still runs) and forgets the session.
    /// A host another server keeps is left alone.
    pub fn discard(&self) {
        if let Ok(attached) = self.attach() {
            attached.connection.close();
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

/// Session directories of hosts started by earlier server runs.
#[must_use]
pub fn leftovers() -> Vec<Leftover> {
    let Ok(entries) = fs::read_dir(sessions_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().join(SPEC_FILE).is_file())
        .filter_map(|entry| {
            let directory = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            let spec = fs::read(directory.join(SPEC_FILE))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok());
            let Some(spec) = spec else {
                let _ = fs::remove_dir_all(&directory);
                return None;
            };
            Some(Leftover {
                id,
                spec,
                directory,
            })
        })
        .collect()
}

/// Binary hosts run from. On Windows a running executable cannot be
/// replaced, so hosts run from a copy and rebuilding the server still works
/// while sessions are open.
fn host_executable() -> Result<PathBuf, String> {
    let current = env::current_exe()
        .map_err(|error| format!("failed to locate the server binary: {error}"))?;
    #[cfg(windows)]
    {
        let target = executable_copy(&current)
            .map_err(|error| format!("failed to read the server binary: {error}"))?;
        if !target.is_file() {
            let partial = target.with_extension(format!("{}.partial", uuid::Uuid::new_v4()));
            fs::create_dir_all(EXECUTABLES_DIR)
                .and_then(|()| fs::copy(&current, &partial))
                .map_err(|error| format!("failed to prepare the session host: {error}"))?;
            if fs::rename(&partial, &target).is_err() {
                let _ = fs::remove_file(&partial);
            }
        }
        target
            .canonicalize()
            .map(|target| simplify_verbatim_path(&target))
            .map_err(|error| format!("failed to prepare the session host: {error}"))
    }
    #[cfg(not(windows))]
    Ok(current)
}

/// Copy of `current` for this build, named by its size and modification time.
#[cfg(windows)]
fn executable_copy(current: &Path) -> io::Result<PathBuf> {
    let metadata = fs::metadata(current)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_secs());
    let stem = current
        .file_stem()
        .map_or_else(|| "server".into(), |stem| stem.to_string_lossy());
    Ok(
        PathBuf::from(EXECUTABLES_DIR)
            .join(format!("{stem}-{:x}-{modified:x}.exe", metadata.len())),
    )
}

/// Removes host binaries of earlier builds; copies running hosts use stay
/// locked and are tried again next time.
pub fn remove_stale_executables() {
    #[cfg(windows)]
    {
        let current = env::current_exe()
            .and_then(|current| executable_copy(&current))
            .ok();
        let Ok(entries) = fs::read_dir(EXECUTABLES_DIR) else {
            return;
        };
        for entry in entries.flatten() {
            if current.as_ref().and_then(|path| path.file_name())
                != Some(entry.file_name().as_os_str())
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Starts the host outside this process's console, process group and job, so
/// stopping the server does not take it down.
fn spawn_detached(executable: &Path, directory: &Path) -> io::Result<std::process::Child> {
    let mut command = Command::new(executable);
    command
        .arg(HOST_ARG)
        .arg(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const ERROR_ACCESS_DENIED: i32 = 5;
        // Every inheritable handle leaks into children on Windows; a host
        // holding the server's output pipes would keep a caller reading them
        // waiting (see `rdp_gateway::spawn_detached_wsl`).
        for handle in [
            io::stdin().as_raw_handle(),
            io::stdout().as_raw_handle(),
            io::stderr().as_raw_handle(),
        ] {
            if !handle.is_null() {
                // SAFETY: the standard handles belong to this process and
                // stay open; only their inherit flag changes.
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
            }
        }
        command.creation_flags(
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB,
        );
        match command.spawn() {
            // The job the server runs in (e.g. a scheduled task) forbids
            // breaking away; the host then ends when that job is stopped.
            Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
                command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
                command.spawn()
            }
            result => result,
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches no memory; a new
        // session leaves the terminal's and the service's process group.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn()
    }
}

/// Kills a host that no longer answers, with its shell.
fn terminate(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(unix)]
    {
        // The shell gets SIGHUP once the host's PTY closes.
        if let Ok(pid) = i32::try_from(pid) {
            // SAFETY: kill takes integer arguments only; pid is a host this
            // server started.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        write_line, Host, HostEvent, HostRequest, HostSpec, ShellCommand, TcpListener, TcpStream,
    };
    use std::{
        io::{BufRead, BufReader},
        net::Ipv4Addr,
        thread,
        time::Duration,
    };

    fn echo_command() -> ShellCommand {
        #[cfg(windows)]
        {
            let mut command = ShellCommand::new("cmd.exe");
            command.arg("/Q");
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = ShellCommand::new("/bin/sh");
            command.arg("-i");
            command
        }
    }

    fn start(persist: bool) -> (thread::JoinHandle<()>, u16) {
        let spec = HostSpec {
            token: "secret".to_string(),
            command: echo_command(),
            cols: 80,
            rows: 24,
            persist,
            history_lines: super::MAX_SCROLLBACK_LINES,
            session: serde_json::Value::Null,
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (host, child, reader) = Host::start(&spec).unwrap();
        let serve = thread::spawn(move || host.serve(listener, child, reader));
        (serve, port)
    }

    fn connect(port: u16, token: &str) -> (TcpStream, BufReader<TcpStream>) {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        write_line(
            &mut stream,
            &HostRequest::Attach {
                token: token.to_string(),
            },
        )
        .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        (stream, reader)
    }

    fn next_event(reader: &mut BufReader<TcpStream>) -> Option<HostEvent> {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => serde_json::from_str(&line).ok(),
        }
    }

    fn wait_for_output(reader: &mut BufReader<TcpStream>, needle: &str) -> bool {
        let mut seen = String::new();
        while let Some(event) = next_event(reader) {
            if let HostEvent::Output { data } = event {
                seen.push_str(&data);
                if seen.contains(needle) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn a_reattached_server_gets_the_history_and_the_same_shell() {
        let (serve, port) = start(true);
        let (_wrong, mut rejected) = connect(port, "wrong");
        assert!(
            next_event(&mut rejected).is_none(),
            "bad tokens are dropped"
        );

        let (mut first, mut events) = connect(port, "secret");
        assert!(matches!(
            next_event(&mut events),
            Some(HostEvent::Hello { .. })
        ));
        let input = b"echo marker-one\r\n".to_vec();
        write_line(&mut first, &HostRequest::Input { data: input }).unwrap();
        assert!(wait_for_output(&mut events, "marker-one"));
        // The server goes away; a persisting host keeps the shell. Until it
        // noticed, the next server is told the host is busy.
        drop(first);
        drop(events);
        let (mut second, mut events, history) = (0..50)
            .find_map(|_| {
                let (stream, mut events) = connect(port, "secret");
                match next_event(&mut events) {
                    Some(HostEvent::Hello { history, .. }) => Some((stream, events, history)),
                    _ => {
                        thread::sleep(Duration::from_millis(100));
                        None
                    }
                }
            })
            .expect("the host accepts the next server");
        assert!(history.concat().contains("marker-one"));
        write_line(&mut second, &HostRequest::Close).unwrap();
        while let Some(event) = next_event(&mut events) {
            if matches!(event, HostEvent::Exit) {
                break;
            }
        }
        serve.join().unwrap();
    }

    #[test]
    fn history_keeps_the_newest_lines_and_replays_them_in_few_pieces() {
        use super::{scrollback_lines, OutputHistory, MAX_SCROLLBACK_LINES};

        assert_eq!(scrollback_lines(0), MAX_SCROLLBACK_LINES);
        assert_eq!(scrollback_lines(-5), MAX_SCROLLBACK_LINES);
        assert_eq!(scrollback_lines(2_000), 2_000);
        assert_eq!(scrollback_lines(10_000_000), MAX_SCROLLBACK_LINES);

        let mut history = OutputHistory::default();
        for line in 0..1_000 {
            history.push(format!("line {line}\n"), 100);
        }
        let text = history.snapshot().concat();
        assert_eq!(text.lines().count(), 100);
        assert!(text.starts_with("line 900\n") && text.ends_with("line 999\n"));
        // A thousand keystroke-sized chunks replay as one message.
        assert_eq!(history.snapshot().len(), 1);

        // Output that never breaks lines is still bounded.
        let mut progress = OutputHistory::default();
        for step in 0..10_000 {
            progress.push(format!("\r{step:>40}%"), 10);
        }
        assert!(progress.snapshot().concat().len() <= 10 * 256 + 64);

        // Lowering the limit trims what is kept.
        history.trim(10);
        assert_eq!(history.snapshot().concat().lines().count(), 10);
    }

    /// The handshake timeout must not stay on the connection: a terminal
    /// that is quiet for a while (someone typing a long command) is not gone.
    #[test]
    fn a_quiet_session_outlives_the_handshake_timeout() {
        let (serve, port) = start(true);
        let directory =
            std::env::temp_dir().join(format!("ssh-host-quiet-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        super::write_info(
            &directory,
            &super::HostInfo {
                pid: std::process::id(),
                port: Some(port),
                error: None,
            },
        )
        .unwrap();
        let attached = super::attach(&directory, "secret").unwrap();
        let mut events = attached.events;
        thread::sleep(super::HANDSHAKE_TIMEOUT * 2);
        attached
            .connection
            .send(&HostRequest::Input {
                data: b"echo still-here\r\n".to_vec(),
            })
            .unwrap();
        let mut seen = String::new();
        for event in events.by_ref() {
            if let HostEvent::Output { data } = event {
                seen.push_str(&data);
                if seen.contains("still-here") {
                    break;
                }
            }
        }
        assert!(seen.contains("still-here"), "the connection was dropped");
        attached.connection.close();
        serve.join().unwrap();
        let _ = std::fs::remove_dir_all(directory);
    }

    /// A dropped connection is not the end of the session: the server
    /// attaches again and keeps talking to the same shell.
    #[test]
    fn a_dropped_connection_reattaches_to_the_same_shell() {
        let (serve, port) = start(true);
        let directory =
            std::env::temp_dir().join(format!("ssh-host-reattach-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        super::write_info(
            &directory,
            &super::HostInfo {
                pid: std::process::id(),
                port: Some(port),
                error: None,
            },
        )
        .unwrap();
        let attached = super::attach(&directory, "secret").unwrap();
        let connection = attached.connection;
        let mut events = attached.events;
        connection
            .send(&HostRequest::Input {
                data: b"set MARK=kept\r\n".to_vec(),
            })
            .unwrap();
        // Something cuts the connection.
        connection
            .stream
            .lock()
            .unwrap()
            .shutdown(std::net::Shutdown::Both)
            .unwrap();
        while events.next().is_some() {}

        let mut events = connection.reconnect().expect("the host is still running");
        #[cfg(windows)]
        let probe = b"echo %MARK%-again\r\n".to_vec();
        #[cfg(not(windows))]
        let probe = b"echo $MARK-again\r\n".to_vec();
        connection
            .send(&HostRequest::Input { data: probe })
            .unwrap();
        let mut seen = String::new();
        for event in events.by_ref() {
            if let HostEvent::Output { data } = event {
                seen.push_str(&data);
                if seen.contains("kept-again") {
                    break;
                }
            }
        }
        assert!(seen.contains("kept-again"), "not the same shell: {seen}");
        connection.close();
        serve.join().unwrap();
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn a_lost_server_ends_the_shell_unless_it_persists() {
        let (serve, port) = start(false);
        let (stream, mut events) = connect(port, "secret");
        assert!(matches!(
            next_event(&mut events),
            Some(HostEvent::Hello { .. })
        ));
        drop(stream);
        drop(events);
        // Returns once the shell was killed.
        serve.join().unwrap();
    }
}
