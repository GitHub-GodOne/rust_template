#![allow(clippy::missing_errors_doc)]

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use clap::{error::ErrorKind, Args, Parser, Subcommand, ValueEnum};
use loco_rs::{Error, Result};

const DEFAULT_SERVICE_NAME: &str = "gpt-images";
#[cfg(any(test, target_os = "macos"))]
const SERVICE_LABEL_PREFIX: &str = "top.pikju";

#[derive(Debug, Parser)]
#[command(
    name = "service",
    about = "Install and control the application as an operating-system service",
    disable_help_subcommand = true
)]
struct ServiceCli {
    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Install autostart and restart-on-failure configuration
    #[command(alias = "enable")]
    Install(InstallArgs),
    /// Remove the installed service configuration
    #[command(alias = "disable")]
    Uninstall(TargetArgs),
    /// Start the installed service
    Start(TargetArgs),
    /// Stop the installed service
    Stop(TargetArgs),
    /// Restart the installed service
    Restart(TargetArgs),
    /// Show native service-manager status
    Status(TargetArgs),
}

#[derive(Debug, Clone, Args)]
struct TargetArgs {
    /// Native service name
    #[arg(long, default_value = DEFAULT_SERVICE_NAME)]
    name: String,

    /// Install a per-user service instead of a system-wide service
    #[arg(long)]
    user: bool,
}

#[derive(Debug, Args)]
struct InstallArgs {
    #[command(flatten)]
    target: TargetArgs,

    /// Loco environment stored in the service command
    #[arg(short = 'e', long)]
    environment: Option<String>,

    /// Override the configured server port
    #[arg(short, long)]
    port: Option<u16>,

    /// Override the configured bind address
    #[arg(short, long)]
    binding: Option<String>,

    /// Components started by the service
    #[arg(long, value_enum, default_value_t = ServiceStartMode::Server)]
    mode: ServiceStartMode,

    /// Application working directory; defaults to the current directory
    #[arg(long)]
    working_directory: Option<PathBuf>,

    /// Operating-system account used by a system-wide Linux/macOS service
    #[arg(long)]
    run_as: Option<String>,

    /// Install and enable autostart without starting immediately
    #[arg(long)]
    no_start: bool,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum ServiceStartMode {
    #[default]
    Server,
    ServerAndWorker,
    All,
}

#[derive(Debug)]
struct ServiceDefinition {
    name: String,
    executable: PathBuf,
    working_directory: PathBuf,
    arguments: Vec<String>,
    run_as: Option<String>,
    user_scope: bool,
}

/// Handles the custom `service` command before Loco parses its own command set.
/// Returns `false` for all existing Loco commands.
pub fn handle_if_requested() -> Result<bool> {
    let mut arguments = env::args_os();
    let _binary = arguments.next();
    let Some(command) = arguments.next() else {
        return Ok(false);
    };
    if command != OsStr::new("service") {
        return Ok(false);
    }

    let parser_arguments = std::iter::once(OsString::from("service")).chain(arguments);
    let cli = match ServiceCli::try_parse_from(parser_arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            print!("{error}");
            return Ok(true);
        }
        Err(error) => return Err(message(error.to_string())),
    };
    execute(cli.command)?;
    Ok(true)
}

fn execute(command: ServiceCommand) -> Result<()> {
    match command {
        ServiceCommand::Install(arguments) => {
            let no_start = arguments.no_start;
            let definition = resolve_definition(arguments)?;
            platform::install(&definition, no_start)
        }
        ServiceCommand::Uninstall(target) => {
            validate_target(&target)?;
            platform::uninstall(&target)
        }
        ServiceCommand::Start(target) => {
            validate_target(&target)?;
            platform::start(&target)
        }
        ServiceCommand::Stop(target) => {
            validate_target(&target)?;
            platform::stop(&target)
        }
        ServiceCommand::Restart(target) => {
            validate_target(&target)?;
            platform::restart(&target)
        }
        ServiceCommand::Status(target) => {
            validate_target(&target)?;
            platform::status(&target)
        }
    }
}

fn resolve_definition(arguments: InstallArgs) -> Result<ServiceDefinition> {
    validate_target(&arguments.target)?;
    if arguments.port == Some(0) {
        return Err(message("service port must be between 1 and 65535"));
    }
    if arguments
        .binding
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.chars().any(char::is_control))
    {
        return Err(message("service binding is invalid"));
    }
    if arguments
        .run_as
        .as_deref()
        .is_some_and(|value| !valid_account_name(value))
    {
        return Err(message("service account name is invalid"));
    }

    let working_directory = arguments
        .working_directory
        .unwrap_or(env::current_dir().map_err(Error::from)?)
        .canonicalize()
        .map_err(|error| {
            message(format!(
                "failed to resolve service working directory: {error}"
            ))
        })?;
    if !working_directory.is_dir() {
        return Err(message("service working directory is not a directory"));
    }
    let executable = env::current_exe()
        .map_err(Error::from)?
        .canonicalize()
        .map_err(|error| message(format!("failed to resolve current executable: {error}")))?;
    let environment = resolve_environment(arguments.environment.as_deref(), &working_directory)?;
    let run_as = arguments.run_as.or_else(default_run_as);
    let mut start_arguments = vec![
        "-e".to_string(),
        environment,
        "start".to_string(),
        "--no-banner".to_string(),
    ];
    match arguments.mode {
        ServiceStartMode::Server => {}
        ServiceStartMode::ServerAndWorker => {
            start_arguments.push("--server-and-worker".to_string());
        }
        ServiceStartMode::All => start_arguments.push("--all".to_string()),
    }
    if let Some(port) = arguments.port {
        start_arguments.extend(["--port".to_string(), port.to_string()]);
    }
    if let Some(binding) = arguments.binding {
        start_arguments.extend(["--binding".to_string(), binding]);
    }

    Ok(ServiceDefinition {
        name: arguments.target.name,
        executable,
        working_directory,
        arguments: start_arguments,
        run_as,
        user_scope: arguments.target.user,
    })
}

fn resolve_environment(requested: Option<&str>, working_directory: &Path) -> Result<String> {
    let environment = requested
        .map(str::to_string)
        .or_else(|| non_empty_environment("LOCO_ENV"))
        .or_else(|| non_empty_environment("RAILS_ENV"))
        .or_else(|| non_empty_environment("NODE_ENV"))
        .unwrap_or_else(|| detect_environment_from_config(working_directory));
    if !valid_environment_name(&environment) {
        return Err(message(
            "service environment may only contain letters, numbers, underscores, and hyphens",
        ));
    }
    Ok(environment)
}

fn detect_environment_from_config(working_directory: &Path) -> String {
    let config_folder = env::var_os("LOCO_CONFIG_FOLDER").map_or_else(
        || working_directory.join("config"),
        |path| {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                working_directory.join(path)
            }
        },
    );
    if ["production.local.yaml", "production.yaml"]
        .iter()
        .any(|name| config_folder.join(name).is_file())
    {
        "production".to_string()
    } else {
        "development".to_string()
    }
}

fn non_empty_environment(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_target(target: &TargetArgs) -> Result<()> {
    if !valid_service_name(&target.name) {
        return Err(message(
            "service name must start with a letter or number and contain only letters, numbers, hyphens, and underscores",
        ));
    }
    Ok(())
}

fn valid_service_name(value: &str) -> bool {
    value.len() <= 64
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn valid_account_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.chars().any(|character| {
            character.is_control() || matches!(character, '/' | '\\' | ':' | ';' | '"' | '\'')
        })
}

fn default_run_as() -> Option<String> {
    ["SUDO_USER", "USER", "USERNAME"]
        .into_iter()
        .find_map(non_empty_environment)
        .filter(|value| value != "root")
}

#[cfg(any(test, target_os = "macos"))]
fn service_label(name: &str) -> String {
    format!("{SERVICE_LABEL_PREFIX}.{name}")
}

#[cfg(any(test, target_os = "macos"))]
fn launchd_log_directory(user_scope: bool, account: &str, home_directory: &Path) -> PathBuf {
    if user_scope || account != "root" {
        home_directory.join("Library/Logs")
    } else {
        PathBuf::from("/var/log")
    }
}

fn message(value: impl Into<String>) -> Error {
    Error::Message(value.into())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_service_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            message(format!(
                "failed to create service directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, contents).map_err(|error| {
        message(format!(
            "failed to write service file {}: {error}; system-wide installation may require administrator privileges",
            path.display()
        ))
    })
}

fn run_command(command: &mut Command, action: &str) -> Result<()> {
    let status = command
        .status()
        .map_err(|error| message(format!("failed to {action}: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(message(format!(
            "failed to {action}: command exited with {status}"
        )))
    }
}

#[cfg(any(test, target_os = "linux"))]
fn render_systemd_unit(definition: &ServiceDefinition) -> String {
    let executable = systemd_quote(&definition.executable.to_string_lossy());
    let arguments = definition
        .arguments
        .iter()
        .map(|argument| systemd_quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let user = if definition.user_scope {
        String::new()
    } else {
        definition
            .run_as
            .as_deref()
            .map_or_else(String::new, |account| format!("User={account}\n"))
    };
    let wanted_by = if definition.user_scope {
        "default.target"
    } else {
        "multi-user.target"
    };
    format!(
        "[Unit]\nDescription=GPT Images ({name})\nWants=network-online.target\nAfter=network-online.target\n\n[Service]\nType=simple\n{user}WorkingDirectory={working_directory}\nExecStart={executable} {arguments}\nRestart=always\nRestartSec=5\nTimeoutStopSec=30\n\n[Install]\nWantedBy={wanted_by}\n",
        name = definition.name,
        working_directory = systemd_quote(&definition.working_directory.to_string_lossy()),
    )
}

#[cfg(any(test, target_os = "linux"))]
fn systemd_quote(value: &str) -> String {
    let escaped = value
        .replace('%', "%%")
        .replace('$', "$$")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(any(test, target_os = "macos"))]
fn render_launchd_plist(
    definition: &ServiceDefinition,
    stdout_path: &Path,
    stderr_path: &Path,
    account: &str,
    home_directory: &Path,
) -> String {
    let mut program_arguments = vec![definition.executable.to_string_lossy().into_owned()];
    program_arguments.extend(definition.arguments.iter().cloned());
    let program_arguments = program_arguments
        .iter()
        .map(|argument| format!("    <string>{}</string>", xml_escape(argument)))
        .collect::<Vec<_>>()
        .join("\n");
    let user_name = if definition.user_scope {
        String::new()
    } else {
        definition
            .run_as
            .as_deref()
            .map_or_else(String::new, |user| {
                format!(
                    "  <key>UserName</key>\n  <string>{}</string>\n",
                    xml_escape(user)
                )
            })
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{label}</string>\n  <key>ProgramArguments</key>\n  <array>\n{program_arguments}\n  </array>\n  <key>WorkingDirectory</key>\n  <string>{working_directory}</string>\n{user_name}  <key>EnvironmentVariables</key>\n  <dict>\n    <key>HOME</key>\n    <string>{home_directory}</string>\n    <key>USER</key>\n    <string>{account}</string>\n    <key>LOGNAME</key>\n    <string>{account}</string>\n    <key>GPT_IMAGES_DB_WATCHDOG</key>\n    <string>1</string>\n  </dict>\n  <key>RunAtLoad</key>\n  <true/>\n  <key>KeepAlive</key>\n  <true/>\n  <key>ThrottleInterval</key>\n  <integer>5</integer>\n  <key>StandardOutPath</key>\n  <string>{stdout}</string>\n  <key>StandardErrorPath</key>\n  <string>{stderr}</string>\n</dict>\n</plist>\n",
        label = xml_escape(&service_label(&definition.name)),
        working_directory = xml_escape(&definition.working_directory.to_string_lossy()),
        home_directory = xml_escape(&home_directory.to_string_lossy()),
        account = xml_escape(account),
        stdout = xml_escape(&stdout_path.to_string_lossy()),
        stderr = xml_escape(&stderr_path.to_string_lossy()),
    )
}

#[cfg(any(test, target_os = "windows"))]
fn render_windows_task(definition: &ServiceDefinition) -> String {
    let arguments = definition
        .arguments
        .iter()
        .map(|argument| windows_quote_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let (trigger, principal) = if definition.user_scope {
        let user = definition.run_as.as_deref().unwrap_or("CURRENT_USER");
        (
            format!(
                "    <LogonTrigger>\n      <Enabled>true</Enabled>\n      <UserId>{}</UserId>\n    </LogonTrigger>",
                xml_escape(user)
            ),
            format!(
                "      <UserId>{}</UserId>\n      <LogonType>InteractiveToken</LogonType>\n      <RunLevel>HighestAvailable</RunLevel>",
                xml_escape(user)
            ),
        )
    } else {
        (
            "    <BootTrigger>\n      <Enabled>true</Enabled>\n    </BootTrigger>".to_string(),
            "      <UserId>S-1-5-18</UserId>\n      <LogonType>ServiceAccount</LogonType>\n      <RunLevel>HighestAvailable</RunLevel>".to_string(),
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n  <RegistrationInfo>\n    <Description>GPT Images ({name})</Description>\n  </RegistrationInfo>\n  <Triggers>\n{trigger}\n  </Triggers>\n  <Principals>\n    <Principal id=\"Author\">\n{principal}\n    </Principal>\n  </Principals>\n  <Settings>\n    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n    <AllowHardTerminate>true</AllowHardTerminate>\n    <StartWhenAvailable>true</StartWhenAvailable>\n    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\n    <AllowStartOnDemand>true</AllowStartOnDemand>\n    <Enabled>true</Enabled>\n    <Hidden>false</Hidden>\n    <RunOnlyIfIdle>false</RunOnlyIfIdle>\n    <WakeToRun>false</WakeToRun>\n    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n    <RestartOnFailure>\n      <Interval>PT10S</Interval>\n      <Count>999</Count>\n    </RestartOnFailure>\n    <Priority>7</Priority>\n  </Settings>\n  <Actions Context=\"Author\">\n    <Exec>\n      <Command>{executable}</Command>\n      <Arguments>{arguments}</Arguments>\n      <WorkingDirectory>{working_directory}</WorkingDirectory>\n    </Exec>\n  </Actions>\n</Task>\n",
        name = xml_escape(&definition.name),
        executable = xml_escape(&definition.executable.to_string_lossy()),
        arguments = xml_escape(&arguments),
        working_directory = xml_escape(&definition.working_directory.to_string_lossy()),
    )
}

#[cfg(any(test, target_os = "windows"))]
fn windows_quote_argument(value: &str) -> String {
    if !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return value.to_string();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in value.chars() {
        if character == '\\' {
            backslashes += 1;
        } else if character == '"' {
            quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
            quoted.push('"');
            backslashes = 0;
        } else {
            quoted.push_str(&"\\".repeat(backslashes));
            backslashes = 0;
            quoted.push(character);
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(any(test, target_os = "macos", target_os = "windows"))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{
        env, fs, message, render_systemd_unit, run_command, write_service_file, Command, PathBuf,
        Result, ServiceDefinition, TargetArgs,
    };

    pub fn install(definition: &ServiceDefinition, no_start: bool) -> Result<()> {
        let target = TargetArgs {
            name: definition.name.clone(),
            user: definition.user_scope,
        };
        let path = service_path(&target)?;
        write_service_file(&path, &render_systemd_unit(definition))?;
        systemctl(&target, &["daemon-reload"], "reload systemd")?;
        let unit = unit_name(&target.name);
        systemctl(&target, &["enable", &unit], "enable service autostart")?;
        if !no_start {
            systemctl(&target, &["restart", &unit], "start or restart service")?;
        }
        println!("Installed {}", path.display());
        println!("Service name: {unit}");
        Ok(())
    }

    pub fn uninstall(target: &TargetArgs) -> Result<()> {
        let unit = unit_name(&target.name);
        let _ = systemctl(target, &["disable", "--now", &unit], "disable service");
        let path = service_path(target)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                message(format!("failed to remove {}: {error}", path.display()))
            })?;
        }
        systemctl(target, &["daemon-reload"], "reload systemd")?;
        println!("Removed {unit}");
        Ok(())
    }

    pub fn start(target: &TargetArgs) -> Result<()> {
        systemctl(
            target,
            &["start", &unit_name(&target.name)],
            "start service",
        )
    }

    pub fn stop(target: &TargetArgs) -> Result<()> {
        systemctl(target, &["stop", &unit_name(&target.name)], "stop service")
    }

    pub fn restart(target: &TargetArgs) -> Result<()> {
        systemctl(
            target,
            &["restart", &unit_name(&target.name)],
            "restart service",
        )
    }

    pub fn status(target: &TargetArgs) -> Result<()> {
        systemctl(
            target,
            &["status", "--no-pager", &unit_name(&target.name)],
            "query service status",
        )
    }

    fn unit_name(name: &str) -> String {
        format!("{name}.service")
    }

    fn service_path(target: &TargetArgs) -> Result<PathBuf> {
        if target.user {
            Ok(home_directory()?.join(format!(".config/systemd/user/{}", unit_name(&target.name))))
        } else {
            Ok(PathBuf::from("/etc/systemd/system").join(unit_name(&target.name)))
        }
    }

    fn systemctl(target: &TargetArgs, arguments: &[&str], action: &str) -> Result<()> {
        let mut command = Command::new("systemctl");
        if target.user {
            command.arg("--user");
        }
        command.args(arguments);
        run_command(&mut command, action)
    }

    fn home_directory() -> Result<PathBuf> {
        env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| message("HOME is required for per-user service installation"))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{os::unix::fs::PermissionsExt, process::Stdio};

    use super::{
        env, fs, launchd_log_directory, message, render_launchd_plist, run_command, service_label,
        write_service_file, Command, Path, PathBuf, Result, ServiceDefinition, TargetArgs,
    };

    pub fn install(definition: &ServiceDefinition, no_start: bool) -> Result<()> {
        let target = TargetArgs {
            name: definition.name.clone(),
            user: definition.user_scope,
        };
        let (account, home_directory) = launchd_identity(definition)?;
        let (stdout_path, stderr_path) = log_paths(&target, &account, &home_directory);
        prepare_log_directory(&stdout_path, &target, &account)?;
        let path = service_path(&target)?;
        write_service_file(
            &path,
            &render_launchd_plist(
                definition,
                &stdout_path,
                &stderr_path,
                &account,
                &home_directory,
            ),
        )?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).map_err(|error| {
            message(format!(
                "failed to set permissions on {}: {error}",
                path.display()
            ))
        })?;
        if !no_start {
            if is_loaded(&target)? {
                bootout(&target)?;
            }
            bootstrap(&target)?;
        }
        println!("Installed {}", path.display());
        println!("Service label: {}", service_label(&target.name));
        println!("Standard log: {}", stdout_path.display());
        println!("Error log: {}", stderr_path.display());
        Ok(())
    }

    pub fn uninstall(target: &TargetArgs) -> Result<()> {
        if is_loaded(target)? {
            bootout(target)?;
        }
        let path = service_path(target)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                message(format!("failed to remove {}: {error}", path.display()))
            })?;
        }
        println!("Removed {}", service_label(&target.name));
        Ok(())
    }

    pub fn start(target: &TargetArgs) -> Result<()> {
        if is_loaded(target)? {
            kickstart(target)
        } else {
            bootstrap(target)
        }
    }

    pub fn stop(target: &TargetArgs) -> Result<()> {
        bootout(target)
    }

    pub fn restart(target: &TargetArgs) -> Result<()> {
        if is_loaded(target)? {
            kickstart(target)
        } else {
            bootstrap(target)
        }
    }

    pub fn status(target: &TargetArgs) -> Result<()> {
        let mut command = Command::new("launchctl");
        command.args(["print", &service_target(target)?]);
        run_command(&mut command, "query service status")
    }

    fn bootstrap(target: &TargetArgs) -> Result<()> {
        let mut command = Command::new("launchctl");
        command
            .arg("bootstrap")
            .arg(service_domain(target)?)
            .arg(service_path(target)?);
        run_command(&mut command, "start launchd service")
    }

    fn bootout(target: &TargetArgs) -> Result<()> {
        let mut command = Command::new("launchctl");
        command.args(["bootout", &service_target(target)?]);
        run_command(&mut command, "stop launchd service")
    }

    fn kickstart(target: &TargetArgs) -> Result<()> {
        let mut command = Command::new("launchctl");
        command.args(["kickstart", "-k", &service_target(target)?]);
        run_command(&mut command, "restart launchd service")
    }

    fn is_loaded(target: &TargetArgs) -> Result<bool> {
        let status = Command::new("launchctl")
            .args(["print", &service_target(target)?])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| message(format!("failed to query launchd: {error}")))?;
        Ok(status.success())
    }

    fn service_path(target: &TargetArgs) -> Result<PathBuf> {
        let filename = format!("{}.plist", service_label(&target.name));
        if target.user {
            Ok(home_directory()?
                .join("Library/LaunchAgents")
                .join(filename))
        } else {
            Ok(PathBuf::from("/Library/LaunchDaemons").join(filename))
        }
    }

    fn log_paths(target: &TargetArgs, account: &str, home_directory: &Path) -> (PathBuf, PathBuf) {
        let directory = launchd_log_directory(target.user, account, home_directory);
        (
            directory.join(format!("{}.log", target.name)),
            directory.join(format!("{}.error.log", target.name)),
        )
    }

    fn prepare_log_directory(stdout_path: &Path, target: &TargetArgs, account: &str) -> Result<()> {
        let directory = stdout_path
            .parent()
            .ok_or_else(|| message("launchd log path has no parent directory"))?;
        if directory.exists() {
            if !directory.is_dir() {
                return Err(message(format!(
                    "launchd log directory is not a directory: {}",
                    directory.display()
                )));
            }
            return Ok(());
        }

        fs::create_dir_all(directory).map_err(|error| {
            message(format!(
                "failed to create launchd log directory {}: {error}",
                directory.display()
            ))
        })?;
        if !target.user && account != "root" {
            let mut command = Command::new("/usr/sbin/chown");
            command.arg(account).arg(directory);
            run_command(&mut command, "set launchd log directory owner")?;
        }
        Ok(())
    }

    fn launchd_identity(definition: &ServiceDefinition) -> Result<(String, PathBuf)> {
        let account = definition.run_as.clone().unwrap_or_else(|| {
            env::var("USER")
                .or_else(|_| env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string())
        });
        let home = if definition.user_scope {
            home_directory()?
        } else {
            account_home_directory(&account)?
        };
        Ok((account, home))
    }

    fn account_home_directory(account: &str) -> Result<PathBuf> {
        let output = Command::new("/usr/bin/dscacheutil")
            .args(["-q", "user", "-a", "name", account])
            .output()
            .map_err(|error| message(format!("failed to resolve home for {account}: {error}")))?;
        if !output.status.success() {
            return Err(message(format!(
                "failed to resolve home directory for service account {account}"
            )));
        }
        let stdout = String::from_utf8(output.stdout)
            .map_err(|_| message("service account home directory is invalid"))?;
        let home = stdout
            .lines()
            .find_map(|line| line.strip_prefix("dir:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| message("service account home directory was not returned"))?;
        Ok(PathBuf::from(home))
    }

    fn service_domain(target: &TargetArgs) -> Result<String> {
        if target.user {
            Ok(format!("gui/{}", current_uid()?))
        } else {
            Ok("system".to_string())
        }
    }

    fn service_target(target: &TargetArgs) -> Result<String> {
        Ok(format!(
            "{}/{}",
            service_domain(target)?,
            service_label(&target.name)
        ))
    }

    fn current_uid() -> Result<String> {
        let output = Command::new("id")
            .arg("-u")
            .output()
            .map_err(|error| message(format!("failed to determine current uid: {error}")))?;
        if !output.status.success() {
            return Err(message("failed to determine current uid"));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_string())
            .map_err(|_| message("current uid output is invalid"))
    }

    fn home_directory() -> Result<PathBuf> {
        env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| message("HOME is required for per-user service installation"))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{
        env, fs, message, render_windows_task, run_command, Command, Result, ServiceDefinition,
        TargetArgs,
    };

    pub fn install(definition: &ServiceDefinition, no_start: bool) -> Result<()> {
        let target = TargetArgs {
            name: definition.name.clone(),
            user: definition.user_scope,
        };
        if !no_start {
            end_if_present(&target);
        }
        let task_xml = render_windows_task(definition);
        let task_file = env::temp_dir().join(format!(
            "{}-service-{}.xml",
            definition.name,
            std::process::id()
        ));
        fs::write(&task_file, task_xml).map_err(|error| {
            message(format!(
                "failed to write temporary task definition: {error}"
            ))
        })?;
        let mut create = Command::new("schtasks.exe");
        create
            .args(["/Create", "/TN"])
            .arg(&definition.name)
            .arg("/XML")
            .arg(&task_file)
            .arg("/F");
        let result = run_command(&mut create, "install Windows startup task");
        let _ = fs::remove_file(&task_file);
        result?;
        if !no_start {
            start(&target)?;
        }
        println!("Installed Windows startup task {}", definition.name);
        Ok(())
    }

    pub fn uninstall(target: &TargetArgs) -> Result<()> {
        end_if_present(target);
        let mut command = Command::new("schtasks.exe");
        command.args(["/Delete", "/TN"]).arg(&target.name).arg("/F");
        run_command(&mut command, "remove Windows startup task")
    }

    pub fn start(target: &TargetArgs) -> Result<()> {
        let mut command = Command::new("schtasks.exe");
        command.args(["/Run", "/TN"]).arg(&target.name);
        run_command(&mut command, "start Windows task")
    }

    pub fn stop(target: &TargetArgs) -> Result<()> {
        let mut command = Command::new("schtasks.exe");
        command.args(["/End", "/TN"]).arg(&target.name);
        run_command(&mut command, "stop Windows task")
    }

    pub fn restart(target: &TargetArgs) -> Result<()> {
        end_if_present(target);
        start(target)
    }

    pub fn status(target: &TargetArgs) -> Result<()> {
        let mut command = Command::new("schtasks.exe");
        command
            .args(["/Query", "/TN"])
            .arg(&target.name)
            .args(["/FO", "LIST", "/V"]);
        run_command(&mut command, "query Windows task status")
    }

    fn end_if_present(target: &TargetArgs) {
        let _ = Command::new("schtasks.exe")
            .args(["/End", "/TN"])
            .arg(&target.name)
            .output();
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::{message, Result, ServiceDefinition, TargetArgs};

    pub fn install(_definition: &ServiceDefinition, _no_start: bool) -> Result<()> {
        unsupported()
    }

    pub fn uninstall(_target: &TargetArgs) -> Result<()> {
        unsupported()
    }

    pub fn start(_target: &TargetArgs) -> Result<()> {
        unsupported()
    }

    pub fn stop(_target: &TargetArgs) -> Result<()> {
        unsupported()
    }

    pub fn restart(_target: &TargetArgs) -> Result<()> {
        unsupported()
    }

    pub fn status(_target: &TargetArgs) -> Result<()> {
        unsupported()
    }

    fn unsupported() -> Result<()> {
        Err(message(
            "service management is supported on Linux, macOS, and Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        launchd_log_directory, render_launchd_plist, render_systemd_unit, render_windows_task,
        valid_service_name, windows_quote_argument, xml_escape, Path, PathBuf, ServiceDefinition,
    };

    fn definition(user_scope: bool) -> ServiceDefinition {
        ServiceDefinition {
            name: "gpt-images".to_string(),
            executable: PathBuf::from("/opt/GPT Images/gpt_images-cli"),
            working_directory: PathBuf::from("/opt/GPT Images"),
            arguments: vec![
                "-e".to_string(),
                "production".to_string(),
                "start".to_string(),
                "--no-banner".to_string(),
                "--port".to_string(),
                "5150".to_string(),
            ],
            run_as: Some("app-user".to_string()),
            user_scope,
        }
    }

    #[test]
    fn systemd_unit_has_autostart_restart_and_exact_arguments() {
        let unit = render_systemd_unit(&definition(false));
        assert!(unit.contains("User=app-user"));
        assert!(unit.contains("WantedBy=multi-user.target"));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("\"/opt/GPT Images/gpt_images-cli\""));
        assert!(unit.contains("\"--port\" \"5150\""));
    }

    #[test]
    fn launchd_plist_has_keep_alive_and_escaped_paths() {
        let plist = render_launchd_plist(
            &definition(false),
            Path::new("/var/log/gpt-images.log"),
            Path::new("/var/log/gpt-images.error.log"),
            "app-user",
            Path::new("/Users/app-user"),
        );
        assert!(plist.contains("<string>top.pikju.gpt-images</string>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<key>GPT_IMAGES_DB_WATCHDOG</key>"));
        assert!(plist.contains("<key>UserName</key>"));
        assert!(plist.contains("<string>/opt/GPT Images</string>"));
        assert!(plist.contains("<key>HOME</key>"));
        assert!(plist.contains("<string>/Users/app-user</string>"));
    }

    #[test]
    fn launchd_logs_are_writable_by_the_service_account() {
        let user_home = Path::new("/Users/app-user");
        assert_eq!(
            launchd_log_directory(false, "app-user", user_home),
            user_home.join("Library/Logs")
        );
        assert_eq!(
            launchd_log_directory(true, "app-user", user_home),
            user_home.join("Library/Logs")
        );
        assert_eq!(
            launchd_log_directory(false, "root", Path::new("/var/root")),
            PathBuf::from("/var/log")
        );
    }

    #[test]
    fn windows_task_runs_at_boot_and_restarts_on_failure() {
        let task = render_windows_task(&definition(false));
        assert!(task.contains("<BootTrigger>"));
        assert!(task.contains("<UserId>S-1-5-18</UserId>"));
        assert!(task.contains("<RestartOnFailure>"));
        assert!(task.contains("<Command>/opt/GPT Images/gpt_images-cli</Command>"));
        assert!(task.contains("<Arguments>-e production start --no-banner --port 5150</Arguments>"));
    }

    #[test]
    fn user_scope_uses_login_native_targets() {
        let definition = definition(true);
        let unit = render_systemd_unit(&definition);
        let plist = render_launchd_plist(
            &definition,
            Path::new("/tmp/output.log"),
            Path::new("/tmp/error.log"),
            "app-user",
            Path::new("/Users/app-user"),
        );
        let task = render_windows_task(&definition);
        assert!(unit.contains("WantedBy=default.target"));
        assert!(!unit.contains("User=app-user"));
        assert!(!plist.contains("<key>UserName</key>"));
        assert!(task.contains("<LogonTrigger>"));
    }

    #[test]
    fn names_and_windows_arguments_are_validated_and_escaped() {
        assert!(valid_service_name("gpt-images_2"));
        assert!(!valid_service_name("../gpt-images"));
        assert_eq!(windows_quote_argument("plain"), "plain");
        assert_eq!(windows_quote_argument("two words"), "\"two words\"");
        assert_eq!(xml_escape("a<&\"'"), "a&lt;&amp;&quot;&apos;");
    }
}
