//! Cross-platform PostgreSQL database export and import commands.

#![allow(clippy::missing_errors_doc)]

use std::{env, path::PathBuf, process::Command};

use clap::{error::ErrorKind, Args, CommandFactory, Parser, Subcommand};
use loco_rs::{Error, Result};

#[derive(Debug, Parser)]
#[command(name = "data", about = "Export and import application PostgreSQL data")]
struct DataCli {
    #[command(subcommand)]
    command: DataCommand,
}

#[derive(Debug, Subcommand)]
enum DataCommand {
    /// Export the configured PostgreSQL database to a custom-format dump.
    Export(DatabaseArgs),
    /// Import a custom-format dump into the configured PostgreSQL database.
    Import(ImportArgs),
}

#[derive(Debug, Args)]
struct DatabaseArgs {
    /// PostgreSQL connection URL; overrides the project config.
    #[arg(long)]
    database_url: Option<String>,
    /// Loco environment whose config should be read first.
    #[arg(short = 'e', long, default_value = "development")]
    environment: String,
    /// Output dump path. Defaults to gpt-images-<timestamp>.dump.
    #[arg(short, long)]
    file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ImportArgs {
    /// PostgreSQL connection URL; overrides the project config.
    #[arg(long)]
    database_url: Option<String>,
    /// Loco environment whose config should be read first.
    #[arg(short = 'e', long, default_value = "development")]
    environment: String,
    /// Custom-format dump path created by `data export`.
    #[arg(short, long)]
    file: PathBuf,
}

/// Clap definition of this command, listed on the project CLI page.
#[must_use]
pub fn cli_command() -> clap::Command {
    DataCli::command()
}

pub fn handle_if_requested() -> Result<bool> {
    let mut arguments = env::args_os();
    let _binary = arguments.next();
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("data")) {
        return Ok(false);
    }
    let parser_arguments = std::iter::once("data".to_string())
        .chain(arguments.map(|value| value.to_string_lossy().into_owned()));
    let cli = match DataCli::try_parse_from(parser_arguments) {
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

fn execute(command: DataCommand) -> Result<()> {
    match command {
        DataCommand::Export(args) => export(args),
        DataCommand::Import(args) => import(args),
    }
}

fn export(args: DatabaseArgs) -> Result<()> {
    let file = args.file.unwrap_or_else(default_dump_path);
    if file.exists() {
        return Err(message(format!(
            "refusing to overwrite existing dump file: {}",
            file.display()
        )));
    }
    let database_url = resolve_database_url(args.database_url.as_deref(), &args.environment)?;
    run_dump("pg_dump", &database_url, &file)
}

fn import(args: ImportArgs) -> Result<()> {
    if !args.file.is_file() {
        return Err(message(format!(
            "dump file does not exist: {}",
            args.file.display()
        )));
    }
    eprintln!(
        "Importing {} into the target database.",
        args.file.display()
    );
    let database_url = resolve_database_url(args.database_url.as_deref(), &args.environment)?;
    let status = Command::new("pg_restore")
        .args(["--exit-on-error", "--no-owner", "--no-acl", "--dbname"])
        .arg(&database_url)
        .arg(&args.file)
        .status()
        .map_err(|error| message(format!("failed to run pg_restore: {error}")))?;
    if !status.success() {
        return Err(message(format!("pg_restore exited with {status}")));
    }
    println!("Database imported from {}", args.file.display());
    Ok(())
}

fn run_dump(program: &str, database_url: &str, file: &PathBuf) -> Result<()> {
    let status = Command::new(program)
        .args(["--format=custom", "--no-owner", "--no-acl", "--dbname"])
        .arg(database_url)
        .arg("--file")
        .arg(file)
        .status()
        .map_err(|error| message(format!("failed to run {program}: {error}")))?;
    if !status.success() {
        return Err(message(format!("{program} exited with {status}")));
    }
    println!("Database exported to {}", file.display());
    Ok(())
}

fn default_dump_path() -> PathBuf {
    format!(
        "gpt-images-{}.dump",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    )
    .into()
}

fn resolve_database_url(explicit: Option<&str>, environment: &str) -> Result<String> {
    let config_folder = env::var_os("LOCO_CONFIG_FOLDER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config"));
    for path in [
        config_folder.join(format!("{environment}.local.yaml")),
        config_folder.join(format!("{environment}.yaml")),
    ] {
        if let Some(url) = database_url_from_config(&path)? {
            return Ok(url);
        }
    }
    if let Some(url) = explicit.map(str::trim).filter(|url| !url.is_empty()) {
        return Ok(url.to_string());
    }
    if let Some(url) = env::var("DATABASE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(url);
    }
    Err(message(format!(
        "database URL not found; configure config/{environment}.local.yaml or pass --database-url"
    )))
}

fn database_url_from_config(path: &PathBuf) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|error| message(format!("failed to read {}: {error}", path.display())))?;
    let Some(value) = contents
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("uri:").map(str::trim))
    else {
        return Ok(None);
    };
    let value = value.trim_matches(|character| character == '\'' || character == '"');
    if let Some(start) = value.find("default=") {
        let default = &value[start + "default=".len()..];
        let default = default.trim_matches(|character| {
            character == '\'' || character == '"' || character == ')' || character == ' '
        });
        return Ok((!default.is_empty() && !default.contains("{{")).then(|| default.to_string()));
    }
    Ok((!value.is_empty() && !value.contains("{{")).then(|| value.to_string()))
}

fn message(value: impl Into<String>) -> Error {
    Error::Message(value.into())
}
