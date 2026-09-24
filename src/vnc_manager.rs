//! Native VNC server detection and installation helpers.

#![allow(clippy::missing_errors_doc)]

use std::{env, process::Command};

use clap::{error::ErrorKind, Args, Parser, Subcommand};
use loco_rs::{Error, Result};

#[derive(Debug, Parser)]
#[command(name = "vnc", about = "Detect and install a local VNC server")]
struct VncCli {
    #[command(subcommand)]
    command: VncCommand,
}

#[derive(Debug, Subcommand)]
enum VncCommand {
    /// Show whether a local VNC service is installed and reachable.
    Status,
    /// Install TightVNC with winget, then start and verify its service.
    Install(InstallArgs),
    /// Start the installed VNC service.
    Start,
}

#[derive(Debug, Args)]
struct InstallArgs {
    /// Do not ask winget to install anything; only start and verify an existing service.
    #[arg(long)]
    no_install: bool,
}

pub fn handle_if_requested() -> Result<bool> {
    let mut arguments = env::args_os();
    let _binary = arguments.next();
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("vnc")) {
        return Ok(false);
    }
    let parser_arguments = std::iter::once("vnc".to_string())
        .chain(arguments.map(|value| value.to_string_lossy().into_owned()));
    let cli = match VncCli::try_parse_from(parser_arguments) {
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

fn execute(command: VncCommand) -> Result<()> {
    match command {
        VncCommand::Status => platform::status(),
        VncCommand::Install(args) => platform::install(!args.no_install),
        VncCommand::Start => platform::start(),
    }
}

fn message(value: impl Into<String>) -> Error {
    Error::Message(value.into())
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{message, run, Command, Result};

    const SERVICE: &str = "tvnserver";
    const PORT: &str = "5900";

    pub fn status() -> Result<()> {
        let service = Command::new("sc.exe").args(["query", SERVICE]).output();
        let port = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                "Test-NetConnection 127.0.0.1 -Port 5900 -InformationLevel Quiet",
            ])
            .output();
        match service {
            Ok(output) if output.status.success() => {
                let state = String::from_utf8_lossy(&output.stdout);
                let listening = port
                    .as_ref()
                    .is_ok_and(|value| String::from_utf8_lossy(&value.stdout).trim() == "True");
                println!("TightVNC service detected; port {PORT} listening: {listening}");
                if state.contains("RUNNING") {
                    println!("service state: RUNNING");
                } else {
                    println!("service state: installed but not running");
                }
                Ok(())
            }
            _ => {
                println!(
                    "No local VNC service detected on Windows (expected {SERVICE}, port {PORT})."
                );
                Ok(())
            }
        }
    }

    pub fn install(allow_install: bool) -> Result<()> {
        if !service_exists() && allow_install {
            println!("Installing TightVNC via winget...");
            run(
                Command::new("winget").args([
                    "install",
                    "--id",
                    "GlavSoft.TightVNC",
                    "--exact",
                    "--accept-source-agreements",
                    "--accept-package-agreements",
                    "--silent",
                ]),
                "install TightVNC",
            )?;
        }
        if !service_exists() {
            return Err(message(
                "TightVNC is not installed. Install winget or run without --no-install.",
            ));
        }
        start()
    }

    pub fn start() -> Result<()> {
        run(
            Command::new("sc.exe").args(["config", SERVICE, "start=", "auto"]),
            "configure TightVNC autostart",
        )?;
        run(
            Command::new("sc.exe").args(["start", SERVICE]),
            "start TightVNC",
        )?;
        println!("Local VNC is ready at 127.0.0.1:{PORT}.");
        Ok(())
    }

    fn service_exists() -> bool {
        Command::new("sc.exe")
            .args(["query", SERVICE])
            .output()
            .is_ok_and(|output| output.status.success())
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::{message, Result};

    pub fn status() -> Result<()> {
        println!("Automatic VNC installation is currently implemented for Windows only.");
        Ok(())
    }
    pub fn install(_: bool) -> Result<()> {
        Err(message("automatic VNC installation is supported on Windows only; configure a VNC server and use a remote VNC target"))
    }
    pub fn start() -> Result<()> {
        Err(message(
            "automatic VNC service control is supported on Windows only",
        ))
    }
}

#[cfg(target_os = "windows")]
fn run(command: &mut Command, action: &str) -> Result<()> {
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
