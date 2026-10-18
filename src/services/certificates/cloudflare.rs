//! Minimal Cloudflare API client for certificate DNS challenges, A/AAAA
//! records and Cloudflare Tunnel management.

use std::collections::HashMap;

use sea_orm::DatabaseConnection;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::services::{
    cloudflare_ddns::{cloudflare_auth_headers, zone_candidates},
    http_client::{build_http_client_with_overrides, HttpClientRuntimeOverrides},
};

const REQUEST_TIMEOUT_SECONDS: u64 = 30;

/// Effective Cloudflare credentials (certificate settings, falling back to DDNS).
#[derive(Clone, Debug, Default)]
pub struct CloudflareCredentials {
    pub api_base: String,
    pub auth_mode: String,
    pub api_token: String,
    pub api_email: String,
}

#[derive(Deserialize)]
struct Envelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<Message>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct Message {
    code: Option<i64>,
    message: String,
}

#[derive(Deserialize)]
struct Zone {
    id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub content: String,
    #[serde(default)]
    pub proxied: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tunnel {
    pub id: String,
    pub name: String,
}

/// Desired state of one DNS record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesiredRecord {
    pub record_type: &'static str,
    pub name: String,
    pub content: String,
    pub proxied: bool,
}

pub struct CloudflareApi {
    http: reqwest::Client,
    api_base: String,
    auth_headers: reqwest::header::HeaderMap,
    zones: tokio::sync::Mutex<HashMap<String, String>>,
}

impl CloudflareApi {
    pub async fn new(
        db: &DatabaseConnection,
        credentials: &CloudflareCredentials,
    ) -> Result<Self, String> {
        if credentials.api_token.trim().is_empty() {
            return Err(
                "未配置 Cloudflare API Token：请在证书设置或 IPv6 动态解析中填写".to_string(),
            );
        }
        let auth_headers = cloudflare_auth_headers(
            &credentials.auth_mode,
            &credentials.api_token,
            &credentials.api_email,
        )?;
        let http = build_http_client_with_overrides(
            db,
            HttpClientRuntimeOverrides {
                request_timeout_seconds: Some(REQUEST_TIMEOUT_SECONDS),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("创建 HTTP 客户端失败：{}", error.message()))?;
        Ok(Self {
            http,
            api_base: credentials.api_base.trim_end_matches('/').to_string(),
            auth_headers,
            zones: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Finds the zone that owns `name`, trying the longest candidate first.
    pub async fn zone_id(&self, name: &str) -> Result<String, String> {
        for candidate in zone_candidates(name) {
            let cached = self.zones.lock().await.get(&candidate).cloned();
            if let Some(zone_id) = cached {
                return Ok(zone_id);
            }
            let zones: Vec<Zone> = self
                .request(
                    self.http
                        .get(format!("{}/zones", self.api_base))
                        .query(&[("name", candidate.as_str())]),
                )
                .await?;
            if let Some(zone) = zones.into_iter().next() {
                self.zones.lock().await.insert(candidate, zone.id.clone());
                return Ok(zone.id);
            }
        }
        Err(format!("Cloudflare 账号下找不到 {name} 所属的域名（Zone）"))
    }

    pub async fn list_records(
        &self,
        zone_id: &str,
        record_type: &str,
        name: &str,
    ) -> Result<Vec<DnsRecord>, String> {
        self.request(
            self.http
                .get(self.records_url(zone_id))
                .query(&[("type", record_type), ("name", name)]),
        )
        .await
    }

    /// Creates or updates the single record of `record.record_type` for its name.
    /// Returns `created`, `updated` or `unchanged`.
    pub async fn upsert_record(&self, record: &DesiredRecord) -> Result<&'static str, String> {
        let zone_id = self.zone_id(&record.name).await?;
        let existing = self
            .list_records(&zone_id, record.record_type, &record.name)
            .await?;
        let body = serde_json::json!({
            "type": record.record_type,
            "name": record.name,
            "content": record.content,
            "ttl": 1,
            "proxied": record.proxied,
        });
        let Some(current) = existing.into_iter().next() else {
            let _: serde_json::Value = self
                .request(self.http.post(self.records_url(&zone_id)).json(&body))
                .await?;
            return Ok("created");
        };
        if current.content.eq_ignore_ascii_case(&record.content)
            && current.proxied == record.proxied
        {
            return Ok("unchanged");
        }
        let _: serde_json::Value = self
            .request(
                self.http
                    .patch(format!("{}/{}", self.records_url(&zone_id), current.id))
                    .json(&body),
            )
            .await?;
        Ok("updated")
    }

    /// Removes every record of `record_type` for `name`; returns how many were deleted.
    pub async fn delete_records(&self, record_type: &str, name: &str) -> Result<usize, String> {
        let zone_id = self.zone_id(name).await?;
        let records = self.list_records(&zone_id, record_type, name).await?;
        for record in &records {
            self.delete_record(&zone_id, &record.id).await?;
        }
        Ok(records.len())
    }

    /// Adds a TXT record (several may share a name during ACME challenges).
    pub async fn create_txt(&self, name: &str, content: &str) -> Result<(String, String), String> {
        let zone_id = self.zone_id(name).await?;
        let record: DnsRecord = self
            .request(
                self.http
                    .post(self.records_url(&zone_id))
                    .json(&serde_json::json!({
                        "type": "TXT",
                        "name": name,
                        "content": content,
                        "ttl": 60,
                    })),
            )
            .await?;
        Ok((zone_id, record.id))
    }

    pub async fn delete_record(&self, zone_id: &str, record_id: &str) -> Result<(), String> {
        let _: serde_json::Value = self
            .request(
                self.http
                    .delete(format!("{}/{record_id}", self.records_url(zone_id))),
            )
            .await?;
        Ok(())
    }

    pub async fn find_tunnel(
        &self,
        account_id: &str,
        name: &str,
    ) -> Result<Option<Tunnel>, String> {
        let tunnels: Vec<Tunnel> = self
            .request(
                self.http
                    .get(self.tunnels_url(account_id))
                    .query(&[("name", name), ("is_deleted", "false")]),
            )
            .await?;
        Ok(tunnels.into_iter().find(|tunnel| tunnel.name == name))
    }

    pub async fn tunnel_exists(&self, account_id: &str, tunnel_id: &str) -> Result<bool, String> {
        #[derive(Deserialize)]
        struct TunnelState {
            #[serde(default)]
            deleted_at: Option<String>,
        }
        match self
            .request::<TunnelState>(
                self.http
                    .get(format!("{}/{tunnel_id}", self.tunnels_url(account_id))),
            )
            .await
        {
            Ok(state) => Ok(state.deleted_at.is_none()),
            Err(error) if error.contains("HTTP 404") => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Creates a remotely-managed tunnel (ingress rules live in Cloudflare).
    pub async fn create_tunnel(&self, account_id: &str, name: &str) -> Result<Tunnel, String> {
        self.request(
            self.http
                .post(self.tunnels_url(account_id))
                .json(&serde_json::json!({ "name": name, "config_src": "cloudflare" })),
        )
        .await
    }

    pub async fn tunnel_token(&self, account_id: &str, tunnel_id: &str) -> Result<String, String> {
        self.request(self.http.get(format!(
            "{}/{tunnel_id}/token",
            self.tunnels_url(account_id)
        )))
        .await
    }

    pub async fn put_tunnel_config(
        &self,
        account_id: &str,
        tunnel_id: &str,
        config: &serde_json::Value,
    ) -> Result<(), String> {
        let _: serde_json::Value = self
            .request(
                self.http
                    .put(format!(
                        "{}/{tunnel_id}/configurations",
                        self.tunnels_url(account_id)
                    ))
                    .json(&serde_json::json!({ "config": config })),
            )
            .await?;
        Ok(())
    }

    fn records_url(&self, zone_id: &str) -> String {
        format!("{}/zones/{zone_id}/dns_records", self.api_base)
    }

    fn tunnels_url(&self, account_id: &str) -> String {
        format!("{}/accounts/{account_id}/cfd_tunnel", self.api_base)
    }

    async fn request<T: DeserializeOwned>(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<T, String> {
        let response = builder
            .headers(self.auth_headers.clone())
            .send()
            .await
            .map_err(|error| format!("请求 Cloudflare 失败：{error}"))?;
        let status = response.status();
        let envelope: Envelope<T> = response.json().await.map_err(|error| {
            format!("Cloudflare 返回了无法解析的响应（HTTP {status}）：{error}")
        })?;
        if status.is_success() && envelope.success {
            return envelope
                .result
                .ok_or_else(|| "Cloudflare 响应缺少 result".to_string());
        }
        let mut detail = envelope
            .errors
            .iter()
            .map(|message| {
                message.code.map_or_else(
                    || message.message.clone(),
                    |code| format!("[{code}] {}", message.message),
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        // Never let an echoed credential end up in stored status messages.
        for header in self.auth_headers.values() {
            if let Ok(value) = header.to_str() {
                detail = detail.replace(value, "[REDACTED]");
                if let Some(token) = value.strip_prefix("Bearer ") {
                    detail = detail.replace(token, "[REDACTED]");
                }
            }
        }
        Err(format!("Cloudflare 返回错误（HTTP {status}）：{detail}"))
    }
}
