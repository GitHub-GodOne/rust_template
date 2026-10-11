//! Catalog and runner behind the admin "项目命令" page.
//!
//! The catalog is read from the CLIs themselves, so new commands and flags
//! show up without touching this file:
//! - Loco's built-in commands come from the binary's `-h` output (Loco keeps
//!   its clap types private);
//! - the custom `service`, `vnc` and `data` commands come from their clap
//!   definitions;
//! - tasks come from `App::register_tasks`, with parameters documented in
//!   `tasks::TASK_PARAMS`.
//!
//! Only the Chinese notes and risk levels in `COMMAND_NOTES` are hand-written;
//! commands without a note are still listed and runnable.

#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{HashMap, VecDeque},
    env,
    hash::BuildHasher,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};

use chrono::{DateTime, FixedOffset, Local};
use clap::ArgAction;
use loco_rs::{app::Hooks, task::Tasks};
use regex::Regex;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
    IntoActiveModel, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    sync::{mpsc, Notify},
};

use crate::{
    app::App,
    data_manager,
    errors::{ApiError, ApiResult},
    models::_entities::{project_cli_run_logs, project_cli_runs, users},
    responses::PageResponse,
    service_manager,
    services::command_runner,
    tasks, vnc_manager,
};

/// File name of the project binary (`[[bin]]` in Cargo.toml).
const CLI_BINARY_NAME: &str = "gpt_images-cli";
/// Commands handled by this project before Loco parses the command line.
const CUSTOM_ROOTS: [&str; 3] = ["service", "vnc", "data"];
const HELP_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ACTIVE_RUNS: usize = 4;
/// Older runs (and their output) are deleted beyond this many.
const MAX_RUNS_KEPT: u64 = 500;
/// Output kept per run; the oldest lines are dropped beyond this.
const MAX_STORED_OUTPUT_BYTES: i64 = 2 * 1024 * 1024;
const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(300);
const LOG_BATCH_LINES: usize = 500;
const LOG_ROW_BYTES: usize = 16 * 1024;
const DETAIL_CHUNK_LIMIT: u64 = 1000;
const MAX_ARGUMENT_LENGTH: usize = 4096;
const MAX_EXTRA_ARGUMENTS: usize = 32;
const SECRET_MASK: &str = "******";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CliArgKind {
    /// `--flag` without a value.
    Flag,
    /// `--name <VALUE>`.
    Option,
    /// Value given by position.
    Positional,
    /// `key:value` parameter of a task.
    TaskParam,
}

// Mirrors clap's argument settings, which are independent booleans.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct CliArgSpec {
    /// Key used in `RunProjectCliParams::values`.
    pub id: String,
    pub kind: CliArgKind,
    pub long: Option<String>,
    pub short: Option<String>,
    pub value_name: Option<String>,
    pub help: String,
    pub required: bool,
    pub multiple: bool,
    /// The option may be given without a value (`--worker [<WORKER>...]`).
    pub optional_value: bool,
    pub default_value: Option<String>,
    pub possible_values: Vec<String>,
    pub secret: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CliRisk {
    Safe,
    Caution,
    Danger,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct CliCommandSpec {
    /// Words after the binary name, e.g. `["db", "migrate"]`.
    pub path: Vec<String>,
    /// `loco`, `service`, `vnc`, `data` or `task`.
    pub source: String,
    pub about: String,
    /// Chinese explanation, when one is written in `COMMAND_NOTES`.
    pub note: Option<String>,
    /// False for groups that need a subcommand.
    pub runnable: bool,
    pub risk: CliRisk,
    /// Keeps running until stopped (servers, watchers).
    pub long_running: bool,
    pub args: Vec<CliArgSpec>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectCliCatalog {
    /// Environment of this server; runs default to it.
    pub environment: String,
    pub binary: String,
    pub binary_name: String,
    pub working_directory: String,
    /// `cargo loco …` works here (a Cargo project is checked out).
    pub cargo_project: bool,
    pub commands: Vec<CliCommandSpec>,
    pub warnings: Vec<String>,
    #[schema(value_type = String, format = DateTime)]
    pub generated_at: DateTime<Local>,
}

struct CommandNote {
    path: &'static str,
    risk: CliRisk,
    long_running: bool,
    note: &'static str,
}

const fn note(path: &'static str, risk: CliRisk, note: &'static str) -> CommandNote {
    CommandNote {
        path,
        risk,
        long_running: false,
        note,
    }
}

const fn long_running(path: &'static str, note: &'static str) -> CommandNote {
    CommandNote {
        path,
        risk: CliRisk::Caution,
        long_running: true,
        note,
    }
}

/// Chinese notes and risk levels. A note on a group (`generate`) also applies
/// to its subcommands unless they have their own.
const COMMAND_NOTES: &[CommandNote] = &[
    long_running(
        "start",
        "启动应用服务。会一直运行直到停止；当前后台已占用配置端口时，请用 --port 换一个端口",
    ),
    long_running(
        "watch",
        "监听源码变化并自动重启（开发用，需要安装 cargo-watch）",
    ),
    long_running("scheduler", "运行定时调度器；不加 --list 时会一直运行"),
    note("db", CliRisk::Safe, "数据库迁移、种子数据和结构导出"),
    note("db create", CliRisk::Safe, "创建配置中的数据库"),
    note("db migrate", CliRisk::Safe, "执行所有尚未应用的数据库迁移"),
    note("db status", CliRisk::Safe, "查看每个迁移是否已应用"),
    note("db schema", CliRisk::Safe, "导出数据库结构"),
    note(
        "db down",
        CliRisk::Danger,
        "回滚最近的迁移，可能删除表和其中的数据",
    ),
    note(
        "db reset",
        CliRisk::Danger,
        "删除所有表后重新执行迁移，数据会全部丢失",
    ),
    note(
        "db truncate",
        CliRisk::Danger,
        "清空所有表的数据（保留表结构）",
    ),
    note(
        "db seed",
        CliRisk::Caution,
        "写入初始数据；勾选 --reset 会先清空数据库",
    ),
    note(
        "db entities",
        CliRisk::Caution,
        "根据数据库结构重新生成 src/models/_entities 下的源码",
    ),
    note("routes", CliRisk::Safe, "列出全部 HTTP 路由"),
    note("middleware", CliRisk::Safe, "列出启用的中间件"),
    note(
        "task",
        CliRisk::Safe,
        "运行自定义任务；不填名称时列出全部任务",
    ),
    note(
        "task admin.recover",
        CliRisk::Caution,
        "创建管理员或重置其密码，并绑定角色",
    ),
    note(
        "task admin.unlock",
        CliRisk::Safe,
        "清除登录失败次数并解锁管理员账号",
    ),
    note(
        "task admin.sync_fixture_baseline",
        CliRisk::Caution,
        "把内置权限、菜单和角色授权同步到数据库，不会清空业务数据",
    ),
    note(
        "task operations.run_due_scheduled_tasks",
        CliRisk::Safe,
        "立即执行已到期的定时任务",
    ),
    note("jobs", CliRisk::Safe, "后台任务队列管理"),
    note("jobs cancel", CliRisk::Caution, "取消指定名称的排队任务"),
    note(
        "jobs tidy",
        CliRisk::Caution,
        "删除已完成或已取消的任务记录",
    ),
    note("jobs purge", CliRisk::Caution, "按时间删除任务记录"),
    note(
        "generate",
        CliRisk::Caution,
        "生成代码文件，会修改项目源码（仅开发构建提供）",
    ),
    note(
        "doctor",
        CliRisk::Safe,
        "检查配置以及数据库、队列等依赖能否连接",
    ),
    note("version", CliRisk::Safe, "显示应用版本"),
    note(
        "service",
        CliRisk::Safe,
        "把应用安装为系统服务：开机自启、异常自动重启",
    ),
    note(
        "service install",
        CliRisk::Caution,
        "安装并启用系统服务，通常需要管理员权限",
    ),
    note("service status", CliRisk::Safe, "查看系统服务状态"),
    note("service start", CliRisk::Safe, "启动已安装的系统服务"),
    note(
        "service stop",
        CliRisk::Danger,
        "停止系统服务；如果当前后台就是该服务，页面会断开",
    ),
    note(
        "service restart",
        CliRisk::Danger,
        "重启系统服务；如果当前后台就是该服务，页面会短暂断开",
    ),
    note(
        "service uninstall",
        CliRisk::Danger,
        "移除系统服务；如果当前后台就是该服务，会被停止",
    ),
    note("vnc", CliRisk::Safe, "检测、安装和启动本机 VNC 服务"),
    note(
        "vnc install",
        CliRisk::Caution,
        "通过 winget 安装 TightVNC（Windows）",
    ),
    note("data", CliRisk::Safe, "PostgreSQL 数据导出 / 导入"),
    note("data export", CliRisk::Safe, "把数据库导出为 .dump 文件"),
    note(
        "data import",
        CliRisk::Danger,
        "用 .dump 文件覆盖当前数据库",
    ),
];

fn command_note(path: &[String]) -> Option<&'static CommandNote> {
    // The longest matching prefix wins, so `db reset` beats `db`.
    (1..=path.len()).rev().find_map(|length| {
        let key = path[..length].join(" ");
        COMMAND_NOTES.iter().find(|note| note.path == key)
    })
}

fn annotate(mut command: CliCommandSpec) -> CliCommandSpec {
    if let Some(found) = command_note(&command.path) {
        let exact = found.path == command.path.join(" ");
        command.risk = found.risk;
        command.long_running = found.long_running;
        if exact {
            command.note = Some(found.note.to_string());
        }
    }
    command
}

// ---------------------------------------------------------------------------
// `-h` output parsing (Loco's command tree)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedHelp {
    pub about: String,
    pub subcommands: Vec<String>,
    pub requires_subcommand: bool,
    pub args: Vec<CliArgSpec>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HelpSection {
    About,
    Commands,
    Arguments,
    Options,
    Other,
}

fn help_annotation() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\s*\[(default|possible values|aliases): ([^\]]*)\]").expect("valid regex")
    })
}

/// Splits `  spec   help text` into its two columns.
fn split_columns(line: &str) -> (&str, &str) {
    let trimmed = line.trim();
    trimmed.find("  ").map_or((trimmed, ""), |index| {
        (&trimmed[..index], trimmed[index..].trim())
    })
}

const fn new_arg(id: String, kind: CliArgKind) -> CliArgSpec {
    CliArgSpec {
        id,
        kind,
        long: None,
        short: None,
        value_name: None,
        help: String::new(),
        required: false,
        multiple: false,
        optional_value: false,
        default_value: None,
        possible_values: Vec::new(),
        secret: false,
    }
}

/// Moves `[default: …]` / `[possible values: …]` out of the help text.
fn apply_help(arg: &mut CliArgSpec, help: &str) {
    let raw = if arg.help.is_empty() {
        help.to_string()
    } else {
        format!("{} {help}", arg.help)
    };
    for capture in help_annotation().captures_iter(&raw) {
        match &capture[1] {
            "default" => arg.default_value = Some(capture[2].trim().to_string()),
            "possible values" => {
                arg.possible_values = capture[2]
                    .split(',')
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect();
            }
            _ => {}
        }
    }
    arg.help = help_annotation().replace_all(&raw, "").trim().to_string();
}

fn parse_option(spec: &str) -> Option<CliArgSpec> {
    let normalized = spec.replace(", ", ",");
    let (names, value) = normalized
        .split_once(' ')
        .map_or((normalized.as_str(), None), |(names, value)| {
            (names, Some(value.trim()))
        });
    let mut long = None;
    let mut short = None;
    for name in names.split(',') {
        if let Some(name) = name.strip_prefix("--") {
            long = Some(name.to_string());
        } else if let Some(name) = name.strip_prefix('-') {
            short = Some(name.to_string());
        }
    }
    if matches!(long.as_deref(), Some("help" | "version")) {
        return None;
    }
    let id = long.clone().or_else(|| short.clone())?;
    let mut arg = new_arg(
        id,
        if value.is_some() {
            CliArgKind::Option
        } else {
            CliArgKind::Flag
        },
    );
    arg.long = long;
    arg.short = short;
    if let Some(value) = value {
        arg.multiple = value.contains("...");
        arg.optional_value = value.starts_with('[');
        arg.value_name = value
            .split_once('<')
            .and_then(|(_, rest)| rest.split_once('>'))
            .map(|(name, _)| name.to_string());
    }
    Some(arg)
}

fn parse_positional(spec: &str) -> Option<CliArgSpec> {
    let multiple = spec.ends_with("...");
    let bare = spec.trim_end_matches("...");
    let required = bare.starts_with('<');
    let name = bare
        .trim_start_matches(['<', '['])
        .trim_end_matches(['>', ']']);
    if name.is_empty() {
        return None;
    }
    let mut arg = new_arg(name.to_lowercase(), CliArgKind::Positional);
    arg.value_name = Some(name.to_string());
    arg.required = required;
    arg.multiple = multiple;
    Some(arg)
}

/// Parses clap's short (`-h`) help output.
#[must_use]
pub fn parse_help(text: &str) -> ParsedHelp {
    let mut parsed = ParsedHelp::default();
    let mut about = Vec::new();
    let mut usage = "";
    let mut section = HelpSection::About;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Usage:") {
            usage = rest.trim();
            section = HelpSection::Other;
            continue;
        }
        if !line.starts_with(' ') && line.trim_end().ends_with(':') {
            section = match line.trim_end() {
                "Commands:" => HelpSection::Commands,
                "Arguments:" => HelpSection::Arguments,
                "Options:" => HelpSection::Options,
                _ => HelpSection::Other,
            };
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        match section {
            HelpSection::About => about.push(line.trim()),
            HelpSection::Commands if indent == 2 => {
                let (name, _) = split_columns(line);
                if name != "help" {
                    parsed.subcommands.push(name.to_string());
                }
            }
            HelpSection::Arguments if indent == 2 => {
                let (spec, help) = split_columns(line);
                if let Some(mut arg) = parse_positional(spec) {
                    apply_help(&mut arg, help);
                    parsed.args.push(arg);
                }
            }
            HelpSection::Options if line.trim_start().starts_with('-') => {
                let (spec, help) = split_columns(line);
                if let Some(mut arg) = parse_option(spec) {
                    apply_help(&mut arg, help);
                    parsed.args.push(arg);
                }
            }
            // A wrapped help line continues the previous entry.
            HelpSection::Arguments | HelpSection::Options if indent > 2 => {
                if let Some(arg) = parsed.args.last_mut() {
                    apply_help(arg, line.trim());
                }
            }
            _ => {}
        }
    }
    parsed.about = about.join(" ");
    // Options listed outside `[OPTIONS]` in the usage line are required.
    for word in usage.split_whitespace() {
        if word == "<COMMAND>" {
            parsed.requires_subcommand = true;
        }
        if let Some(long) = word.strip_prefix("--") {
            let long = long.split('=').next().unwrap_or(long);
            for arg in &mut parsed.args {
                if arg.long.as_deref() == Some(long) {
                    arg.required = true;
                }
            }
        }
    }
    parsed
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// The CLI binary to run. Test harnesses live in `target/<profile>/deps`,
/// so they use the sibling project binary instead of themselves.
pub fn resolve_binary() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("PROJECT_CLI_BINARY") {
        return Ok(PathBuf::from(path));
    }
    let current = env::current_exe().map_err(|error| error.to_string())?;
    let parent = current.parent();
    let in_deps = parent
        .and_then(Path::file_name)
        .is_some_and(|name| name == "deps");
    if !in_deps {
        return Ok(current);
    }
    let candidate = parent
        .and_then(Path::parent)
        .map(|dir| dir.join(format!("{CLI_BINARY_NAME}{}", env::consts::EXE_SUFFIX)));
    candidate
        .filter(|path| path.exists())
        .ok_or_else(|| format!("找不到项目可执行文件 {CLI_BINARY_NAME}"))
}

async fn read_help(binary: &Path, environment: &str, path: &[String]) -> Result<String, String> {
    let mut command = Command::new(binary);
    command
        .args(path)
        .arg("-h")
        .env("LOCO_ENV", environment)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(HELP_TIMEOUT, command.output())
        .await
        .map_err(|_| "读取帮助超时".to_string())?
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.lines().next().unwrap_or("执行失败").to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Walks Loco's command tree level by level, reading each `-h` in parallel.
async fn loco_commands(
    binary: &Path,
    environment: &str,
    warnings: &mut Vec<String>,
) -> Vec<CliCommandSpec> {
    let mut commands = Vec::new();
    let mut level: Vec<Vec<String>> = vec![Vec::new()];
    while !level.is_empty() {
        let handles: Vec<_> = level
            .iter()
            .map(|path| {
                let binary = binary.to_path_buf();
                let environment = environment.to_string();
                let path = path.clone();
                tokio::spawn(async move { read_help(&binary, &environment, &path).await })
            })
            .collect();
        let mut next = Vec::new();
        for (path, handle) in level.into_iter().zip(handles) {
            let help = match handle.await {
                Ok(result) => result,
                Err(error) => Err(error.to_string()),
            };
            let parsed = match help {
                Ok(text) => parse_help(&text),
                Err(error) => {
                    let name = if path.is_empty() {
                        CLI_BINARY_NAME.to_string()
                    } else {
                        path.join(" ")
                    };
                    warnings.push(format!("读取 `{name}` 的命令帮助失败：{error}"));
                    continue;
                }
            };
            for name in &parsed.subcommands {
                if path.is_empty() && CUSTOM_ROOTS.contains(&name.as_str()) {
                    continue;
                }
                let mut child = path.clone();
                child.push(name.clone());
                next.push(child);
            }
            if !path.is_empty() {
                commands.push(CliCommandSpec {
                    path,
                    source: "loco".to_string(),
                    about: parsed.about,
                    note: None,
                    runnable: !parsed.requires_subcommand,
                    risk: CliRisk::Safe,
                    long_running: false,
                    args: parsed.args,
                });
            }
        }
        level = next;
    }
    // Parents before children, in the order the help listed them.
    sort_tree(commands)
}

fn sort_tree(commands: Vec<CliCommandSpec>) -> Vec<CliCommandSpec> {
    fn visit(
        prefix: &[String],
        pending: &mut Vec<Option<CliCommandSpec>>,
        out: &mut Vec<CliCommandSpec>,
    ) {
        for index in 0..pending.len() {
            let is_child = pending[index].as_ref().is_some_and(|command| {
                command.path.len() == prefix.len() + 1 && command.path.starts_with(prefix)
            });
            if is_child {
                let command = pending[index].take().expect("checked above");
                let path = command.path.clone();
                out.push(command);
                visit(&path, pending, out);
            }
        }
    }
    let mut pending: Vec<Option<CliCommandSpec>> = commands.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(pending.len());
    visit(&[], &mut pending, &mut out);
    out.extend(pending.into_iter().flatten());
    out
}

fn clap_arg(arg: &clap::Arg) -> CliArgSpec {
    let takes_value = arg.get_action().takes_values();
    let kind = if arg.is_positional() {
        CliArgKind::Positional
    } else if takes_value {
        CliArgKind::Option
    } else {
        CliArgKind::Flag
    };
    let mut spec = new_arg(arg.get_id().to_string(), kind);
    spec.long = arg.get_long().map(ToString::to_string);
    spec.short = arg.get_short().map(|short| short.to_string());
    spec.value_name = arg
        .get_value_names()
        .and_then(|names| names.first())
        .map(ToString::to_string);
    spec.help = arg.get_help().map(ToString::to_string).unwrap_or_default();
    spec.required = arg.is_required_set();
    spec.multiple = matches!(arg.get_action(), ArgAction::Append)
        || arg
            .get_num_args()
            .is_some_and(|range| range.max_values() > 1);
    spec.optional_value = arg
        .get_num_args()
        .is_some_and(|range| range.min_values() == 0);
    if takes_value {
        let defaults: Vec<String> = arg
            .get_default_values()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        spec.default_value = (!defaults.is_empty()).then(|| defaults.join(","));
        spec.possible_values = arg
            .get_possible_values()
            .iter()
            .filter(|value| !value.is_hide_set())
            .map(|value| value.get_name().to_string())
            .collect();
    }
    spec
}

fn clap_commands(command: &clap::Command, path: &[String], out: &mut Vec<CliCommandSpec>) {
    let subcommands: Vec<&clap::Command> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        .collect();
    out.push(CliCommandSpec {
        path: path.to_vec(),
        source: path[0].clone(),
        about: command
            .get_about()
            .map(ToString::to_string)
            .unwrap_or_default(),
        note: None,
        runnable: subcommands.is_empty() || !command.is_subcommand_required_set(),
        risk: CliRisk::Safe,
        long_running: false,
        args: command
            .get_arguments()
            .filter(|arg| {
                !arg.is_hide_set() && !matches!(arg.get_id().as_str(), "help" | "version")
            })
            .map(clap_arg)
            .collect(),
    });
    for sub in subcommands {
        let mut child = path.to_vec();
        child.push(sub.get_name().to_string());
        clap_commands(sub, &child, out);
    }
}

/// Clap definitions of the commands this project adds in `src/bin/main.rs`.
#[must_use]
pub fn custom_commands() -> Vec<CliCommandSpec> {
    let mut out = Vec::new();
    for mut command in [
        service_manager::cli_command(),
        vnc_manager::cli_command(),
        data_manager::cli_command(),
    ] {
        command.build();
        let name = command.get_name().to_string();
        clap_commands(&command, &[name], &mut out);
    }
    out
}

/// One entry per registered task; `options` are the `task` command's own
/// options (such as `--environment`).
#[must_use]
pub fn task_commands(options: &[CliArgSpec]) -> Vec<CliCommandSpec> {
    let mut registry = Tasks::default();
    App::register_tasks(&mut registry);
    registry
        .list()
        .into_iter()
        .map(|info| {
            let mut args: Vec<CliArgSpec> = options
                .iter()
                .filter(|arg| arg.kind != CliArgKind::Positional)
                .cloned()
                .collect();
            for param in tasks::task_params(&info.name).unwrap_or_default() {
                let mut arg = new_arg(param.name.to_string(), CliArgKind::TaskParam);
                arg.value_name = Some(param.name.to_string());
                arg.help = param.help.to_string();
                arg.required = param.required;
                arg.secret = param.secret;
                args.push(arg);
            }
            CliCommandSpec {
                path: vec!["task".to_string(), info.name],
                source: "task".to_string(),
                about: info.detail,
                note: None,
                runnable: true,
                risk: CliRisk::Safe,
                long_running: false,
                args,
            }
        })
        .collect()
}

pub async fn build_catalog(environment: &str) -> ProjectCliCatalog {
    let mut warnings = Vec::new();
    let binary = resolve_binary();
    let mut commands = match &binary {
        Ok(binary) => loco_commands(binary, environment, &mut warnings).await,
        Err(error) => {
            warnings.push(error.clone());
            Vec::new()
        }
    };
    let task_options = commands
        .iter()
        .find(|command| command.path == ["task"])
        .map(|command| command.args.clone())
        .unwrap_or_default();
    // Tasks sit right after the `task` command they belong to.
    let tasks = task_commands(&task_options);
    let task_index = commands
        .iter()
        .position(|command| command.path == ["task"])
        .map_or(commands.len(), |index| index + 1);
    commands.splice(task_index..task_index, tasks);
    commands.extend(custom_commands());

    let working_directory = env::current_dir().unwrap_or_default();
    let binary = binary.unwrap_or_default();
    ProjectCliCatalog {
        environment: environment.to_string(),
        binary_name: binary.file_name().map_or_else(
            || CLI_BINARY_NAME.to_string(),
            |name| name.to_string_lossy().into_owned(),
        ),
        binary: binary.display().to_string(),
        cargo_project: working_directory.join("Cargo.toml").exists(),
        working_directory: working_directory.display().to_string(),
        commands: commands.into_iter().map(annotate).collect(),
        warnings,
        generated_at: Local::now(),
    }
}

fn catalog_cache() -> &'static tokio::sync::Mutex<Option<ProjectCliCatalog>> {
    static CACHE: OnceLock<tokio::sync::Mutex<Option<ProjectCliCatalog>>> = OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(None))
}

/// The catalog, built once per process (the binary cannot change under a
/// running server). Results with warnings are rebuilt on the next request.
pub async fn catalog(environment: &str, refresh: bool) -> ProjectCliCatalog {
    let mut cache = catalog_cache().lock().await;
    if let Some(catalog) = cache.as_ref().filter(|_| !refresh) {
        return catalog.clone();
    }
    let catalog = build_catalog(environment).await;
    *cache = catalog.warnings.is_empty().then(|| catalog.clone());
    catalog
}

// ---------------------------------------------------------------------------
// Building the argument list
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RunProjectCliParams {
    /// Command path from the catalog, e.g. `["db", "migrate"]`.
    pub path: Vec<String>,
    /// Values keyed by argument id: `true` for flags, a string or a list of
    /// strings for the rest.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub values: HashMap<String, Value>,
    /// Extra raw arguments appended at the end.
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BuiltArguments {
    pub args: Vec<String>,
    /// Same as `args` with secret values masked.
    pub display: Vec<String>,
}

fn check_text(value: &str, label: &str) -> ApiResult<()> {
    if value.len() > MAX_ARGUMENT_LENGTH {
        return Err(ApiError::bad_request(format!("{label} 过长")));
    }
    if value.contains('\0') {
        return Err(ApiError::bad_request(format!("{label} 包含非法字符")));
    }
    Ok(())
}

fn values_of(value: &Value, label: &str) -> ApiResult<Vec<String>> {
    let values = match value {
        Value::Null => Vec::new(),
        Value::String(text) => vec![text.clone()],
        Value::Number(number) => vec![number.to_string()],
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(text) => Ok(text.clone()),
                Value::Number(number) => Ok(number.to_string()),
                _ => Err(ApiError::bad_request(format!("{label} 的值必须是文本"))),
            })
            .collect::<ApiResult<_>>()?,
        _ => return Err(ApiError::bad_request(format!("{label} 的值必须是文本"))),
    };
    let values: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    for value in &values {
        check_text(value, label)?;
    }
    Ok(values)
}

fn arg_label(arg: &CliArgSpec) -> String {
    arg.long.as_ref().map_or_else(
        || {
            arg.short.as_ref().map_or_else(
                || arg.value_name.clone().unwrap_or_else(|| arg.id.clone()),
                |short| format!("-{short}"),
            )
        },
        |long| format!("--{long}"),
    )
}

fn option_name(arg: &CliArgSpec) -> String {
    arg.long.as_ref().map_or_else(
        || format!("-{}", arg.short.clone().unwrap_or_default()),
        |long| format!("--{long}"),
    )
}

/// Turns form values into argv for `command`. No shell is involved, so values
/// are passed through verbatim.
pub fn build_arguments<S: BuildHasher>(
    command: &CliCommandSpec,
    values: &HashMap<String, Value, S>,
    extra_args: &[String],
) -> ApiResult<BuiltArguments> {
    for key in values.keys() {
        if !command.args.iter().any(|arg| &arg.id == key) {
            return Err(ApiError::bad_request(format!("未知参数 {key}")));
        }
    }
    if extra_args.len() > MAX_EXTRA_ARGUMENTS {
        return Err(ApiError::bad_request("附加参数过多"));
    }
    let mut args = command.path.clone();
    let mut display = command.path.clone();
    let mut positionals: Vec<(String, String)> = Vec::new();
    let mut task_params: Vec<(String, String)> = Vec::new();
    for arg in &command.args {
        let label = arg_label(arg);
        let value = values.get(&arg.id).unwrap_or(&Value::Null);
        let mask = |text: &str| {
            if arg.secret {
                SECRET_MASK.to_string()
            } else {
                text.to_string()
            }
        };
        match arg.kind {
            CliArgKind::Flag => {
                let on = match value {
                    Value::Null => false,
                    Value::Bool(on) => *on,
                    _ => return Err(ApiError::bad_request(format!("{label} 是开关参数"))),
                };
                if on {
                    args.push(option_name(arg));
                    display.push(option_name(arg));
                }
                continue;
            }
            CliArgKind::Option if value == &Value::Bool(true) && arg.optional_value => {
                args.push(option_name(arg));
                display.push(option_name(arg));
                continue;
            }
            _ => {}
        }
        let given = values_of(value, &label)?;
        if given.is_empty() {
            if arg.required {
                return Err(ApiError::bad_request(format!("缺少必填参数 {label}")));
            }
            continue;
        }
        if given.len() > 1 && !arg.multiple {
            return Err(ApiError::bad_request(format!("{label} 只能填一个值")));
        }
        for text in given {
            match arg.kind {
                CliArgKind::Option => {
                    let name = option_name(arg);
                    // `--name=value` keeps values that start with `-` intact.
                    if arg.long.is_some() {
                        args.push(format!("{name}={text}"));
                        display.push(format!("{name}={}", mask(&text)));
                    } else {
                        args.extend([name.clone(), text.clone()]);
                        display.extend([name, mask(&text)]);
                    }
                }
                CliArgKind::Positional => {
                    if text.starts_with('-') {
                        return Err(ApiError::bad_request(format!("{label} 不能以 - 开头")));
                    }
                    positionals.push((text.clone(), mask(&text)));
                }
                CliArgKind::TaskParam => {
                    task_params.push((
                        format!("{}:{text}", arg.id),
                        format!("{}:{}", arg.id, mask(&text)),
                    ));
                }
                CliArgKind::Flag => {}
            }
        }
    }
    for (arg, shown) in positionals.into_iter().chain(task_params) {
        args.push(arg);
        display.push(shown);
    }
    for extra in extra_args {
        check_text(extra, "附加参数")?;
        if !extra.trim().is_empty() {
            args.push(extra.clone());
            display.push(extra.clone());
        }
    }
    Ok(BuiltArguments { args, display })
}

/// Joins arguments for display, quoting the ones a shell would split.
#[must_use]
pub fn format_command_line(program: &str, args: &[String]) -> String {
    let quote = |value: &str| {
        let plain = !value.is_empty()
            && value
                .chars()
                .all(|char| char.is_ascii_alphanumeric() || "_@%+=:,./-*".contains(char));
        if plain {
            value.to_string()
        } else {
            format!("'{}'", value.replace('\'', "'\\''"))
        }
    };
    std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| quote(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Runs: stored in `project_cli_runs`, output in `project_cli_run_logs`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectCliRunStatus {
    Running,
    Success,
    Failed,
    Cancelled,
}

impl ProjectCliRunStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "success" => Self::Success,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectCliOutputChunk {
    pub seq: i32,
    /// `stdout`, `stderr` or `system`.
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectCliRunSummary {
    pub id: i32,
    pub path: Vec<String>,
    /// Arguments after the binary, secrets masked.
    pub args: Vec<String>,
    pub command_line: String,
    pub environment: String,
    pub status: ProjectCliRunStatus,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub started_at: DateTime<FixedOffset>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub finished_at: Option<DateTime<FixedOffset>>,
    pub duration_ms: Option<i64>,
    pub created_by: Option<i32>,
    pub created_by_name: String,
    pub last_seq: i32,
    pub output_bytes: i64,
    /// Older output was dropped to stay under the size limit.
    pub truncated: bool,
}

impl From<project_cli_runs::Model> for ProjectCliRunSummary {
    fn from(run: project_cli_runs::Model) -> Self {
        Self {
            id: run.id,
            path: serde_json::from_str(&run.path).unwrap_or_default(),
            args: serde_json::from_str(&run.args).unwrap_or_default(),
            command_line: run.command_line,
            environment: run.environment,
            status: ProjectCliRunStatus::parse(&run.status),
            exit_code: run.exit_code,
            error: run.error,
            started_at: run.started_at,
            finished_at: run.finished_at,
            duration_ms: run.duration_ms,
            created_by: run.created_by,
            created_by_name: run.created_by_name,
            last_seq: run.last_seq,
            output_bytes: run.output_bytes,
            truncated: run.truncated,
        }
    }
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ProjectCliRunDetail {
    pub run: ProjectCliRunSummary,
    /// Output after the requested `after` sequence number.
    pub output: Vec<ProjectCliOutputChunk>,
}

/// What to execute and how to show it in the history.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub path: Vec<String>,
    pub binary: PathBuf,
    pub args: Vec<String>,
    /// `args` with secrets masked; this is what gets stored.
    pub display_args: Vec<String>,
    pub display_program: String,
    pub environment: String,
    pub working_directory: PathBuf,
}

/// Processes started by this server instance, by run id. A `running` row
/// that is not in here was interrupted by a restart.
fn live_runs() -> &'static tokio::sync::Mutex<HashMap<i32, Arc<Notify>>> {
    static LIVE: OnceLock<tokio::sync::Mutex<HashMap<i32, Arc<Notify>>>> = OnceLock::new();
    LIVE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

async fn find_run(db: &DatabaseConnection, id: i32) -> ApiResult<project_cli_runs::Model> {
    project_cli_runs::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| ApiError::not_found("运行记录不存在"))
}

/// Marks runs whose process is gone (the server restarted) as failed.
pub async fn reconcile_interrupted_runs(db: &DatabaseConnection) -> ApiResult<()> {
    let live = live_runs().lock().await;
    let ids: Vec<i32> = live.keys().copied().collect();
    let now = Local::now().fixed_offset();
    let mut query = project_cli_runs::Entity::update_many()
        .col_expr(
            project_cli_runs::Column::Status,
            Expr::value(ProjectCliRunStatus::Failed.as_str()),
        )
        .col_expr(
            project_cli_runs::Column::Error,
            Expr::value("服务重启，命令运行被中断"),
        )
        .col_expr(project_cli_runs::Column::FinishedAt, Expr::value(now))
        .filter(project_cli_runs::Column::Status.eq(ProjectCliRunStatus::Running.as_str()));
    if !ids.is_empty() {
        query = query.filter(project_cli_runs::Column::Id.is_not_in(ids));
    }
    query.exec(db).await?;
    drop(live);
    Ok(())
}

/// Keeps the newest `MAX_RUNS_KEPT` runs; their logs go with them.
async fn prune_history(db: &DatabaseConnection) -> ApiResult<()> {
    let cutoff = project_cli_runs::Entity::find()
        .order_by_desc(project_cli_runs::Column::Id)
        .offset(MAX_RUNS_KEPT)
        .one(db)
        .await?;
    if let Some(cutoff) = cutoff {
        project_cli_runs::Entity::delete_many()
            .filter(project_cli_runs::Column::Id.lte(cutoff.id))
            .filter(project_cli_runs::Column::Status.ne(ProjectCliRunStatus::Running.as_str()))
            .exec(db)
            .await?;
    }
    Ok(())
}

type OutputLine = (&'static str, String);

#[derive(Default)]
struct LogStats {
    last_seq: i32,
    bytes: i64,
    truncated: bool,
}

/// Groups consecutive lines of the same stream into one row.
fn merge_lines(lines: Vec<OutputLine>) -> Vec<OutputLine> {
    let mut rows: Vec<OutputLine> = Vec::new();
    for (stream, text) in lines {
        match rows.last_mut() {
            Some((last, buffer)) if *last == stream && buffer.len() < LOG_ROW_BYTES => {
                buffer.push_str(&text);
            }
            _ => rows.push((stream, text)),
        }
    }
    rows
}

/// Writes output in small batches and keeps at most `MAX_STORED_OUTPUT_BYTES`
/// per run by deleting the oldest rows.
async fn write_logs(
    db: DatabaseConnection,
    run_id: i32,
    mut lines: mpsc::UnboundedReceiver<OutputLine>,
) -> LogStats {
    let mut stats = LogStats::default();
    let mut stored: VecDeque<(i32, i64)> = VecDeque::new();
    while let Some(first) = lines.recv().await {
        let mut batch = vec![first];
        let deadline = tokio::time::Instant::now() + LOG_FLUSH_INTERVAL;
        while batch.len() < LOG_BATCH_LINES {
            match tokio::time::timeout_at(deadline, lines.recv()).await {
                Ok(Some(line)) => batch.push(line),
                _ => break,
            }
        }
        let now = Local::now().fixed_offset();
        let rows: Vec<project_cli_run_logs::ActiveModel> = merge_lines(batch)
            .into_iter()
            .map(|(stream, text)| {
                stats.last_seq += 1;
                let size = i64::try_from(text.len()).unwrap_or(i64::MAX);
                stats.bytes += size;
                stored.push_back((stats.last_seq, size));
                project_cli_run_logs::ActiveModel {
                    run_id: Set(run_id),
                    seq: Set(stats.last_seq),
                    stream: Set(stream.to_string()),
                    text: Set(text),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                }
            })
            .collect();
        if let Err(error) = project_cli_run_logs::Entity::insert_many(rows)
            .exec(&db)
            .await
        {
            tracing::warn!(run_id, error = %error, "failed to store project cli output");
            continue;
        }
        let mut drop_through = None;
        while stats.bytes > MAX_STORED_OUTPUT_BYTES && stored.len() > 1 {
            let Some((seq, size)) = stored.pop_front() else {
                break;
            };
            stats.bytes -= size;
            drop_through = Some(seq);
        }
        if let Some(seq) = drop_through {
            stats.truncated = true;
            let _ = project_cli_run_logs::Entity::delete_many()
                .filter(project_cli_run_logs::Column::RunId.eq(run_id))
                .filter(project_cli_run_logs::Column::Seq.lte(seq))
                .exec(&db)
                .await;
        }
        let _ = project_cli_runs::Entity::update_many()
            .col_expr(
                project_cli_runs::Column::LastSeq,
                Expr::value(stats.last_seq),
            )
            .col_expr(
                project_cli_runs::Column::OutputBytes,
                Expr::value(stats.bytes),
            )
            .col_expr(
                project_cli_runs::Column::Truncated,
                Expr::value(stats.truncated),
            )
            .filter(project_cli_runs::Column::Id.eq(run_id))
            .exec(&db)
            .await;
    }
    stats
}

async fn pump(
    stream: &'static str,
    reader: impl AsyncRead + Unpin,
    lines: mpsc::UnboundedSender<OutputLine>,
) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if lines
                    .send((stream, String::from_utf8_lossy(&line).into_owned()))
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

/// Runs the process to completion (or until cancelled) and records the result.
async fn drive(
    db: DatabaseConnection,
    run: project_cli_runs::Model,
    spec: ProcessSpec,
    cancel: Arc<Notify>,
) {
    let run_id = run.id;
    let (lines, receiver) = mpsc::unbounded_channel();
    let writer = tokio::spawn(write_logs(db.clone(), run_id, receiver));
    let outcome = execute(&spec, &lines, &cancel).await;
    let message = match &outcome {
        (_, _, Some(error)) => format!("\n{error}\n"),
        (ProjectCliRunStatus::Cancelled, _, _) => "\n已停止\n".to_string(),
        (_, Some(code), _) => format!("\n进程退出，退出码 {code}\n"),
        _ => "\n进程已结束\n".to_string(),
    };
    let _ = lines.send(("system", message));
    drop(lines);
    let log_stats = writer.await.unwrap_or_default();

    let finished_at = Local::now().fixed_offset();
    let (status, exit_code, error) = outcome;
    let mut active = run.into_active_model();
    active.status = Set(status.as_str().to_string());
    active.exit_code = Set(exit_code);
    active.error = Set(error);
    active.duration_ms = Set(Some(
        (finished_at - *active.started_at.as_ref()).num_milliseconds(),
    ));
    active.finished_at = Set(Some(finished_at));
    active.last_seq = Set(log_stats.last_seq);
    active.output_bytes = Set(log_stats.bytes);
    active.truncated = Set(log_stats.truncated);
    active.updated_at = Set(finished_at);
    if let Err(error) = active.update(&db).await {
        tracing::error!(run_id, error = %error, "failed to record project cli result");
    }
    live_runs().lock().await.remove(&run_id);
}

async fn execute(
    spec: &ProcessSpec,
    lines: &mpsc::UnboundedSender<OutputLine>,
    cancel: &Notify,
) -> (ProjectCliRunStatus, Option<i32>, Option<String>) {
    let mut command = Command::new(&spec.binary);
    command
        .args(&spec.args)
        .current_dir(&spec.working_directory)
        .env("LOCO_ENV", &spec.environment)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return (
                ProjectCliRunStatus::Failed,
                None,
                Some(format!("无法启动 {}：{error}", spec.binary.display())),
            );
        }
    };
    let process_id = child.id();
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|out| tokio::spawn(pump("stdout", out, lines.clone()))),
        child
            .stderr
            .take()
            .map(|err| tokio::spawn(pump("stderr", err, lines.clone()))),
    ]
    .into_iter()
    .flatten()
    .collect();

    let waited = tokio::select! {
        status = child.wait() => Some(status),
        () = cancel.notified() => None,
    };
    let outcome = match waited {
        Some(Ok(status)) => (
            if status.success() {
                ProjectCliRunStatus::Success
            } else {
                ProjectCliRunStatus::Failed
            },
            status.code(),
            None,
        ),
        Some(Err(error)) => (
            ProjectCliRunStatus::Failed,
            None,
            Some(format!("等待进程结束失败：{error}")),
        ),
        None => {
            let error = command_runner::terminate_process_tree(&mut child, process_id)
                .await
                .err();
            (ProjectCliRunStatus::Cancelled, None, error)
        }
    };
    // Background grandchildren may keep the pipes open; don't wait forever.
    for reader in readers {
        let abort = reader.abort_handle();
        if tokio::time::timeout(Duration::from_secs(2), reader)
            .await
            .is_err()
        {
            abort.abort();
        }
    }
    outcome
}

/// Records a run and starts its process in the background.
pub async fn spawn_run(
    db: &DatabaseConnection,
    spec: ProcessSpec,
    user: Option<&users::Model>,
) -> ApiResult<ProjectCliRunSummary> {
    let mut live = live_runs().lock().await;
    if live.len() >= MAX_ACTIVE_RUNS {
        return Err(ApiError::bad_request(format!(
            "最多同时运行 {MAX_ACTIVE_RUNS} 个命令，请先停止或等待其他命令结束"
        )));
    }
    let now = Local::now().fixed_offset();
    let to_json = |value: &[String]| serde_json::to_string(value).unwrap_or_else(|_| "[]".into());
    let run = project_cli_runs::ActiveModel {
        path: Set(to_json(&spec.path)),
        args: Set(to_json(&spec.display_args)),
        command_line: Set(format_command_line(
            &spec.display_program,
            &spec.display_args,
        )),
        environment: Set(spec.environment.clone()),
        status: Set(ProjectCliRunStatus::Running.as_str().to_string()),
        started_at: Set(now),
        created_by: Set(user.map(|user| user.id)),
        created_by_name: Set(user.map(|user| user.name.clone()).unwrap_or_default()),
        last_seq: Set(0),
        output_bytes: Set(0),
        truncated: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    let cancel = Arc::new(Notify::new());
    live.insert(run.id, cancel.clone());
    drop(live);

    tracing::info!(
        run_id = run.id,
        user_id = run.created_by,
        command = run.command_line,
        "project cli command started"
    );
    let summary = ProjectCliRunSummary::from(run.clone());
    tokio::spawn(drive(db.clone(), run, spec, cancel));
    if let Err(error) = prune_history(db).await {
        tracing::warn!(
            error = error.message(),
            "failed to prune project cli history"
        );
    }
    Ok(summary)
}

pub async fn start_run(
    db: &DatabaseConnection,
    environment: &str,
    user: &users::Model,
    params: &RunProjectCliParams,
) -> ApiResult<ProjectCliRunSummary> {
    let catalog = catalog(environment, false).await;
    let command = catalog
        .commands
        .iter()
        .find(|command| command.path == params.path)
        .ok_or_else(|| ApiError::bad_request("命令不存在，请刷新命令列表"))?;
    if !command.runnable {
        return Err(ApiError::bad_request("请选择一个具体的子命令"));
    }
    let binary = resolve_binary().map_err(ApiError::bad_request)?;
    let built = build_arguments(command, &params.values, &params.extra_args)?;
    let working_directory = env::current_dir()
        .map_err(|error| ApiError::internal(format!("无法读取工作目录：{error}")))?;
    spawn_run(
        db,
        ProcessSpec {
            path: command.path.clone(),
            binary,
            args: built.args,
            display_args: built.display,
            display_program: catalog.binary_name.clone(),
            environment: environment.to_string(),
            working_directory,
        },
        Some(user),
    )
    .await
}

pub async fn list_runs(
    db: &DatabaseConnection,
    page: u64,
    page_size: u64,
) -> ApiResult<PageResponse<ProjectCliRunSummary>> {
    reconcile_interrupted_runs(db).await?;
    let paginator = project_cli_runs::Entity::find()
        .order_by_desc(project_cli_runs::Column::Id)
        .paginate(db, page_size);
    let total = paginator.num_items().await?;
    let items = paginator
        .fetch_page(page.saturating_sub(1))
        .await?
        .into_iter()
        .map(ProjectCliRunSummary::from)
        .collect();
    Ok(PageResponse {
        items,
        page,
        page_size,
        total,
    })
}

pub async fn run_detail(
    db: &DatabaseConnection,
    id: i32,
    after: i32,
) -> ApiResult<ProjectCliRunDetail> {
    let mut run = find_run(db, id).await?;
    if run.status == ProjectCliRunStatus::Running.as_str() {
        reconcile_interrupted_runs(db).await?;
        run = find_run(db, id).await?;
    }
    let output = project_cli_run_logs::Entity::find()
        .filter(project_cli_run_logs::Column::RunId.eq(id))
        .filter(project_cli_run_logs::Column::Seq.gt(after))
        .order_by_asc(project_cli_run_logs::Column::Seq)
        .limit(DETAIL_CHUNK_LIMIT)
        .all(db)
        .await?
        .into_iter()
        .map(|chunk| ProjectCliOutputChunk {
            seq: chunk.seq,
            stream: chunk.stream,
            text: chunk.text,
        })
        .collect();
    Ok(ProjectCliRunDetail {
        run: run.into(),
        output,
    })
}

pub async fn cancel_run(db: &DatabaseConnection, id: i32) -> ApiResult<ProjectCliRunSummary> {
    let signal = live_runs().lock().await.get(&id).cloned();
    match signal {
        Some(cancel) => cancel.notify_one(),
        None => reconcile_interrupted_runs(db).await?,
    }
    Ok(find_run(db, id).await?.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_HELP: &str = "Start an app

Usage: gpt_images-cli start [OPTIONS]

Options:
  -w, --worker [<WORKER>...]       Start worker. Optionally provide tags to run specific jobs (e.g. --worker=tag1,tag2)
  -s, --server-and-worker          Start the server and worker in the same process
  -p, --port <PORT>                server port address
  -n, --no-banner                  disable the banner display
  -e, --environment <ENVIRONMENT>  Specify the environment [default: development]
  -h, --help                       Print help
  -V, --version                    Print version
";

    const DB_HELP: &str = "Perform DB operations

Usage: gpt_images-cli db [OPTIONS] <COMMAND>

Commands:
  create    Create schema
  migrate   Migrate schema (up)
  help      Print this message or the help of the given subcommand(s)

Options:
  -e, --environment <ENVIRONMENT>  Specify the environment [default: development]
  -h, --help                       Print help
";

    const MODEL_HELP: &str = "Generates a new model file

Usage: gpt_images-cli generate model [OPTIONS] <NAME> [FIELDS]...

Arguments:
  <NAME>       Name of the thing to generate
  [FIELDS]...  Model fields, eg. title:string hits:int

Options:
      --without-tz                 Generate model without timestamps
  -h, --help                       Print help

Examples:
  - Generate empty model:
      $ cargo loco g model posts
";

    const PURGE_HELP: &str = "Deletes jobs

Usage: gpt_images-cli jobs cancel [OPTIONS] --name <NAME>

Options:
      --name <NAME>                Names of jobs to cancel
      --status <STATUS>            Limits the jobs [possible values: queued, processing, completed]
      --max-age <MAX_AGE>          Deletes jobs older than
                                   the given days [default: 90]
  -h, --help                       Print help
";

    #[test]
    fn parses_options_and_flags() {
        let parsed = parse_help(START_HELP);
        assert_eq!(parsed.about, "Start an app");
        assert!(!parsed.requires_subcommand);
        let ids: Vec<&str> = parsed.args.iter().map(|arg| arg.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "worker",
                "server-and-worker",
                "port",
                "no-banner",
                "environment"
            ]
        );
        let worker = &parsed.args[0];
        assert_eq!(worker.kind, CliArgKind::Option);
        assert!(worker.multiple && worker.optional_value);
        assert_eq!(worker.short.as_deref(), Some("w"));
        assert_eq!(worker.value_name.as_deref(), Some("WORKER"));
        assert_eq!(parsed.args[1].kind, CliArgKind::Flag);
        let environment = &parsed.args[4];
        assert_eq!(environment.default_value.as_deref(), Some("development"));
        assert_eq!(environment.help, "Specify the environment");
    }

    #[test]
    fn parses_subcommands_positionals_and_sections() {
        let db = parse_help(DB_HELP);
        assert!(db.requires_subcommand);
        assert_eq!(db.subcommands, ["create", "migrate"]);

        let model = parse_help(MODEL_HELP);
        assert_eq!(model.args.len(), 3, "the Examples section is ignored");
        assert_eq!(model.args[0].id, "name");
        assert!(model.args[0].required && !model.args[0].multiple);
        assert_eq!(model.args[1].id, "fields");
        assert!(!model.args[1].required && model.args[1].multiple);
        assert_eq!(model.args[2].long.as_deref(), Some("without-tz"));
    }

    #[test]
    fn reads_required_options_possible_values_and_wrapped_help() {
        let parsed = parse_help(PURGE_HELP);
        assert!(parsed.args[0].required, "--name is outside [OPTIONS]");
        assert_eq!(
            parsed.args[1].possible_values,
            ["queued", "processing", "completed"]
        );
        assert_eq!(
            parsed.args[2].help,
            "Deletes jobs older than the given days"
        );
        assert_eq!(parsed.args[2].default_value.as_deref(), Some("90"));
    }

    #[test]
    fn custom_commands_come_from_clap() {
        let commands = custom_commands();
        let install = commands
            .iter()
            .find(|command| command.path == ["service", "install"])
            .expect("service install");
        let mode = install
            .args
            .iter()
            .find(|arg| arg.id == "mode")
            .expect("--mode");
        assert_eq!(mode.default_value.as_deref(), Some("server"));
        assert_eq!(mode.possible_values, ["server", "server-and-worker", "all"]);
        let service = commands
            .iter()
            .find(|command| command.path == ["service"])
            .unwrap();
        assert!(!service.runnable, "service needs a subcommand");
        let import = commands
            .iter()
            .find(|command| command.path == ["data", "import"])
            .unwrap();
        assert!(import
            .args
            .iter()
            .any(|arg| arg.id == "file" && arg.required));
    }

    #[test]
    fn every_custom_command_in_main_is_listed() {
        let main = include_str!("../bin/main.rs");
        let handlers = main.matches("::handle_if_requested()").count();
        let roots: Vec<String> = custom_commands()
            .into_iter()
            .filter(|command| command.path.len() == 1)
            .map(|command| command.path[0].clone())
            .collect();
        assert_eq!(
            handlers,
            roots.len(),
            "src/bin/main.rs handles a command that project_cli::custom_commands() does not list"
        );
        assert_eq!(roots, CUSTOM_ROOTS);
    }

    #[test]
    fn tasks_list_their_parameters() {
        let environment = parse_help(START_HELP).args.pop().unwrap();
        let commands = task_commands(&[environment]);
        let recover = commands
            .iter()
            .find(|command| command.path == ["task", "admin.recover"])
            .unwrap();
        assert_eq!(recover.args[0].id, "environment");
        let password = recover
            .args
            .iter()
            .find(|arg| arg.id == "password")
            .unwrap();
        assert!(password.required && password.secret);
    }

    #[test]
    fn notes_match_the_longest_prefix() {
        let path = |words: &[&str]| words.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            command_note(&path(&["db", "reset"])).unwrap().risk,
            CliRisk::Danger
        );
        assert_eq!(
            command_note(&path(&["generate", "model"])).unwrap().path,
            "generate"
        );
        assert!(command_note(&path(&["start"])).unwrap().long_running);
        assert!(command_note(&path(&["unknown"])).is_none());
    }

    #[test]
    fn builds_arguments_from_values() {
        let mut command = annotate(CliCommandSpec {
            path: vec!["start".into()],
            source: "loco".into(),
            about: String::new(),
            note: None,
            runnable: true,
            risk: CliRisk::Safe,
            long_running: false,
            args: parse_help(START_HELP).args,
        });
        let values = HashMap::from([
            ("port".to_string(), Value::String("5160".into())),
            ("no-banner".to_string(), Value::Bool(true)),
            ("server-and-worker".to_string(), Value::Bool(false)),
            ("worker".to_string(), Value::Bool(true)),
        ]);
        let built = build_arguments(&command, &values, &[]).unwrap();
        assert_eq!(
            built.args,
            ["start", "--worker", "--port=5160", "--no-banner"]
        );

        let unknown = HashMap::from([("nope".to_string(), Value::Bool(true))]);
        assert!(build_arguments(&command, &unknown, &[]).is_err());

        command.args[2].required = true;
        assert!(build_arguments(&command, &HashMap::new(), &[]).is_err());
    }

    #[test]
    fn masks_secret_task_parameters() {
        let commands = task_commands(&[]);
        let recover = commands
            .iter()
            .find(|command| command.path == ["task", "admin.recover"])
            .unwrap();
        let values = HashMap::from([
            ("email".to_string(), Value::String("a@b.c".into())),
            ("password".to_string(), Value::String("p4ss word".into())),
        ]);
        let built = build_arguments(recover, &values, &[]).unwrap();
        assert_eq!(
            built.args,
            ["task", "admin.recover", "email:a@b.c", "password:p4ss word"]
        );
        assert_eq!(built.display[3], "password:******");
        assert_eq!(
            format_command_line("cli", &built.display),
            "cli task admin.recover email:a@b.c password:******"
        );
        assert_eq!(
            format_command_line("cli", &["a b".into(), "it's".into()]),
            "cli 'a b' 'it'\\''s'"
        );

        let missing = HashMap::from([("email".to_string(), Value::String("a@b.c".into()))]);
        assert!(build_arguments(recover, &missing, &[]).is_err());
    }

    #[test]
    fn output_lines_are_merged_per_stream() {
        let rows = merge_lines(vec![
            ("stdout", "a\n".into()),
            ("stdout", "b\n".into()),
            ("stderr", "c\n".into()),
            ("stdout", "d\n".into()),
        ]);
        assert_eq!(
            rows,
            [
                ("stdout", "a\nb\n".to_string()),
                ("stderr", "c\n".to_string()),
                ("stdout", "d\n".to_string())
            ]
        );
    }

    #[test]
    fn positionals_follow_options() {
        let command = CliCommandSpec {
            path: vec!["generate".into(), "model".into()],
            source: "loco".into(),
            about: String::new(),
            note: None,
            runnable: true,
            risk: CliRisk::Caution,
            long_running: false,
            args: parse_help(MODEL_HELP).args,
        };
        let values = HashMap::from([
            ("name".to_string(), Value::String("posts".into())),
            (
                "fields".to_string(),
                Value::Array(vec!["title:string".into(), "hits:int".into()]),
            ),
            ("without-tz".to_string(), Value::Bool(true)),
        ]);
        let built = build_arguments(&command, &values, &["--help".to_string()]).unwrap();
        assert_eq!(
            built.args,
            [
                "generate",
                "model",
                "--without-tz",
                "posts",
                "title:string",
                "hits:int",
                "--help"
            ]
        );
        let dashed = HashMap::from([("name".to_string(), Value::String("-x".into()))]);
        assert!(build_arguments(&command, &dashed, &[]).is_err());
    }
}
