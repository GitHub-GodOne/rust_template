use anyhow::{anyhow, Result};
use rathole::Cli;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tokio::{fs, sync::broadcast};

static RUNNING: OnceLock<Mutex<HashMap<i32, broadcast::Sender<bool>>>> = OnceLock::new();
fn running() -> &'static Mutex<HashMap<i32, broadcast::Sender<bool>>> {
    RUNNING.get_or_init(|| Mutex::new(HashMap::new()))
}
fn config_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("storage")
        .join("rathole")
}

pub async fn validate(config: &str, mode: &str) -> Result<()> {
    let parsed = rathole::Config::from_str(config)?;
    match mode {
        "server" if parsed.server.is_none() => return Err(anyhow!("配置中缺少 [server]")),
        "client" if parsed.client.is_none() => return Err(anyhow!("配置中缺少 [client]")),
        "auto" if parsed.server.is_some() == parsed.client.is_some() => {
            return Err(anyhow!(
                "auto 模式必须且只能包含 [server] 或 [client] 其中一个"
            ))
        }
        _ => {}
    }
    Ok(())
}

pub async fn start(id: i32, mode: &str, config: &str) -> Result<()> {
    validate(config, mode).await?;
    stop(id).await?;
    let dir = config_dir();
    fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{id}.toml"));
    fs::write(&path, config).await?;
    let (tx, rx) = broadcast::channel(2);
    let cli = Cli {
        config_path: Some(path),
        server: mode == "server",
        client: mode == "client",
        genkey: None,
    };
    tokio::spawn(async move {
        if let Err(error) = rathole::run(cli, rx).await {
            tracing::error!(profile_id = id, %error, "rathole profile stopped with error");
        }
    });
    running()
        .lock()
        .expect("rathole manager mutex poisoned")
        .insert(id, tx);
    Ok(())
}

pub async fn stop(id: i32) -> Result<()> {
    if let Some(tx) = running()
        .lock()
        .expect("rathole manager mutex poisoned")
        .remove(&id)
    {
        let _ = tx.send(true);
    }
    Ok(())
}
pub fn is_running(id: i32) -> bool {
    running()
        .lock()
        .expect("rathole manager mutex poisoned")
        .contains_key(&id)
}

pub async fn start_enabled(db: sea_orm::DatabaseConnection) {
    use crate::models::_entities::rathole_profiles;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    let Ok(items) = rathole_profiles::Entity::find()
        .filter(rathole_profiles::Column::Enabled.eq(true))
        .all(&db)
        .await
    else {
        return;
    };
    for item in items {
        if let Err(error) = start(item.id, &item.mode, &item.config_toml).await {
            tracing::error!(profile_id = item.id, %error, "failed to start enabled rathole profile");
        }
    }
}
