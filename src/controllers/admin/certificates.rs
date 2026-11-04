#![allow(clippy::missing_errors_doc)]

use chrono::{DateTime, FixedOffset, Local};
use loco_rs::{environment::Environment, prelude::*};
use sea_orm::{PaginatorTrait, QueryOrder, Set};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    models::{
        _entities::{domain_mappings, tls_certificates},
        admin_logs,
    },
    responses::{self, ApiResponse, EmptyData},
    services::certificates::{
        self as certs,
        cloudflare::CloudflareApi,
        proxy::ListenerStatus,
        tunnel::{self, TunnelStatus},
        CertificateSettingsView, SaveCertificateSettingsParams, MODE_DIRECT, MODE_TUNNEL,
    },
};

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CertificateRecord {
    pub id: i32,
    pub domain: String,
    pub alt_names: Vec<String>,
    /// `pending`, `issuing`, `active` or `failed`.
    pub status: String,
    pub auto_renew: bool,
    pub issuing: bool,
    pub issuer: Option<String>,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    pub days_remaining: Option<i64>,
    pub last_error: Option<String>,
    pub last_attempt_at: Option<String>,
    pub renewed_at: Option<String>,
    pub mapping_count: usize,
    pub created_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DomainMappingRecord {
    pub id: i32,
    pub name: String,
    pub domain: String,
    /// `direct` (built-in HTTPS) or `tunnel` (Cloudflare Tunnel).
    pub mode: String,
    pub upstream_host: String,
    pub upstream_port: i32,
    pub public_port: i32,
    pub public_url: String,
    pub ipv4_mode: String,
    pub ipv4_address: Option<String>,
    pub ipv6_mode: String,
    pub ipv6_address: Option<String>,
    pub enabled: bool,
    /// `active`, `pending`, `error` or `disabled`.
    pub status: String,
    pub last_error: Option<String>,
    pub synced_at: Option<String>,
    pub certificate_id: Option<i32>,
    pub certificate_status: Option<String>,
    pub certificate_not_after: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CertificateRuntimeRecord {
    pub listeners: Vec<ListenerStatus>,
    pub tunnel: TunnelStatus,
    pub tunnel_id: String,
    pub tunnel_name: String,
    /// Port of this admin server; mappings cannot reuse it.
    pub server_port: i32,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateCertificateParams {
    pub domain: String,
    /// Extra names on the same certificate, e.g. `*.example.com`.
    pub alt_names: Option<Vec<String>>,
    pub auto_renew: Option<bool>,
    /// Start issuing right away (default true).
    pub issue_now: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateCertificateParams {
    pub auto_renew: Option<bool>,
    pub alt_names: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct SaveDomainMappingParams {
    pub name: Option<String>,
    pub domain: String,
    pub mode: String,
    pub upstream_host: Option<String>,
    pub upstream_port: i32,
    /// HTTPS port for direct mode (default 443); tunnels always use 443.
    pub public_port: Option<i32>,
    pub ipv4_mode: Option<String>,
    pub ipv4_address: Option<String>,
    pub ipv6_mode: Option<String>,
    pub ipv6_address: Option<String>,
    pub enabled: Option<bool>,
    /// Direct mode: create and issue a certificate when none covers the domain (default true).
    pub issue_certificate: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SaveDomainMappingRecord {
    pub mapping: DomainMappingRecord,
    /// Why the certificate was not issued automatically, if it was not.
    pub notice: Option<String>,
}

fn time_text(value: DateTime<FixedOffset>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn certificate_record(
    certificate: &tls_certificates::Model,
    mappings: &[domain_mappings::Model],
) -> CertificateRecord {
    CertificateRecord {
        id: certificate.id,
        domain: certificate.domain.clone(),
        alt_names: serde_json::from_str(&certificate.alt_names).unwrap_or_default(),
        status: certificate.status.clone(),
        auto_renew: certificate.auto_renew,
        issuing: certs::is_issuing(certificate.id),
        issuer: certificate.issuer.clone(),
        not_before: certificate.not_before.map(time_text),
        not_after: certificate.not_after.map(time_text),
        days_remaining: certificate
            .not_after
            .map(|not_after| (not_after - Local::now().fixed_offset()).num_days()),
        last_error: certificate.last_error.clone(),
        last_attempt_at: certificate.last_attempt_at.map(time_text),
        renewed_at: certificate.renewed_at.map(time_text),
        mapping_count: mappings
            .iter()
            .filter(|mapping| mapping.certificate_id == Some(certificate.id))
            .count(),
        created_at: time_text(certificate.created_at),
    }
}

fn mapping_record(
    mapping: &domain_mappings::Model,
    certificates: &[tls_certificates::Model],
) -> DomainMappingRecord {
    let certificate = mapping
        .certificate_id
        .and_then(|id| certificates.iter().find(|certificate| certificate.id == id));
    DomainMappingRecord {
        id: mapping.id,
        name: mapping.name.clone(),
        domain: mapping.domain.clone(),
        mode: mapping.mode.clone(),
        upstream_host: mapping.upstream_host.clone(),
        upstream_port: mapping.upstream_port,
        public_port: mapping.public_port,
        public_url: certs::public_url(&mapping.domain, &mapping.mode, mapping.public_port),
        ipv4_mode: mapping.ipv4_mode.clone(),
        ipv4_address: mapping.ipv4_address.clone(),
        ipv6_mode: mapping.ipv6_mode.clone(),
        ipv6_address: mapping.ipv6_address.clone(),
        enabled: mapping.enabled,
        status: mapping.status.clone(),
        last_error: mapping.last_error.clone(),
        synced_at: mapping.synced_at.map(time_text),
        certificate_id: mapping.certificate_id,
        certificate_status: certificate.map(|certificate| {
            if certs::is_issuing(certificate.id) {
                "issuing".to_string()
            } else {
                certificate.status.clone()
            }
        }),
        certificate_not_after: certificate
            .and_then(|certificate| certificate.not_after.map(time_text)),
        created_at: time_text(mapping.created_at),
        updated_at: time_text(mapping.updated_at),
    }
}

async fn record_log(
    ctx: &AppContext,
    user: &crate::models::users::Model,
    action: &'static str,
    message: String,
) {
    admin_logs::record(
        &ctx.db,
        admin_logs::LogInput {
            log_type: "operation",
            level: "info",
            module: "certificates",
            action,
            message: &message,
            user_id: Some(user.id),
            operator: Some(user.email.clone()),
            method: None,
            path: Some("/api/admin/certificates"),
            status: Some(200),
            error_message: None,
        },
    )
    .await;
}

/// Applies changes in the background; tests run without listeners or tunnels.
fn spawn_sync(ctx: &AppContext) {
    if matches!(ctx.environment, Environment::Test) {
        return;
    }
    let db = ctx.db.clone();
    tokio::spawn(async move {
        if let Err(error) = certs::sync_all(&db, true).await {
            tracing::warn!(error = ?error, "certificate/domain mapping sync failed");
        }
    });
}

fn normalize_alt_names(domain: &str, names: Option<Vec<String>>) -> ApiResult<Vec<String>> {
    let mut normalized: Vec<String> = Vec::new();
    for name in names.unwrap_or_default() {
        if name.trim().is_empty() {
            continue;
        }
        let name = certs::normalize_domain(&name, true).map_err(ApiError::bad_request)?;
        if name != domain && !normalized.contains(&name) {
            normalized.push(name);
        }
    }
    if normalized.len() > 20 {
        return Err(ApiError::bad_request("一张证书最多包含 20 个附加域名"));
    }
    Ok(normalized)
}

async fn load_all(
    ctx: &AppContext,
) -> ApiResult<(Vec<tls_certificates::Model>, Vec<domain_mappings::Model>)> {
    let certificates = tls_certificates::Entity::find()
        .order_by_asc(tls_certificates::Column::Domain)
        .all(&ctx.db)
        .await?;
    let mappings = domain_mappings::Entity::find()
        .order_by_asc(domain_mappings::Column::Domain)
        .all(&ctx.db)
        .await?;
    Ok((certificates, mappings))
}

// ---------------------------------------------------------------------------
// Settings and runtime
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/certificates/settings",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<CertificateSettingsView>))
)]
#[debug_handler]
pub async fn get_settings(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:certificate:list").await?;
    let settings = certs::load_settings(&ctx.db).await?;
    Ok(responses::ok(certs::settings_view(&settings)))
}

#[utoipa::path(
    put,
    path = "/api/admin/certificates/settings",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    request_body = SaveCertificateSettingsParams,
    responses((status = 200, body = ApiResponse<CertificateSettingsView>))
)]
#[debug_handler]
pub async fn update_settings(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveCertificateSettingsParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:update").await?;
    let settings = certs::save_settings(&ctx.db, params, user.id).await?;
    record_log(
        &ctx,
        &user,
        "update_settings",
        "更新证书与域名映射设置".to_string(),
    )
    .await;
    spawn_sync(&ctx);
    Ok(responses::ok(certs::settings_view(&settings)))
}

#[utoipa::path(
    get,
    path = "/api/admin/domain-mappings/runtime",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<CertificateRuntimeRecord>))
)]
#[debug_handler]
pub async fn runtime(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:certificate:list").await?;
    let settings = certs::load_settings(&ctx.db).await?;
    Ok(responses::ok(CertificateRuntimeRecord {
        listeners: certs::proxy::status().await,
        tunnel: tunnel::status(&settings.cloudflared_path).await,
        tunnel_id: settings.tunnel_id,
        tunnel_name: if settings.tunnel_name.is_empty() {
            certs::default_tunnel_name()
        } else {
            settings.tunnel_name
        },
        server_port: ctx.config.server.port,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/domain-mappings/tunnel/restart",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn restart_tunnel(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:issue").await?;
    if !tunnel::restart().await {
        return Err(ApiError::bad_request("cloudflared 当前未运行"));
    }
    record_log(
        &ctx,
        &user,
        "restart_tunnel",
        "重启 cloudflared".to_string(),
    )
    .await;
    Ok(responses::empty())
}

// ---------------------------------------------------------------------------
// Certificates
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/certificates",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<CertificateRecord>>))
)]
#[debug_handler]
pub async fn list_certificates(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:certificate:list").await?;
    let (certificates, mappings) = load_all(&ctx).await?;
    Ok(responses::ok(
        certificates
            .iter()
            .map(|certificate| certificate_record(certificate, &mappings))
            .collect::<Vec<_>>(),
    ))
}

#[utoipa::path(
    post,
    path = "/api/admin/certificates",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    request_body = CreateCertificateParams,
    responses((status = 200, body = ApiResponse<CertificateRecord>))
)]
#[debug_handler]
pub async fn create_certificate(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<CreateCertificateParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:create").await?;
    let domain = certs::normalize_domain(&params.domain, true).map_err(ApiError::bad_request)?;
    let alt_names = normalize_alt_names(&domain, params.alt_names)?;
    if tls_certificates::Entity::find()
        .filter(tls_certificates::Column::Domain.eq(&domain))
        .one(&ctx.db)
        .await?
        .is_some()
    {
        return Err(ApiError::bad_request(format!("{domain} 的证书已存在")));
    }
    let certificate = tls_certificates::ActiveModel {
        domain: Set(domain.clone()),
        alt_names: Set(serde_json::to_string(&alt_names).unwrap_or_else(|_| "[]".to_string())),
        status: Set("pending".to_string()),
        auto_renew: Set(params.auto_renew.unwrap_or(true)),
        created_by: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    record_log(
        &ctx,
        &user,
        "create_certificate",
        format!("新增证书：{domain}"),
    )
    .await;
    if params.issue_now.unwrap_or(true) {
        certs::start_issue(&ctx.db, certificate.id).await?;
    }
    let certificate = tls_certificates::Entity::find_by_id(certificate.id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("证书不存在"))?;
    Ok(responses::ok(certificate_record(&certificate, &[])))
}

#[utoipa::path(
    put,
    path = "/api/admin/certificates/{id}",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = UpdateCertificateParams,
    responses((status = 200, body = ApiResponse<CertificateRecord>))
)]
#[debug_handler]
pub async fn update_certificate(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<UpdateCertificateParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:update").await?;
    let certificate = tls_certificates::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("证书不存在"))?;
    let domain = certificate.domain.clone();
    let mut active = certificate.into_active_model();
    if let Some(auto_renew) = params.auto_renew {
        active.auto_renew = Set(auto_renew);
    }
    if let Some(names) = params.alt_names {
        let alt_names = normalize_alt_names(&domain, Some(names))?;
        active.alt_names =
            Set(serde_json::to_string(&alt_names).unwrap_or_else(|_| "[]".to_string()));
    }
    let certificate = active.update(&ctx.db).await?;
    record_log(
        &ctx,
        &user,
        "update_certificate",
        format!("更新证书：{domain}"),
    )
    .await;
    let (_, mappings) = load_all(&ctx).await?;
    Ok(responses::ok(certificate_record(&certificate, &mappings)))
}

#[utoipa::path(
    delete,
    path = "/api/admin/certificates/{id}",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_certificate(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:delete").await?;
    let certificate = tls_certificates::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("证书不存在"))?;
    let in_use = domain_mappings::Entity::find()
        .filter(domain_mappings::Column::CertificateId.eq(id))
        .filter(domain_mappings::Column::Enabled.eq(true))
        .count(&ctx.db)
        .await?;
    if in_use > 0 {
        return Err(ApiError::bad_request(format!(
            "仍有 {in_use} 个启用的域名映射在使用该证书，请先停用或删除映射"
        )));
    }
    if certs::is_issuing(id) {
        return Err(ApiError::bad_request("证书正在签发中，请稍后再删除"));
    }
    let domain = certificate.domain.clone();
    // The key folder goes into the recycle bin with the record.
    let key_folders: Vec<std::path::PathBuf> = certificate
        .cert_path
        .as_deref()
        .and_then(|cert_path| std::path::Path::new(cert_path).parent())
        .filter(|folder| folder.starts_with(certs::storage_dir()))
        .map(std::path::Path::to_path_buf)
        .into_iter()
        .collect();
    crate::services::recycle_bin::delete_row(
        &ctx.db,
        crate::services::recycle_bin::Deletion::new("certificates", id, &domain, &user),
        "tls_certificates",
        i64::from(id),
        &key_folders,
    )
    .await?;
    record_log(
        &ctx,
        &user,
        "delete_certificate",
        format!("删除证书：{domain}"),
    )
    .await;
    spawn_sync(&ctx);
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/certificates/{id}/issue",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<CertificateRecord>))
)]
#[debug_handler]
pub async fn issue_certificate(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:issue").await?;
    certs::start_issue(&ctx.db, id).await?;
    let certificate = tls_certificates::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("证书不存在"))?;
    record_log(
        &ctx,
        &user,
        "issue_certificate",
        format!("签发/续签证书：{}", certificate.domain),
    )
    .await;
    let (_, mappings) = load_all(&ctx).await?;
    Ok(responses::ok(certificate_record(&certificate, &mappings)))
}

// ---------------------------------------------------------------------------
// Domain mappings
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/domain-mappings",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<Vec<DomainMappingRecord>>))
)]
#[debug_handler]
pub async fn list_mappings(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:certificate:list").await?;
    let (certificates, mappings) = load_all(&ctx).await?;
    Ok(responses::ok(
        mappings
            .iter()
            .map(|mapping| mapping_record(mapping, &certificates))
            .collect::<Vec<_>>(),
    ))
}

struct ValidMapping {
    name: String,
    domain: String,
    mode: &'static str,
    upstream_host: String,
    upstream_port: i32,
    public_port: i32,
    ipv4: (String, Option<String>),
    ipv6: (String, Option<String>),
    enabled: bool,
}

fn validate_mapping(ctx: &AppContext, params: &SaveDomainMappingParams) -> ApiResult<ValidMapping> {
    let domain = certs::normalize_domain(&params.domain, false).map_err(ApiError::bad_request)?;
    let mode = match params.mode.trim() {
        MODE_DIRECT => MODE_DIRECT,
        MODE_TUNNEL => MODE_TUNNEL,
        _ => {
            return Err(ApiError::bad_request(
                "映射方式只能是 direct（内置 HTTPS）或 tunnel（Cloudflare Tunnel）",
            ))
        }
    };
    let upstream_host =
        certs::validate_upstream_host(params.upstream_host.as_deref().unwrap_or("127.0.0.1"))
            .map_err(ApiError::bad_request)?;
    let upstream_port = certs::validate_port(params.upstream_port, "本地服务端口")
        .map_err(ApiError::bad_request)?;
    let (public_port, ipv4, ipv6) = if mode == MODE_TUNNEL {
        // Cloudflare's edge serves tunnels on 443; DNS is a proxied CNAME.
        (443, ("off".to_string(), None), ("off".to_string(), None))
    } else {
        let port = certs::validate_port(params.public_port.unwrap_or(443), "HTTPS 端口")
            .map_err(ApiError::bad_request)?;
        if i32::from(port) == ctx.config.server.port {
            return Err(ApiError::bad_request(format!(
                "HTTPS 端口 {port} 与后台服务端口冲突，请换一个端口"
            )));
        }
        let ipv4 = certs::normalize_ip_mode(
            params.ipv4_mode.as_deref().unwrap_or("off"),
            params.ipv4_address.as_deref(),
            false,
        )
        .map_err(ApiError::bad_request)?;
        let ipv6 = certs::normalize_ip_mode(
            params.ipv6_mode.as_deref().unwrap_or("off"),
            params.ipv6_address.as_deref(),
            true,
        )
        .map_err(ApiError::bad_request)?;
        (i32::from(port), ipv4, ipv6)
    };
    let name = params
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| domain.clone(), str::to_string);
    if name.chars().count() > 100 {
        return Err(ApiError::bad_request("映射名称不能超过 100 个字符"));
    }
    Ok(ValidMapping {
        name,
        domain,
        mode,
        upstream_host,
        upstream_port: i32::from(upstream_port),
        public_port,
        ipv4,
        ipv6,
        enabled: params.enabled.unwrap_or(true),
    })
}

/// Links a direct mapping to a certificate covering it, creating one if needed.
async fn ensure_certificate(
    ctx: &AppContext,
    user_id: i32,
    domain: &str,
    issue: bool,
) -> ApiResult<(i32, Option<String>)> {
    let certificates = tls_certificates::Entity::find().all(&ctx.db).await?;
    if let Some(certificate) = certificates.iter().find(|certificate| {
        certs::certificate_covers(&certs::certificate_names(certificate), domain)
    }) {
        return Ok((certificate.id, None));
    }
    let certificate = tls_certificates::ActiveModel {
        domain: Set(domain.to_string()),
        alt_names: Set("[]".to_string()),
        status: Set("pending".to_string()),
        auto_renew: Set(true),
        created_by: Set(Some(user_id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    let notice = if issue {
        certs::start_issue(&ctx.db, certificate.id)
            .await
            .err()
            .map(|error| format!("证书未自动申请：{}", error.message()))
    } else {
        Some("已创建证书记录，请在「证书」中手动申请".to_string())
    };
    Ok((certificate.id, notice))
}

async fn mapping_response(
    ctx: &AppContext,
    id: i32,
    notice: Option<String>,
) -> ApiResult<Response> {
    let (certificates, mappings) = load_all(ctx).await?;
    let mapping = mappings
        .iter()
        .find(|mapping| mapping.id == id)
        .ok_or_else(|| ApiError::not_found("域名映射不存在"))?;
    Ok(responses::ok(SaveDomainMappingRecord {
        mapping: mapping_record(mapping, &certificates),
        notice,
    }))
}

#[utoipa::path(
    post,
    path = "/api/admin/domain-mappings",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    request_body = SaveDomainMappingParams,
    responses((status = 200, body = ApiResponse<SaveDomainMappingRecord>))
)]
#[debug_handler]
pub async fn create_mapping(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Json(params): Json<SaveDomainMappingParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:create").await?;
    let valid = validate_mapping(&ctx, &params)?;
    if domain_mappings::Entity::find()
        .filter(domain_mappings::Column::Domain.eq(&valid.domain))
        .one(&ctx.db)
        .await?
        .is_some()
    {
        return Err(ApiError::bad_request(format!(
            "{} 已经配置了映射",
            valid.domain
        )));
    }
    let (certificate_id, notice) = if valid.mode == MODE_DIRECT {
        let (id, notice) = ensure_certificate(
            &ctx,
            user.id,
            &valid.domain,
            params.issue_certificate.unwrap_or(true),
        )
        .await?;
        (Some(id), notice)
    } else {
        (None, None)
    };
    let mapping = domain_mappings::ActiveModel {
        name: Set(valid.name),
        domain: Set(valid.domain.clone()),
        mode: Set(valid.mode.to_string()),
        upstream_host: Set(valid.upstream_host),
        upstream_port: Set(valid.upstream_port),
        public_port: Set(valid.public_port),
        ipv4_mode: Set(valid.ipv4.0),
        ipv4_address: Set(valid.ipv4.1),
        ipv6_mode: Set(valid.ipv6.0),
        ipv6_address: Set(valid.ipv6.1),
        enabled: Set(valid.enabled),
        status: Set(if valid.enabled { "pending" } else { "disabled" }.to_string()),
        certificate_id: Set(certificate_id),
        created_by: Set(Some(user.id)),
        ..Default::default()
    }
    .insert(&ctx.db)
    .await?;
    record_log(
        &ctx,
        &user,
        "create_mapping",
        format!(
            "新增域名映射：{} → {}:{}",
            certs::public_url(&mapping.domain, &mapping.mode, mapping.public_port),
            mapping.upstream_host,
            mapping.upstream_port
        ),
    )
    .await;
    spawn_sync(&ctx);
    mapping_response(&ctx, mapping.id, notice).await
}

#[utoipa::path(
    put,
    path = "/api/admin/domain-mappings/{id}",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    request_body = SaveDomainMappingParams,
    responses((status = 200, body = ApiResponse<SaveDomainMappingRecord>))
)]
#[debug_handler]
pub async fn update_mapping(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Json(params): Json<SaveDomainMappingParams>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:update").await?;
    let existing = domain_mappings::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("域名映射不存在"))?;
    let valid = validate_mapping(&ctx, &params)?;
    if domain_mappings::Entity::find()
        .filter(domain_mappings::Column::Domain.eq(&valid.domain))
        .filter(domain_mappings::Column::Id.ne(id))
        .one(&ctx.db)
        .await?
        .is_some()
    {
        return Err(ApiError::bad_request(format!(
            "{} 已经配置了映射",
            valid.domain
        )));
    }
    let (certificate_id, notice) = if valid.mode == MODE_DIRECT {
        let (id, notice) = ensure_certificate(
            &ctx,
            user.id,
            &valid.domain,
            params.issue_certificate.unwrap_or(true),
        )
        .await?;
        (Some(id), notice)
    } else {
        (None, None)
    };
    let mut active = existing.into_active_model();
    active.name = Set(valid.name);
    active.domain = Set(valid.domain.clone());
    active.mode = Set(valid.mode.to_string());
    active.upstream_host = Set(valid.upstream_host);
    active.upstream_port = Set(valid.upstream_port);
    active.public_port = Set(valid.public_port);
    active.ipv4_mode = Set(valid.ipv4.0);
    active.ipv4_address = Set(valid.ipv4.1);
    active.ipv6_mode = Set(valid.ipv6.0);
    active.ipv6_address = Set(valid.ipv6.1);
    active.enabled = Set(valid.enabled);
    if !valid.enabled {
        active.status = Set("disabled".to_string());
    }
    active.certificate_id = Set(certificate_id);
    let mapping = active.update(&ctx.db).await?;
    record_log(
        &ctx,
        &user,
        "update_mapping",
        format!("更新域名映射：{}", mapping.domain),
    )
    .await;
    spawn_sync(&ctx);
    mapping_response(&ctx, mapping.id, notice).await
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeleteMappingQuery {
    /// Also remove the DNS records this mapping managed (default true).
    pub remove_dns: Option<bool>,
}

#[utoipa::path(
    delete,
    path = "/api/admin/domain-mappings/{id}",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path), DeleteMappingQuery),
    responses((status = 200, body = ApiResponse<EmptyData>))
)]
#[debug_handler]
pub async fn delete_mapping(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
    Query(query): Query<DeleteMappingQuery>,
) -> ApiResult<Response> {
    let user = authorize(&ctx, &auth, "system:certificate:delete").await?;
    let mapping = domain_mappings::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("域名映射不存在"))?;
    let domain = mapping.domain.clone();
    let mut record_types = Vec::new();
    if mapping.mode == MODE_TUNNEL {
        record_types.push("CNAME");
    } else {
        if mapping.ipv4_mode != "off" {
            record_types.push("A");
        }
        if mapping.ipv6_mode != "off" {
            record_types.push("AAAA");
        }
    }
    crate::services::recycle_bin::delete_row(
        &ctx.db,
        crate::services::recycle_bin::Deletion::new("domain_mappings", id, &domain, &user),
        "domain_mappings",
        i64::from(id),
        &[],
    )
    .await?;
    if query.remove_dns.unwrap_or(true) && !record_types.is_empty() {
        let settings = certs::load_settings(&ctx.db).await?;
        let credentials = certs::cloudflare_credentials(&ctx.db, &settings).await?;
        if let Ok(cloudflare) = CloudflareApi::new(&ctx.db, &credentials).await {
            for record_type in record_types {
                if let Err(error) = cloudflare.delete_records(record_type, &domain).await {
                    tracing::warn!(domain, record_type, error, "failed to remove DNS record");
                }
            }
        }
    }
    record_log(
        &ctx,
        &user,
        "delete_mapping",
        format!("删除域名映射：{domain}"),
    )
    .await;
    spawn_sync(&ctx);
    Ok(responses::empty())
}

#[utoipa::path(
    post,
    path = "/api/admin/domain-mappings/{id}/sync",
    tag = "admin-certificates",
    security(("bearer_auth" = [])),
    params(("id" = i32, Path)),
    responses((status = 200, body = ApiResponse<SaveDomainMappingRecord>))
)]
#[debug_handler]
pub async fn sync_mapping(
    auth: auth::JWT,
    State(ctx): State<AppContext>,
    Path(id): Path<i32>,
) -> ApiResult<Response> {
    authorize(&ctx, &auth, "system:certificate:issue").await?;
    domain_mappings::Entity::find_by_id(id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| ApiError::not_found("域名映射不存在"))?;
    certs::sync_all(&ctx.db, true).await?;
    mapping_response(&ctx, id, None).await
}
