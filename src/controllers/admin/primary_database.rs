//! Primary application database lifecycle.
//!
//! The application owns one `ctx.db` for its entire process lifetime. A
//! primary database change therefore prepares the target, writes a local
//! configuration override, and asks the caller to restart the service. It
//! deliberately never swaps the connection inside a running process.

#![allow(clippy::missing_errors_doc)]

use std::{env, fs, path::PathBuf, time::Duration};

use loco_rs::prelude::*;
use migration::{Migrator, MigratorTrait};
use sea_orm::{
    ActiveModelTrait, ConnectOptions, ConnectionTrait, Database, EntityTrait, IntoActiveModel, Set,
};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use tokio::time::sleep;
use url::Url;

use crate::{
    app::App,
    controllers::admin::{authorize, require_reauth, SessionExt},
    errors::{ApiError, ApiResult},
    models::_entities::users,
    responses::{self, ApiResponse},
    setup::seed_embedded_defaults,
};

pub const REINITIALIZE_CONFIRM_PHRASE: &str = "REINITIALIZE DATABASE";

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PrimaryDatabaseStatus {
    pub engine: String,
    pub masked_uri: String,
    pub environment: String,
    pub override_file: String,
    pub restart_required: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ApplyPrimaryDatabaseParams {
    /// Saved PostgreSQL connection. Omit to keep the current URI.
    pub connection_id: Option<i32>,
    #[serde(default)]
    pub reinitialize: bool,
    pub confirm_phrase: Option<String>,
    /// When true, exits after writing the override so a service supervisor can restart it.
    #[serde(default)]
    pub restart: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApplyPrimaryDatabaseResult {
    pub engine: String,
    pub masked_uri: String,
    pub override_file: String,
    pub migrations_applied: bool,
    pub reinitialized: bool,
    pub restart_scheduled: bool,
}

#[utoipa::path(
    get,
    path = "/api/admin/primary-database",
    tag = "admin-primary-database",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<PrimaryDatabaseStatus>))
)]
#[debug_handler]
pub async fn status(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:database:manage").await?;
    let uri = ctx.config.database.uri.clone();
    let path = override_path(&ctx);
    Ok(responses::ok(PrimaryDatabaseStatus {
        engine: database_engine(&uri),
        masked_uri: mask_uri(&uri),
        environment: ctx.environment.to_string(),
        override_file: path.display().to_string(),
        restart_required: false,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/primary-database/apply",
    tag = "admin-primary-database",
    security(("bearer_auth" = [])),
    request_body = ApplyPrimaryDatabaseParams,
    responses((status = 200, body = ApiResponse<ApplyPrimaryDatabaseResult>))
)]
#[debug_handler]
pub async fn apply(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    session: SessionExt,
    Json(params): Json<ApplyPrimaryDatabaseParams>,
) -> ApiResult<Response> {
    let actor = authorize(&ctx, &auth, "system:database:manage").await?;
    require_reauth(&ctx, &session).await?;
    if params.reinitialize
        && params.confirm_phrase.as_deref().map(str::trim) != Some(REINITIALIZE_CONFIRM_PHRASE)
    {
        return Err(ApiError::bad_request(
            "重建数据库需要输入确认短语 REINITIALIZE DATABASE",
        ));
    }

    let target_uri = if let Some(connection_id) = params.connection_id {
        let (uri, _, _) = crate::controllers::admin::backups::resolve_backup_database(
            &ctx,
            actor.id,
            Some(connection_id),
        )
        .await?;
        uri
    } else {
        ctx.config.database.uri.clone()
    };
    if !is_postgres(&target_uri) {
        return Err(ApiError::bad_request(
            "主库切换当前只支持 PostgreSQL；SQLite 连接仍可在数据库管理中作为外部连接使用",
        ));
    }

    let current_user = if params.reinitialize {
        users::Entity::find_by_id(actor.id).one(&ctx.db).await?
    } else {
        None
    };
    let database = connect_target(&target_uri).await?;
    if params.reinitialize {
        database
            .execute_unprepared("DROP SCHEMA public CASCADE")
            .await
            .map_err(|error| ApiError::bad_request(format!("删除目标数据库结构失败：{error}")))?;
        database
            .execute_unprepared("CREATE SCHEMA public")
            .await
            .map_err(|error| ApiError::bad_request(format!("创建目标数据库结构失败：{error}")))?;
    }
    Migrator::up(&database, None)
        .await
        .map_err(|error| ApiError::bad_request(format!("数据库迁移失败：{error}")))?;

    if params.reinitialize {
        let mut seed_config = ctx.config.clone();
        seed_config.database.uri = target_uri.clone();
        let seed_context = loco_rs::boot::create_context::<App>(&ctx.environment, seed_config)
            .await
            .map_err(|error| ApiError::internal(format!("初始化新数据库上下文失败：{error}")))?;
        seed_embedded_defaults(&seed_context)
            .await
            .map_err(|error| ApiError::internal(format!("写入默认后台数据失败：{error}")))?;
        if let Some(user) = current_user {
            let seeded = users::Entity::find_by_id(3)
                .one(&seed_context.db)
                .await?
                .ok_or_else(|| ApiError::internal("默认管理员记录不存在"))?;
            let mut active = seeded.into_active_model();
            active.email = Set(user.email);
            active.password = Set(user.password);
            active.name = Set(user.name);
            active.api_key = Set(user.api_key);
            active.email_verified_at = Set(user.email_verified_at);
            active.tenant_id = Set(user.tenant_id.or(Some(1)));
            active.current_department_id = Set(user.current_department_id.or(Some(1)));
            active.update(&seed_context.db).await?;
        }
    }
    database.close().await.ok();

    let path = override_path(&ctx);
    write_database_override(&path, &target_uri)?;
    let result = ApplyPrimaryDatabaseResult {
        engine: database_engine(&target_uri),
        masked_uri: mask_uri(&target_uri),
        override_file: path.display().to_string(),
        migrations_applied: true,
        reinitialized: params.reinitialize,
        restart_scheduled: params.restart,
    };
    if params.restart {
        tokio::spawn(async {
            sleep(Duration::from_millis(750)).await;
            std::process::exit(1);
        });
    }
    Ok(responses::ok(result))
}

fn is_postgres(uri: &str) -> bool {
    Url::parse(uri)
        .map(|url| matches!(url.scheme(), "postgres" | "postgresql"))
        .unwrap_or(false)
}

fn database_engine(uri: &str) -> String {
    Url::parse(uri)
        .map(|url| match url.scheme() {
            "postgres" | "postgresql" => "postgres".to_string(),
            scheme => scheme.to_string(),
        })
        .unwrap_or_else(|_| "unknown".to_string())
}

fn mask_uri(uri: &str) -> String {
    let Ok(mut parsed) = Url::parse(uri) else {
        return "<invalid database uri>".to_string();
    };
    if parsed.password().is_some() {
        let _ = parsed.set_password(Some("******"));
    }
    parsed.to_string()
}

fn override_path(ctx: &AppContext) -> PathBuf {
    let folder = env::var_os("LOCO_CONFIG_FOLDER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config"));
    folder.join(format!("{}.local.yaml", ctx.environment))
}

fn write_database_override(path: &PathBuf, uri: &str) -> ApiResult<()> {
    let mut root = if path.is_file() {
        let content = fs::read_to_string(path)
            .map_err(|error| ApiError::internal(format!("读取配置文件失败：{error}")))?;
        serde_yaml::from_str::<Value>(&content)
            .map_err(|error| ApiError::internal(format!("解析配置文件失败：{error}")))?
    } else {
        Value::Mapping(Mapping::new())
    };
    let root_map = root
        .as_mapping_mut()
        .ok_or_else(|| ApiError::internal("配置文件根节点必须是 YAML 对象"))?;
    let database_key = Value::String("database".to_string());
    let database = root_map
        .entry(database_key)
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    let database_map = database
        .as_mapping_mut()
        .ok_or_else(|| ApiError::internal("配置文件 database 节点必须是 YAML 对象"))?;
    database_map.insert(
        Value::String("uri".to_string()),
        Value::String(uri.to_string()),
    );
    let content = serde_yaml::to_string(&root)
        .map_err(|error| ApiError::internal(format!("生成配置文件失败：{error}")))?;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| ApiError::internal(format!("创建配置目录失败：{error}")))?;
    let temporary = parent.join(format!(".primary-database-{}.tmp", std::process::id()));
    fs::write(&temporary, content)
        .map_err(|error| ApiError::internal(format!("写入配置文件失败：{error}")))?;
    fs::rename(&temporary, path)
        .map_err(|error| ApiError::internal(format!("替换配置文件失败：{error}")))?;
    Ok(())
}

async fn connect_target(uri: &str) -> ApiResult<DatabaseConnection> {
    let mut options = ConnectOptions::new(uri.to_string());
    options
        .min_connections(0)
        .max_connections(2)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .idle_timeout(Duration::from_secs(30))
        .sqlx_logging(false);
    Database::connect(options)
        .await
        .map_err(|error| ApiError::bad_request(format!("无法连接目标 PostgreSQL：{error}")))
}
