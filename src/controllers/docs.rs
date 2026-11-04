#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    response::Html,
};
use loco_rs::{auth::jwt::JWT, prelude::*};
use serde::{Deserialize, Serialize};
use utoipa::OpenApi;

use crate::{
    controllers::admin::authorize,
    errors::{ApiError, ApiResult},
    middleware::api_key_auth::bearer_token,
    models::{rbac, user_sessions, users},
    openapi::ApiDoc,
    responses::{self, ApiResponse},
    services::security_settings::{self, DocsAccess},
};

static OPENAPI_JSON: OnceLock<String> = OnceLock::new();
static TICKETS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
const TICKET_TTL: Duration = Duration::from_secs(10 * 60);
const DOCS_PERMISSION: &str = "system:docs:view";

#[derive(Debug, Deserialize)]
pub struct DocsQuery {
    ticket: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct DocsTicketRecord {
    /// `public`, `login` or `disabled`.
    pub mode: String,
    /// Append as `?ticket=` to `/swagger-ui`; absent unless `login`.
    pub ticket: Option<String>,
}

fn tickets() -> &'static Mutex<HashMap<String, Instant>> {
    TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ticket_valid(ticket: Option<&str>) -> bool {
    let Some(ticket) = ticket else {
        return false;
    };
    tickets()
        .lock()
        .ok()
        .and_then(|store| store.get(ticket).copied())
        .is_some_and(|issued| issued.elapsed() < TICKET_TTL)
}

/// A signed-in bearer token (with a live session) that may view the docs.
async fn bearer_allowed(ctx: &AppContext, headers: &HeaderMap) -> bool {
    let Some(token) = bearer_token(headers) else {
        return false;
    };
    let Ok(config) = ctx.config.get_jwt_config() else {
        return false;
    };
    let Ok(token) = JWT::new(&config.secret).validate(token) else {
        return false;
    };
    let claims = token.claims;
    let Some(sid) = claims
        .claims
        .get(user_sessions::SID_CLAIM)
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let Ok(Some(session)) = user_sessions::validate(&ctx.db, sid, &claims.pid).await else {
        return false;
    };
    let Ok(Some(user)) = users::Entity::find_by_id(session.user_id)
        .one(&ctx.db)
        .await
    else {
        return false;
    };
    rbac::user_has_permission(&ctx.db, user.id, DOCS_PERMISSION)
        .await
        .unwrap_or(false)
}

async fn access_denied(
    ctx: &AppContext,
    headers: &HeaderMap,
    query: &DocsQuery,
) -> Option<Response> {
    let mode = security_settings::load(&ctx.db)
        .await
        .map_or(DocsAccess::Login, |settings| settings.docs_access);
    match mode {
        DocsAccess::Public => None,
        DocsAccess::Disabled => {
            Some((StatusCode::NOT_FOUND, "API documentation is disabled").into_response())
        }
        DocsAccess::Login => {
            if ticket_valid(query.ticket.as_deref()) || bearer_allowed(ctx, headers).await {
                None
            } else {
                Some(
                    (
                        StatusCode::UNAUTHORIZED,
                        "Sign in to the admin console to view the API documentation",
                    )
                        .into_response(),
                )
            }
        }
    }
}

/// Lets the signed-in admin page embed Swagger UI in an iframe, which cannot
/// send the bearer token itself.
#[utoipa::path(
    post,
    path = "/api/admin/docs/ticket",
    tag = "admin-docs",
    security(("bearer_auth" = [])),
    responses((status = 200, body = ApiResponse<DocsTicketRecord>))
)]
#[debug_handler]
pub async fn create_ticket(auth: auth::JWT, State(ctx): State<AppContext>) -> ApiResult<Response> {
    authorize(&ctx, &auth, DOCS_PERMISSION).await?;
    let settings = security_settings::load(&ctx.db).await?;
    let (mode, ticket) = match settings.docs_access {
        DocsAccess::Public => ("public", None),
        DocsAccess::Disabled => ("disabled", None),
        DocsAccess::Login => {
            let ticket = uuid::Uuid::new_v4().simple().to_string();
            {
                let mut store = tickets()
                    .lock()
                    .map_err(|_| ApiError::internal("failed to lock docs tickets"))?;
                store.retain(|_, issued| issued.elapsed() < TICKET_TTL);
                store.insert(ticket.clone(), Instant::now());
            }
            ("login", Some(ticket))
        }
    };
    Ok(responses::ok(DocsTicketRecord {
        mode: mode.to_string(),
        ticket,
    }))
}

fn openapi_json_string() -> Result<&'static str> {
    if let Some(spec) = OPENAPI_JSON.get() {
        return Ok(spec);
    }

    let spec = std::thread::Builder::new()
        .stack_size(128 * 1024 * 1024)
        .spawn(|| serde_json::to_string(&ApiDoc::openapi()))
        .map_err(loco_rs::Error::from)?
        .join()
        .map_err(|_| loco_rs::Error::string("failed to build OpenAPI document"))??;
    Ok(OPENAPI_JSON.get_or_init(|| spec))
}

async fn openapi_json(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Query(query): Query<DocsQuery>,
) -> Result<Response> {
    if let Some(denied) = access_denied(&ctx, &headers, &query).await {
        return Ok(denied);
    }
    Ok((
        [("content-type", "application/json; charset=utf-8")],
        openapi_json_string()?.to_string(),
    )
        .into_response())
}

async fn swagger_ui(
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Query(query): Query<DocsQuery>,
) -> Result<Response> {
    if let Some(denied) = access_denied(&ctx, &headers, &query).await {
        return Ok(denied);
    }
    let spec = openapi_json_string()?;
    Ok(Html(format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>后台管理 API Docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.ui = SwaggerUIBundle({{ spec: {spec}, dom_id: '#swagger-ui' }});
  </script>
</body>
</html>"#
    ))
    .into_response())
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/api-docs/openapi.json", get(openapi_json))
        .add("/swagger-ui", get(swagger_ui))
        .add("/api/admin/docs/ticket", post(create_ticket))
}
