use std::{
    env,
    fmt::{self, Display},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use include_dir::{include_dir, Dir};
use loco_rs::{
    app::{AppContext, Hooks},
    boot::create_context,
    config::Config,
    environment::Environment,
    hash,
};
use migration::{Migrator, MigratorTrait, SchemaManager};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectOptions, ConnectionTrait, Database, DatabaseConnection,
    DbBackend, EntityTrait, IntoActiveModel, PaginatorTrait, QueryFilter, Set, Statement,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    io::AsyncWriteExt,
    net::TcpListener,
    sync::{oneshot, Mutex},
};
use url::Url;
use uuid::Uuid;
use validator::Validate;

use crate::{
    app::App,
    controllers::frontend,
    models::_entities::{user_departments, users},
    responses::ApiResponse,
};

const DEFAULT_SETUP_PORT: u16 = 5150;
const DEFAULT_SETUP_BINDING: &str = "0.0.0.0";
const DEFAULT_CONFIG_FOLDER: &str = "config";
static SETUP_FIXTURES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/fixtures");

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatusData {
    pub configured: bool,
    pub phase: String,
    pub environment: String,
    pub config_file: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone)]
struct SetupLaunch {
    environment: String,
    config_path: PathBuf,
    binding: String,
    port: u16,
}

#[derive(Clone)]
struct SetupState {
    launch: Arc<SetupLaunch>,
    install_lock: Arc<Mutex<()>>,
    shutdown: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    restarting: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Deserialize)]
struct SetupDatabase {
    engine: String,
    host: String,
    port: u16,
    username: String,
    password: String,
    database: String,
    #[serde(default = "default_ssl_mode")]
    ssl_mode: String,
}

#[derive(Debug, Deserialize)]
struct TestDatabaseRequest {
    database: SetupDatabase,
}

#[derive(Debug, Deserialize, Validate)]
struct SetupAdmin {
    #[validate(length(min = 2, max = 80, message = "管理员名称需要 2 到 80 个字符"))]
    name: String,
    #[validate(email(message = "管理员邮箱格式不正确"))]
    email: String,
    #[validate(length(min = 10, max = 128, message = "管理员密码至少需要 10 个字符"))]
    password: String,
}

#[derive(Debug, Deserialize)]
struct InstallRequest {
    database: SetupDatabase,
    admin: SetupAdmin,
}

#[derive(Debug, Serialize)]
struct DatabaseTestResult {
    connected: bool,
    engine: &'static str,
    database_exists: bool,
    initialized: bool,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct InstallResult {
    configured: bool,
    config_file: String,
    migrations_applied: bool,
    default_data_created: bool,
    admin_created: bool,
}

#[derive(Debug)]
struct SetupError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl SetupError {
    fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code,
            message: message.into(),
        }
    }

    fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
        }
    }

    fn internal(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code,
            message: message.into(),
        }
    }
}

impl Display for SetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// Starts the bootstrap server before Loco loads its configuration.
///
/// # Errors
///
/// Returns an error when setup launch options are invalid or the bootstrap
/// server cannot be started.
pub async fn run_if_required() -> loco_rs::Result<()> {
    let Some(launch) = detect_setup_launch()? else {
        return Ok(());
    };

    println!(
        "configuration is missing; setup is available at http://127.0.0.1:{}/setup",
        launch.port
    );
    run_setup_server(launch)
        .await
        .map_err(|error| loco_rs::Error::Message(error.to_string()))
}

fn detect_setup_launch() -> loco_rs::Result<Option<SetupLaunch>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if !args.iter().any(|arg| arg == "start" || arg == "s") {
        return Ok(None);
    }

    let environment = argument_value(&args, "--environment", "-e")
        .or_else(resolve_environment_from_env)
        .unwrap_or_else(|| "development".to_string());
    validate_environment_name(&environment)
        .map_err(|message| loco_rs::Error::Message(message.to_string()))?;

    let config_folder = env::var("LOCO_CONFIG_FOLDER")
        .map_or_else(|_| PathBuf::from(DEFAULT_CONFIG_FOLDER), PathBuf::from);
    let local_path = config_folder.join(format!("{environment}.local.yaml"));
    let standard_path = config_folder.join(format!("{environment}.yaml"));
    if local_path.exists() || standard_path.exists() {
        return Ok(None);
    }

    let port = argument_value(&args, "--port", "-p")
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| loco_rs::Error::Message("setup port must be between 1 and 65535".into()))?
        .unwrap_or(DEFAULT_SETUP_PORT);
    if port == 0 {
        return Err(loco_rs::Error::Message(
            "setup port must be between 1 and 65535".into(),
        ));
    }
    let binding = argument_value(&args, "--binding", "-b")
        .unwrap_or_else(|| DEFAULT_SETUP_BINDING.to_string());

    Ok(Some(SetupLaunch {
        environment,
        config_path: local_path,
        binding,
        port,
    }))
}

fn argument_value(args: &[String], long: &str, short: &str) -> Option<String> {
    for (index, argument) in args.iter().enumerate() {
        if let Some(value) = argument.strip_prefix(&format!("{long}=")) {
            return Some(value.to_string());
        }
        if argument == long || argument == short {
            return args.get(index + 1).cloned();
        }
    }
    None
}

fn resolve_environment_from_env() -> Option<String> {
    ["LOCO_ENV", "RAILS_ENV", "NODE_ENV"]
        .into_iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

fn validate_environment_name(environment: &str) -> Result<(), &'static str> {
    if environment.is_empty()
        || !environment
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err("environment name may only contain letters, numbers, underscores, and hyphens");
    }
    Ok(())
}

async fn run_setup_server(launch: SetupLaunch) -> Result<(), SetupError> {
    let address = format!("{}:{}", launch.binding, launch.port);
    let listener = TcpListener::bind(&address).await.map_err(|error| {
        SetupError::internal(
            "SETUP_BIND_FAILED",
            format!("failed to bind setup server to {address}: {error}"),
        )
    })?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let state = SetupState {
        launch: Arc::new(launch),
        install_lock: Arc::new(Mutex::new(())),
        shutdown: Arc::new(Mutex::new(Some(shutdown_tx))),
        restarting: Arc::new(AtomicBool::new(false)),
    };
    let router = Router::new()
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup/test", post(test_database))
        .route("/api/setup/complete", post(complete_setup))
        .route("/", get(setup_index))
        .route("/{*path}", get(setup_asset))
        .with_state(state);

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        })
        .await
        .map_err(|error| {
            SetupError::internal(
                "SETUP_SERVER_FAILED",
                format!("setup server stopped unexpectedly: {error}"),
            )
        })
}

async fn setup_status(State(state): State<SetupState>) -> Response {
    setup_ok(
        "SETUP_REQUIRED",
        "configuration is required",
        SetupStatusData {
            configured: false,
            phase: if state.restarting.load(Ordering::SeqCst) {
                "restarting".to_string()
            } else {
                "setup".to_string()
            },
            environment: state.launch.environment.clone(),
            config_file: Some(state.launch.config_path.display().to_string()),
            port: Some(state.launch.port),
        },
    )
}

async fn test_database(
    State(_state): State<SetupState>,
    Json(request): Json<TestDatabaseRequest>,
) -> Response {
    match probe_database(&request.database).await {
        Ok(result) => setup_ok(
            "DATABASE_CONNECTED",
            "database connection succeeded",
            result,
        ),
        Err(error) => setup_error_response(error),
    }
}

async fn complete_setup(
    State(state): State<SetupState>,
    Json(request): Json<InstallRequest>,
) -> Response {
    let Ok(_guard) = state.install_lock.try_lock() else {
        return setup_error_response(SetupError::conflict(
            "SETUP_IN_PROGRESS",
            "installation is already running",
        ));
    };
    if state.launch.config_path.exists() {
        return setup_error_response(SetupError::conflict(
            "ALREADY_CONFIGURED",
            "configuration file already exists; restart the application",
        ));
    }
    if let Err(error) = request.admin.validate() {
        let message = error
            .field_errors()
            .values()
            .flat_map(|errors| errors.iter())
            .find_map(|error| error.message.as_deref())
            .unwrap_or("administrator information is invalid");
        return setup_error_response(SetupError::bad_request("INVALID_ADMIN", message.to_owned()));
    }

    match install(&state.launch, request).await {
        Ok(result) => {
            state.restarting.store(true, Ordering::SeqCst);
            let shutdown = state.shutdown.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(750)).await;
                let sender = shutdown.lock().await.take();
                if let Some(sender) = sender {
                    let _ = sender.send(());
                }
            });
            setup_ok("SETUP_COMPLETE", "installation completed", result)
        }
        Err(error) => setup_error_response(error),
    }
}

async fn setup_index() -> Response {
    frontend::frontend_response("index.html", false)
}

async fn setup_asset(AxumPath(path): AxumPath<String>) -> Response {
    let path = path.trim_start_matches('/');
    if frontend::is_backend_path(path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if path.is_empty() {
        return frontend::frontend_response("index.html", false);
    }
    let response = frontend::frontend_response(path, true);
    if response.status() == StatusCode::NOT_FOUND {
        frontend::frontend_response("index.html", false)
    } else {
        response
    }
}

async fn install(
    launch: &SetupLaunch,
    request: InstallRequest,
) -> Result<InstallResult, SetupError> {
    request.database.validate()?;
    let uri = request.database.connection_uri()?;
    let database = ensure_database(&request.database).await?;

    Migrator::up(&database, None).await.map_err(|error| {
        SetupError::internal(
            "MIGRATION_FAILED",
            format!("failed to create database tables: {error}"),
        )
    })?;
    database.close().await.map_err(|error| {
        SetupError::internal(
            "DATABASE_CLOSE_FAILED",
            format!("failed to finish database initialization: {error}"),
        )
    })?;

    let config_yaml = generated_config(launch, &uri)?;
    let config: Config = serde_yaml::from_str(&config_yaml).map_err(|error| {
        SetupError::internal(
            "CONFIG_GENERATION_FAILED",
            format!("generated configuration is invalid: {error}"),
        )
    })?;
    let environment = Environment::from(launch.environment.clone());
    let context = create_context::<App>(&environment, config)
        .await
        .map_err(|error| {
            SetupError::internal(
                "DATABASE_CONTEXT_FAILED",
                format!("failed to initialize application database context: {error}"),
            )
        })?;

    let existing_users = users::Entity::find()
        .count(&context.db)
        .await
        .map_err(database_error("USER_CHECK_FAILED", &request.database))?;
    let initialized_defaults = if existing_users == 0 {
        seed_embedded_defaults(&context).await?;
        configure_initial_admin(&context.db, &request.admin, &request.database).await?;
        true
    } else {
        false
    };

    write_config_atomically(&launch.config_path, config_yaml.as_bytes()).await?;
    Ok(InstallResult {
        configured: true,
        config_file: launch.config_path.display().to_string(),
        migrations_applied: true,
        default_data_created: initialized_defaults,
        admin_created: initialized_defaults,
    })
}

async fn configure_initial_admin(
    database: &DatabaseConnection,
    admin: &SetupAdmin,
    database_config: &SetupDatabase,
) -> Result<(), SetupError> {
    user_departments::Entity::delete_many()
        .filter(user_departments::Column::UserId.is_in([1, 2]))
        .exec(database)
        .await
        .map_err(database_error("DEMO_DATA_CLEANUP_FAILED", database_config))?;
    users::Entity::delete_many()
        .filter(users::Column::Id.is_in([1, 2]))
        .exec(database)
        .await
        .map_err(database_error("DEMO_DATA_CLEANUP_FAILED", database_config))?;

    let user = users::Entity::find_by_id(3)
        .one(database)
        .await
        .map_err(database_error("ADMIN_SETUP_FAILED", database_config))?
        .ok_or_else(|| {
            SetupError::internal(
                "ADMIN_SETUP_FAILED",
                "default administrator record was not created",
            )
        })?;
    let password = hash::hash_password(&admin.password).map_err(|error| {
        SetupError::internal(
            "ADMIN_PASSWORD_FAILED",
            format!("failed to secure administrator password: {error}"),
        )
    })?;
    let mut active = user.into_active_model();
    active.name = Set(admin.name.trim().to_string());
    active.email = Set(admin.email.trim().to_ascii_lowercase());
    active.password = Set(password);
    active.api_key = Set(format!("lo-{}", Uuid::new_v4()));
    active.email_verified_at = Set(Some(Utc::now().fixed_offset()));
    active.failed_login_attempts = Set(0);
    active.locked_until = Set(None);
    active.last_failed_login_at = Set(None);
    active.tenant_id = Set(Some(1));
    active.current_department_id = Set(Some(1));
    active
        .update(database)
        .await
        .map_err(database_error("ADMIN_SETUP_FAILED", database_config))?;
    Ok(())
}

async fn probe_database(database: &SetupDatabase) -> Result<DatabaseTestResult, SetupError> {
    database.validate()?;
    let target_uri = database.connection_uri()?;
    match connect(&target_uri).await {
        Ok(connection) => {
            let initialized = SchemaManager::new(&connection)
                .has_table("users")
                .await
                .map_err(database_error("DATABASE_TEST_FAILED", database))?;
            connection
                .close()
                .await
                .map_err(database_error("DATABASE_TEST_FAILED", database))?;
            Ok(DatabaseTestResult {
                connected: true,
                engine: "postgres",
                database_exists: true,
                initialized,
            })
        }
        Err(target_error) => {
            let maintenance =
                connect(&database.maintenance_uri()?)
                    .await
                    .map_err(|maintenance_error| {
                        SetupError::bad_request(
                            "DATABASE_CONNECTION_FAILED",
                            format!(
                                "failed to connect to PostgreSQL: {}",
                                sanitize_database_error(&maintenance_error, database)
                            ),
                        )
                    })?;
            let exists = postgres_database_exists(&maintenance, &database.database)
                .await
                .map_err(database_error("DATABASE_TEST_FAILED", database))?;
            maintenance
                .close()
                .await
                .map_err(database_error("DATABASE_TEST_FAILED", database))?;
            if exists {
                return Err(SetupError::bad_request(
                    "DATABASE_CONNECTION_FAILED",
                    format!(
                        "database exists but cannot be opened: {}",
                        sanitize_database_error(&target_error, database)
                    ),
                ));
            }
            Ok(DatabaseTestResult {
                connected: true,
                engine: "postgres",
                database_exists: false,
                initialized: false,
            })
        }
    }
}

async fn ensure_database(database: &SetupDatabase) -> Result<DatabaseConnection, SetupError> {
    let target_uri = database.connection_uri()?;
    if let Ok(connection) = connect(&target_uri).await {
        return Ok(connection);
    }

    let maintenance = connect(&database.maintenance_uri()?)
        .await
        .map_err(database_error("DATABASE_CONNECTION_FAILED", database))?;
    if !postgres_database_exists(&maintenance, &database.database)
        .await
        .map_err(database_error("DATABASE_CREATE_FAILED", database))?
    {
        maintenance
            .execute_unprepared(&format!(
                "CREATE DATABASE \"{}\" ENCODING 'UTF8' TEMPLATE template0",
                database.database
            ))
            .await
            .map_err(database_error("DATABASE_CREATE_FAILED", database))?;
    }
    maintenance
        .close()
        .await
        .map_err(database_error("DATABASE_CREATE_FAILED", database))?;
    connect(&target_uri)
        .await
        .map_err(database_error("DATABASE_CONNECTION_FAILED", database))
}

async fn postgres_database_exists(
    connection: &DatabaseConnection,
    database: &str,
) -> Result<bool, sea_orm::DbErr> {
    let result = connection
        .query_one(Statement::from_string(
            DbBackend::Postgres,
            format!(
                "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = '{database}') AS exists"
            ),
        ))
        .await?;
    result.map_or(Ok(false), |row| row.try_get("", "exists"))
}

async fn connect(uri: &str) -> Result<DatabaseConnection, sea_orm::DbErr> {
    let mut options = ConnectOptions::new(uri.to_string());
    options
        .min_connections(0)
        .max_connections(2)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(30))
        .sqlx_logging(false);
    Database::connect(options).await
}

impl SetupDatabase {
    fn validate(&self) -> Result<(), SetupError> {
        if self.engine != "postgres" {
            return Err(SetupError::bad_request(
                "UNSUPPORTED_DATABASE",
                "this application currently supports PostgreSQL setup only",
            ));
        }
        if self.host.trim().is_empty() {
            return Err(SetupError::bad_request(
                "INVALID_DATABASE_HOST",
                "PostgreSQL host is required",
            ));
        }
        if self.username.trim().is_empty() {
            return Err(SetupError::bad_request(
                "INVALID_DATABASE_USER",
                "PostgreSQL username is required",
            ));
        }
        validate_database_name(&self.database)?;
        if !matches!(
            self.ssl_mode.as_str(),
            "disable" | "prefer" | "require" | "verify-ca" | "verify-full"
        ) {
            return Err(SetupError::bad_request(
                "INVALID_SSL_MODE",
                "unsupported PostgreSQL SSL mode",
            ));
        }
        Ok(())
    }

    fn connection_uri(&self) -> Result<String, SetupError> {
        postgres_uri(
            &self.host,
            self.port,
            &self.username,
            &self.password,
            &self.database,
            &self.ssl_mode,
        )
    }

    fn maintenance_uri(&self) -> Result<String, SetupError> {
        postgres_uri(
            &self.host,
            self.port,
            &self.username,
            &self.password,
            "postgres",
            &self.ssl_mode,
        )
    }

    fn password(&self) -> Option<&str> {
        (!self.password.is_empty()).then_some(self.password.as_str())
    }
}

fn default_ssl_mode() -> String {
    "prefer".to_string()
}

fn validate_database_name(database: &str) -> Result<(), SetupError> {
    let valid = !database.is_empty()
        && database.len() <= 63
        && database
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && database
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid {
        return Err(SetupError::bad_request(
            "INVALID_DATABASE_NAME",
            "database name must start with a letter or underscore and contain only letters, numbers, and underscores",
        ));
    }
    Ok(())
}

fn postgres_uri(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    database: &str,
    ssl_mode: &str,
) -> Result<String, SetupError> {
    let mut url = Url::parse("postgres://localhost").map_err(|error| {
        SetupError::internal(
            "DATABASE_URL_FAILED",
            format!("failed to initialize PostgreSQL URL: {error}"),
        )
    })?;
    url.set_host(Some(host.trim())).map_err(|_| {
        SetupError::bad_request("INVALID_DATABASE_HOST", "PostgreSQL host is invalid")
    })?;
    url.set_port(Some(port)).map_err(|()| {
        SetupError::bad_request("INVALID_DATABASE_PORT", "PostgreSQL port is invalid")
    })?;
    url.set_username(username.trim()).map_err(|()| {
        SetupError::bad_request("INVALID_DATABASE_USER", "PostgreSQL username is invalid")
    })?;
    url.set_password((!password.is_empty()).then_some(password))
        .map_err(|()| {
            SetupError::bad_request(
                "INVALID_DATABASE_PASSWORD",
                "PostgreSQL password contains unsupported characters",
            )
        })?;
    url.set_path(&format!("/{database}"));
    url.query_pairs_mut().append_pair("sslmode", ssl_mode);
    Ok(url.into())
}

fn generated_config(launch: &SetupLaunch, database_uri: &str) -> Result<String, SetupError> {
    let jwt_secret = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    serde_yaml::to_string(&json!({
        "logger": {
            "enable": true,
            "pretty_backtrace": launch.environment != "production",
            "level": if launch.environment == "production" { "info" } else { "debug" },
            "format": "compact"
        },
        "server": {
            "binding": launch.binding,
            "port": launch.port,
            "host": "http://localhost"
        },
        "workers": {
            "mode": "BackgroundAsync"
        },
        "database": {
            "uri": database_uri,
            "enable_logging": false,
            "connect_timeout": 10000,
            "idle_timeout": 600_000,
            "acquire_timeout": 10000,
            "min_connections": 1,
            "max_connections": 10,
            "auto_migrate": true,
            "dangerously_truncate": false,
            "dangerously_recreate": false
        },
        "auth": {
            "jwt": {
                "secret": jwt_secret,
                "expiration": 604_800
            }
        }
    }))
    .map_err(|error| {
        SetupError::internal(
            "CONFIG_GENERATION_FAILED",
            format!("failed to serialize configuration: {error}"),
        )
    })
}

async fn seed_embedded_defaults(context: &AppContext) -> Result<(), SetupError> {
    let fixture_root = env::temp_dir().join(format!(
        "gpt-images-setup-fixtures-{}",
        Uuid::new_v4().simple()
    ));
    if let Err(error) = materialize_setup_fixtures(&fixture_root).await {
        let _ = tokio::fs::remove_dir_all(&fixture_root).await;
        return Err(SetupError::internal(
            "DEFAULT_DATA_FAILED",
            format!("failed to prepare embedded default system data: {error}"),
        ));
    }

    let seed_result = <App as Hooks>::seed(context, &fixture_root).await;
    if let Err(error) = tokio::fs::remove_dir_all(&fixture_root).await {
        tracing::warn!(
            path = %fixture_root.display(),
            %error,
            "failed to remove temporary setup fixtures"
        );
    }
    seed_result.map_err(|error| {
        SetupError::internal(
            "DEFAULT_DATA_FAILED",
            format!("failed to create default system data: {error}"),
        )
    })
}

async fn materialize_setup_fixtures(destination: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(destination).await?;
    for file in SETUP_FIXTURES.files() {
        let output = destination.join(file.path());
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(output, file.contents()).await?;
    }
    Ok(())
}

async fn write_config_atomically(path: &Path, content: &[u8]) -> Result<(), SetupError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    tokio::fs::create_dir_all(parent).await.map_err(|error| {
        SetupError::internal(
            "CONFIG_WRITE_FAILED",
            format!("failed to create configuration directory: {error}"),
        )
    })?;
    let temporary = parent.join(format!(".setup-{}.tmp", Uuid::new_v4()));
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await
        .map_err(|error| {
            SetupError::internal(
                "CONFIG_WRITE_FAILED",
                format!("failed to create temporary configuration: {error}"),
            )
        })?;
    file.write_all(content).await.map_err(|error| {
        SetupError::internal(
            "CONFIG_WRITE_FAILED",
            format!("failed to write configuration: {error}"),
        )
    })?;
    file.sync_all().await.map_err(|error| {
        SetupError::internal(
            "CONFIG_WRITE_FAILED",
            format!("failed to flush configuration: {error}"),
        )
    })?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|error| {
                SetupError::internal(
                    "CONFIG_WRITE_FAILED",
                    format!("failed to secure configuration permissions: {error}"),
                )
            })?;
    }
    tokio::fs::rename(&temporary, path).await.map_err(|error| {
        SetupError::internal(
            "CONFIG_WRITE_FAILED",
            format!("failed to activate configuration: {error}"),
        )
    })
}

fn database_error<'a, E>(
    code: &'static str,
    database: &'a SetupDatabase,
) -> impl FnOnce(E) -> SetupError + 'a
where
    E: Display,
{
    move |error| {
        SetupError::bad_request(code, sanitize_database_error(&error.to_string(), database))
    }
}

fn sanitize_database_error(error: &impl Display, database: &SetupDatabase) -> String {
    let mut message = error.to_string();
    if let Some(password) = database.password() {
        message = message.replace(password, "******");
    }
    message
}

fn setup_ok<T>(code: &str, message: &str, data: T) -> Response
where
    T: Serialize,
{
    Json(ApiResponse {
        success: true,
        code: code.to_string(),
        message: message.to_string(),
        data: Some(data),
    })
    .into_response()
}

fn setup_error_response(error: SetupError) -> Response {
    (
        error.status,
        Json(ApiResponse::<serde_json::Value> {
            success: false,
            code: error.code.to_string(),
            message: error.message,
            data: None,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::env;

    use loco_rs::config::Config;
    use uuid::Uuid;

    use super::{
        generated_config, materialize_setup_fixtures, postgres_uri, validate_database_name,
        SetupLaunch, SETUP_FIXTURES,
    };

    #[test]
    fn postgres_uri_encodes_credentials() {
        let uri = postgres_uri(
            "127.0.0.1",
            5432,
            "setup@example.com",
            "p@ss:/word",
            "gpt_images",
            "require",
        )
        .expect("valid PostgreSQL URL");
        assert!(uri.contains("setup%40example.com:p%40ss%3A%2Fword@"));
        assert!(uri.ends_with("/gpt_images?sslmode=require"));
    }

    #[test]
    fn rejects_unsafe_database_names() {
        assert!(validate_database_name("gpt_images").is_ok());
        assert!(validate_database_name("gpt-images").is_err());
        assert!(validate_database_name("db;DROP DATABASE postgres").is_err());
    }

    #[test]
    fn generated_yaml_is_a_valid_loco_config() {
        let launch = SetupLaunch {
            environment: "production".to_string(),
            config_path: "config/production.local.yaml".into(),
            binding: "0.0.0.0".to_string(),
            port: 5150,
        };
        let uri = "postgres://postgres:secret@127.0.0.1:5432/app?sslmode=prefer";
        let yaml = generated_config(&launch, uri).expect("configuration generation");
        let config = serde_yaml::from_str::<Config>(&yaml).expect("valid Loco configuration");
        assert_eq!(config.server.port, 5150);
        assert_eq!(config.database.uri, uri);
        assert!(config.database.auto_migrate);
    }

    #[tokio::test]
    async fn materializes_embedded_setup_fixtures() {
        let destination = env::temp_dir().join(format!(
            "gpt-images-setup-fixtures-test-{}",
            Uuid::new_v4().simple()
        ));

        materialize_setup_fixtures(&destination)
            .await
            .expect("embedded fixtures should materialize");

        let users = tokio::fs::read(destination.join("users.yaml"))
            .await
            .expect("users fixture should exist");
        assert_eq!(
            users,
            SETUP_FIXTURES
                .get_file("users.yaml")
                .expect("users fixture should be embedded")
                .contents()
        );

        tokio::fs::remove_dir_all(destination)
            .await
            .expect("temporary fixtures should be removable");
    }
}
