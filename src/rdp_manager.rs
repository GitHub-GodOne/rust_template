//! `rdp` command: installs and starts the RDP gateway (guacd) without Docker.

#![allow(clippy::missing_errors_doc)]

use std::{env, time::Duration};

use clap::{error::ErrorKind, Args, CommandFactory, Parser, Subcommand};
use loco_rs::{Error, Result};

use crate::services::rdp_gateway::{self, Runtime};

#[derive(Debug, Parser)]
#[command(
    name = "rdp",
    about = "Install and start the RDP gateway (guacd) without Docker"
)]
struct RdpCli {
    #[command(subcommand)]
    command: RdpCommand,
}

#[derive(Debug, Subcommand)]
enum RdpCommand {
    /// Show the gateway address, whether it answers and which guacd is installed.
    Status,
    /// Install guacd (Windows: Alpine Linux in WSL; Linux: system packages), then start it.
    Install(InstallArgs),
    /// Start the installed guacd in the background if the gateway is not answering.
    Start,
}

#[derive(Debug, Args)]
struct InstallArgs {
    /// Alpine mirror for the Windows/WSL install, e.g. <https://mirrors.aliyun.com/alpine>; by default the official CDN is tried first, then Chinese mirrors.
    #[arg(long, value_name = "URL")]
    mirror: Option<String>,
    /// Only install; do not start guacd afterwards.
    #[arg(long)]
    no_start: bool,
}

/// Clap definition of this command, listed on the project CLI page.
#[must_use]
pub fn cli_command() -> clap::Command {
    RdpCli::command()
}

pub async fn handle_if_requested() -> Result<bool> {
    // Collected so the future stays `Send` (`ArgsOs` is not).
    let mut arguments = env::args_os().collect::<Vec<_>>().into_iter();
    let _binary = arguments.next();
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("rdp")) {
        return Ok(false);
    }
    let parser_arguments = std::iter::once("rdp".to_string())
        .chain(arguments.map(|value| value.to_string_lossy().into_owned()));
    let cli = match RdpCli::try_parse_from(parser_arguments) {
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
    execute(cli.command).await?;
    Ok(true)
}

async fn execute(command: RdpCommand) -> Result<()> {
    match command {
        RdpCommand::Status => status().await,
        RdpCommand::Install(args) => {
            platform::install(args.mirror.as_deref()).await?;
            if args.no_start {
                println!(
                    "guacd is installed; the backend starts it automatically, or run `rdp start`."
                );
                Ok(())
            } else {
                start().await
            }
        }
        RdpCommand::Start => start().await,
    }
}

fn message(value: impl Into<String>) -> Error {
    Error::Message(value.into())
}

async fn detect() -> Option<Runtime> {
    tokio::task::spawn_blocking(rdp_gateway::detect_runtime)
        .await
        .ok()
        .flatten()
}

async fn status() -> Result<()> {
    let address = rdp_gateway::address();
    println!("gateway address: {address}");
    println!(
        "gateway answering: {}",
        rdp_gateway::reachable(&address).await
    );
    match detect().await {
        Some(runtime) => println!("installed guacd: {}", runtime.describe()),
        None => println!("installed guacd: none (run `rdp install`)"),
    }
    #[cfg(windows)]
    println!(
        "WSL available: {}",
        tokio::task::spawn_blocking(rdp_gateway::wsl::available)
            .await
            .unwrap_or(false)
    );
    if rdp_gateway::local_bind(&address).is_none() {
        println!("RDP_GUACD_ADDRESS points to another machine; this backend will not start guacd itself.");
    }
    Ok(())
}

async fn start() -> Result<()> {
    let address = rdp_gateway::address();
    let Some((host, port)) = rdp_gateway::local_bind(&address) else {
        return Err(message(format!(
            "RDP_GUACD_ADDRESS ({address}) is not on this machine; start guacd there"
        )));
    };
    if rdp_gateway::reachable(&address).await {
        println!("The RDP gateway is already answering at {address}.");
        return Ok(());
    }
    let runtime = detect()
        .await
        .ok_or_else(|| message("guacd is not installed; run `rdp install` first"))?;
    rdp_gateway::spawn_detached(&runtime, &host, port).map_err(message)?;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if rdp_gateway::reachable(&address).await {
            println!(
                "The RDP gateway is ready at {address} ({}).",
                runtime.describe()
            );
            return Ok(());
        }
    }
    Err(message(format!(
        "guacd was started but {address} is still not answering; run `rdp status`"
    )))
}

#[cfg(not(target_os = "macos"))]
fn run(command: &mut std::process::Command, action: &str) -> Result<()> {
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

#[cfg(windows)]
mod platform {
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    use sha2::{Digest, Sha256};

    use super::{message, run, Result};
    use crate::services::rdp_gateway::{self, wsl, WSL_DISTRO};

    const MIRRORS: [&str; 3] = [
        "https://dl-cdn.alpinelinux.org/alpine",
        "https://mirrors.aliyun.com/alpine",
        "https://mirrors.ustc.edu.cn/alpine",
    ];

    async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T> {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|error| message(error.to_string()))
    }

    fn mirrors(custom: Option<&str>) -> Result<Vec<String>> {
        let Some(custom) = custom else {
            return Ok(MIRRORS.iter().map(ToString::to_string).collect());
        };
        let custom = custom.trim().trim_end_matches('/');
        // The mirror ends up in a shell command inside the distro.
        let valid = url::Url::parse(custom)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
            && !custom.contains(['\'', '"', '\\', '`', '$', ' ']);
        if valid {
            Ok(vec![custom.to_string()])
        } else {
            Err(message(format!(
                "--mirror must be an http(s) URL, got {custom}"
            )))
        }
    }

    pub async fn install(custom_mirror: Option<&str>) -> Result<()> {
        let mirrors = mirrors(custom_mirror)?;
        if wsl::running_as_system() {
            return Err(message("WSL is per user and cannot be used from the SYSTEM account; run this from your own account (and install the service with `service install --user`)"));
        }
        if !blocking(wsl::available).await? {
            println!("WSL is not enabled. Running `wsl --install --no-distribution` (needs administrator rights)...");
            let installed = blocking(|| {
                wsl::command()
                    .args(["--install", "--no-distribution"])
                    .status()
                    .is_ok_and(|status| status.success())
            })
            .await?;
            return Err(message(if installed {
                "WSL was enabled. Restart Windows, then run `rdp install` again."
            } else {
                "WSL could not be enabled automatically. Run `wsl --install --no-distribution` in an administrator terminal, restart Windows, then run `rdp install` again."
            }));
        }
        if blocking(wsl::distro_installed).await? {
            println!("WSL distro {WSL_DISTRO} already exists.");
        } else {
            import_distro(&mirrors).await?;
        }
        install_packages(&mirrors).await?;
        let version = blocking(|| wsl::shell(wsl::hidden(), "guacd -v").output()).await?;
        match version {
            Ok(output) if output.status.success() => println!(
                "Installed {} in WSL distro {WSL_DISTRO}.",
                rdp_gateway::decode_wsl_output(&output.stdout).trim()
            ),
            _ => return Err(message("guacd was not found in WSL after installing it")),
        }
        Ok(())
    }

    async fn import_distro(mirrors: &[String]) -> Result<()> {
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            other => return Err(message(format!("unsupported CPU architecture {other}"))),
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|error| message(error.to_string()))?;
        let mut errors = Vec::new();
        for base in mirrors {
            match download_rootfs(&client, base, arch).await {
                Ok(archive) => {
                    let directory = storage_dir()?.join("wsl");
                    std::fs::create_dir_all(&directory)?;
                    println!(
                        "Importing WSL distro {WSL_DISTRO} into {}...",
                        directory.display()
                    );
                    return blocking(move || import(&directory, &archive)).await?;
                }
                Err(error) => {
                    println!("{base}: {error}");
                    errors.push(error);
                }
            }
        }
        Err(message(format!(
            "could not download Alpine Linux ({}); pass --mirror <url>",
            errors.join("; ")
        )))
    }

    /// Imports as WSL 2, falling back to WSL 1 where virtualization is off
    /// (WSL 1 shares Windows' network, so loopback targets work directly).
    fn import(directory: &Path, archive: &Path) -> Result<()> {
        let attempt = |version: &str| {
            run(
                wsl::command()
                    .args(["--import", WSL_DISTRO])
                    .arg(directory)
                    .arg(archive)
                    .args(["--version", version]),
                "import the WSL distro",
            )
        };
        if attempt("2").is_ok() {
            return Ok(());
        }
        println!("WSL 2 is not available here (the Virtual Machine Platform feature may be off); trying WSL 1...");
        attempt("1").map_err(|error| {
            message(format!(
                "{error}. Enable WSL in an administrator terminal with `wsl --install --no-distribution`, restart Windows, then run `rdp install` again."
            ))
        })
    }

    fn storage_dir() -> Result<PathBuf> {
        Ok(std::env::current_dir()?.join("storage").join("rdp-gateway"))
    }

    async fn download_rootfs(
        client: &reqwest::Client,
        base: &str,
        arch: &str,
    ) -> std::result::Result<PathBuf, String> {
        let releases = format!("{base}/latest-stable/releases/{arch}");
        println!("Fetching {releases}/latest-releases.yaml");
        let index = client
            .get(format!("{releases}/latest-releases.yaml"))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| error.to_string())?
            .text()
            .await
            .map_err(|error| error.to_string())?;
        let release = rdp_gateway::parse_minirootfs(&index)
            .ok_or("the release list has no alpine-minirootfs entry")?;
        let directory = storage_dir().map_err(|error| error.to_string())?;
        let path = directory.join(&release.file);
        // Reuse an earlier download, e.g. after a failed import.
        if std::fs::read(&path).is_ok_and(|bytes| {
            format!("{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(&release.sha256)
        }) {
            println!("Using the downloaded {}", path.display());
            return Ok(path);
        }
        let url = format!("{releases}/{}", release.file);
        println!("Downloading {url}");
        let bytes = client
            .get(&url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| error.to_string())?
            .bytes()
            .await
            .map_err(|error| error.to_string())?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if !digest.eq_ignore_ascii_case(&release.sha256) {
            return Err(format!("checksum mismatch for {}", release.file));
        }
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        std::fs::write(&path, &bytes).map_err(|error| error.to_string())?;
        Ok(path)
    }

    async fn install_packages(mirrors: &[String]) -> Result<()> {
        let release = blocking(|| wsl::shell(wsl::hidden(), "cat /etc/alpine-release").output())
            .await?
            .map_err(|error| message(error.to_string()))?;
        let release = rdp_gateway::decode_wsl_output(&release.stdout);
        let branch = alpine_branch(release.trim())
            .ok_or_else(|| message(format!("unexpected Alpine release {:?}", release.trim())))?;
        for base in mirrors {
            println!("Installing guacd from {base}/{branch} ...");
            let links = rdp_gateway::WSL_PLUGIN_LINKS;
            let script = format!(
                "printf '%s\\n' '{base}/{branch}/main' '{base}/{branch}/community' > /etc/apk/repositories && apk add --no-cache guacamole-server guacamole-server-rdp && {links}"
            );
            let installed = blocking(move || {
                run(&mut wsl::shell(wsl::command(), &script), "install guacd").is_ok()
            })
            .await?;
            if installed {
                return Ok(());
            }
        }
        Err(message(
            "could not install guacd from any mirror; pass --mirror <url>",
        ))
    }

    /// `3.24.2` → `v3.24`.
    fn alpine_branch(release: &str) -> Option<String> {
        let mut parts = release.split('.');
        let major = parts
            .next()
            .filter(|part| part.chars().all(|ch| ch.is_ascii_digit()))?;
        let minor = parts
            .next()
            .filter(|part| part.chars().all(|ch| ch.is_ascii_digit()))?;
        (!major.is_empty() && !minor.is_empty()).then(|| format!("v{major}.{minor}"))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn alpine_branch_from_release() {
            assert_eq!(super::alpine_branch("3.24.2").as_deref(), Some("v3.24"));
            assert_eq!(super::alpine_branch("edge"), None);
        }

        #[test]
        fn custom_mirror_must_be_a_plain_url() {
            assert_eq!(
                super::mirrors(Some("https://mirrors.aliyun.com/alpine/")).unwrap(),
                vec!["https://mirrors.aliyun.com/alpine".to_string()]
            );
            assert!(super::mirrors(Some("https://x.test/a';reboot;'")).is_err());
            assert!(super::mirrors(Some("ftp://x.test/alpine")).is_err());
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{message, Result};
    use crate::services::rdp_gateway;

    #[allow(clippy::unused_async)]
    pub async fn install(_mirror: Option<&str>) -> Result<()> {
        if let Some(path) = rdp_gateway::find_guacd() {
            println!("guacd is already installed: {}", path.display());
            return Ok(());
        }
        Err(message("Homebrew has no guacd package. Build guacamole-server from source (https://guacamole.apache.org/doc/gug/installing-guacamole.html) so `guacd` is on PATH, or set RDP_GUACD_ADDRESS to a guacd on another machine."))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use std::{path::Path, process::Command};

    use super::{message, run, Result};
    use crate::services::rdp_gateway;

    #[allow(clippy::unused_async)]
    pub async fn install(mirror: Option<&str>) -> Result<()> {
        if mirror.is_some() {
            println!("--mirror only applies on Windows; using the system package sources.");
        }
        if let Some(path) = rdp_gateway::find_guacd() {
            println!("guacd is already installed: {}", path.display());
            return link_plugins();
        }
        let steps = package_steps().ok_or_else(|| {
            message("no supported package manager found (apk, apt-get, dnf, yum); install guacd with its RDP plugin manually, or set RDP_GUACD_ADDRESS to a guacd on another machine")
        })?;
        for step in steps {
            println!("$ {}", step.join(" "));
            run(&mut as_root(&step)?, "install guacd").map_err(|error| {
                message(format!(
                    "{error}. Debian 12+/Ubuntu 25+ no longer package guacd; RHEL-based systems need EPEL first (dnf install -y epel-release)."
                ))
            })?;
        }
        let path = rdp_gateway::find_guacd()
            .ok_or_else(|| message("guacd was not found after installing the packages"))?;
        println!("Installed guacd: {}", path.display());
        link_plugins()
    }

    /// Some distributions ship the protocol plugins only as `.so.0`, while
    /// guacd loads `libguac-client-<protocol>.so`.
    fn link_plugins() -> Result<()> {
        for directory in [
            "/usr/lib",
            "/usr/lib64",
            "/usr/local/lib",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
        ] {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(stem) = name
                    .strip_suffix(".so.0")
                    .filter(|stem| stem.starts_with("libguac-client-"))
                else {
                    continue;
                };
                let link = Path::new(directory).join(format!("{stem}.so"));
                if !link.exists() {
                    let link = link.to_string_lossy();
                    println!("$ ln -sf {name} {link}");
                    run(
                        &mut as_root(&["ln", "-sf", &name, &link])?,
                        "link the guacd protocol plugin",
                    )?;
                }
            }
        }
        Ok(())
    }

    fn package_steps() -> Option<Vec<Vec<&'static str>>> {
        let has = |tool: &str| rdp_gateway::find_in_path(tool).is_some();
        if has("apk") {
            Some(vec![vec![
                "apk",
                "add",
                "--no-cache",
                "guacamole-server",
                "guacamole-server-rdp",
            ]])
        } else if has("apt-get") {
            Some(vec![
                vec!["apt-get", "update"],
                vec!["apt-get", "install", "-y", "guacd", "libguac-client-rdp0"],
            ])
        } else {
            ["dnf", "yum"]
                .into_iter()
                .find(|tool| has(tool))
                .map(|tool| vec![vec![tool, "install", "-y", "guacd", "libguac-client-rdp"]])
        }
    }

    fn as_root(step: &[&str]) -> Result<Command> {
        let root = Command::new("id")
            .arg("-u")
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "0");
        if root {
            let mut command = Command::new(step[0]);
            command.args(&step[1..]);
            return Ok(command);
        }
        if rdp_gateway::find_in_path("sudo").is_none() {
            return Err(message(
                "installing packages needs root; run this command as root",
            ));
        }
        // Non-interactive: a password prompt would hang the project CLI page.
        let mut command = Command::new("sudo");
        command.arg("-n").args(step);
        Ok(command)
    }
}
