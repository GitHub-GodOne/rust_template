#![allow(clippy::missing_errors_doc)]

use std::time::Duration;

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use crate::{
    errors::{ApiError, ApiResult},
    models::system_settings,
};

pub const HTTP_CLIENT_RUNTIME_KEY: &str = "http_client.runtime";
const MAX_TIMEOUT_SECONDS: u64 = 3600;

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HttpClientRuntimeConfig {
    pub enabled: bool,
    pub request_timeout_seconds: u64,
    pub connect_timeout_seconds: u64,
    pub pool_idle_timeout_seconds: u64,
    pub proxy_enabled: bool,
    pub proxy_url: Option<String>,
    pub danger_accept_invalid_certs: bool,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HttpClientRuntimeOverrides {
    pub request_timeout_seconds: Option<u64>,
}

impl Default for HttpClientRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            request_timeout_seconds: 120,
            connect_timeout_seconds: 20,
            pool_idle_timeout_seconds: 90,
            proxy_enabled: false,
            proxy_url: None,
            danger_accept_invalid_certs: false,
            user_agent: None,
        }
    }
}

impl HttpClientRuntimeConfig {
    pub fn validate(&self) -> ApiResult<()> {
        validate_timeout("request_timeout_seconds", self.request_timeout_seconds)?;
        validate_timeout("connect_timeout_seconds", self.connect_timeout_seconds)?;
        validate_timeout("pool_idle_timeout_seconds", self.pool_idle_timeout_seconds)?;

        if self.proxy_enabled {
            let proxy_url = self
                .proxy_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| ApiError::bad_request("proxy url is required"))?;
            reqwest::Proxy::all(proxy_url)
                .map(|_| ())
                .map_err(|_| ApiError::bad_request("proxy url is invalid"))?;
        }

        Ok(())
    }

    fn effective(&self) -> Self {
        if self.enabled {
            self.clone()
        } else {
            Self::default()
        }
    }
}

pub async fn load_http_client_config(
    db: &DatabaseConnection,
) -> ApiResult<HttpClientRuntimeConfig> {
    let config = system_settings::json_value(
        db,
        HTTP_CLIENT_RUNTIME_KEY,
        HttpClientRuntimeConfig::default(),
    )
    .await?;
    config.validate()?;
    Ok(config)
}

pub async fn build_http_client(db: &DatabaseConnection) -> ApiResult<reqwest::Client> {
    let config = load_http_client_config(db).await?;
    build_client_from_config(&config.effective())
}

pub async fn build_http_client_with_overrides(
    db: &DatabaseConnection,
    overrides: HttpClientRuntimeOverrides,
) -> ApiResult<reqwest::Client> {
    let mut config = load_http_client_config(db).await?.effective();
    if let Some(timeout) = overrides.request_timeout_seconds {
        config.request_timeout_seconds = timeout;
    }
    config.validate()?;
    build_client_from_config(&config)
}

fn build_client_from_config(config: &HttpClientRuntimeConfig) -> ApiResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.request_timeout_seconds))
        .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
        .pool_idle_timeout(Duration::from_secs(config.pool_idle_timeout_seconds))
        .danger_accept_invalid_certs(config.danger_accept_invalid_certs);

    if config.proxy_enabled {
        let proxy_url = config
            .proxy_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::bad_request("proxy url is required"))?;
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|_| ApiError::bad_request("proxy url is invalid"))?;
        builder = builder.proxy(proxy);
    }

    if let Some(user_agent) = config
        .user_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        builder = builder.user_agent(user_agent.to_string());
    }

    builder
        .build()
        .map_err(|_| ApiError::internal("failed to initialize http client"))
}

fn validate_timeout(field: &str, value: u64) -> ApiResult<()> {
    if (1..=MAX_TIMEOUT_SECONDS).contains(&value) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "{field} must be between 1 and {MAX_TIMEOUT_SECONDS}"
        )))
    }
}
