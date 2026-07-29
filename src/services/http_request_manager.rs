use std::{collections::BTreeMap, fs, path::PathBuf, time::Instant};

use chrono::{Datelike, Local};
use loco_rs::app::AppContext;
use reqwest::{header::HeaderMap, Method, Url};
use sea_orm::{ActiveModelTrait, Set};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::{ApiError, ApiResult},
    models::_entities::http_request_runs,
    services::http_client,
};

const RESULT_ROOT: &str = "storage/http-client/results";
const PREVIEW_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HttpRequestDraft {
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub headers_json: Option<String>,
    pub query_json: Option<String>,
    pub body_type: String,
    pub body_text: Option<String>,
    pub content_type: Option<String>,
    pub source_format: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct HttpRequestExecuteParams {
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub headers_json: Option<String>,
    pub query_json: Option<String>,
    pub body_type: Option<String>,
    pub body_text: Option<String>,
    pub content_type: Option<String>,
}

pub async fn execute_request(
    ctx: &AppContext,
    actor_id: i32,
    preset_id: Option<i32>,
    params: HttpRequestExecuteParams,
) -> ApiResult<http_request_runs::Model> {
    let normalized = normalize_request(params)?;
    let client = http_client::build_http_client(&ctx.db).await?;
    let mut url =
        Url::parse(&normalized.url).map_err(|_| ApiError::bad_request("request url is invalid"))?;
    apply_query(&mut url, normalized.query_json.as_deref())?;
    let method = parse_method(&normalized.method)?;
    let mut builder = client.request(method, url.clone());

    for (key, value) in parse_string_map(normalized.headers_json.as_deref(), "headers_json")? {
        builder = builder.header(key, value);
    }
    if let Some(content_type) = normalized
        .content_type
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        builder = builder.header(reqwest::header::CONTENT_TYPE, content_type.trim());
    }
    if let Some(body) = normalized
        .body_text
        .clone()
        .filter(|_| normalized.body_type != "none")
    {
        builder = builder.body(body);
    }

    let started = Instant::now();
    let result = builder.send().await;
    let duration_ms = i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX);
    let request_body_size_bytes = normalized
        .body_text
        .as_ref()
        .map_or(0, |value| i64::try_from(value.len()).unwrap_or(i64::MAX));

    let active = match result {
        Ok(response) => {
            let status = response.status();
            let response_headers_json = serialize_headers(response.headers())?;
            let response_content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string);
            let bytes = response.bytes().await.map_err(|error| {
                ApiError::bad_request(format!("request response read failed: {error}"))
            })?;
            let response_size_bytes = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
            let storage_path = write_response_body(&bytes)?;
            let preview = preview_text(&bytes);
            http_request_runs::ActiveModel {
                preset_id: Set(preset_id),
                name: Set(normalized
                    .name
                    .unwrap_or_else(|| format!("{} {}", normalized.method, normalized.url))),
                method: Set(normalized.method),
                url: Set(url.to_string()),
                headers_json: Set(normalized.headers_json),
                query_json: Set(normalized.query_json),
                request_body_type: Set(normalized.body_type),
                request_body_text: Set(normalized.body_text),
                request_body_size_bytes: Set(request_body_size_bytes),
                status: Set(if status.is_success() {
                    "success"
                } else {
                    "http_error"
                }
                .to_string()),
                response_status_code: Set(Some(i32::from(status.as_u16()))),
                response_headers_json: Set(Some(response_headers_json)),
                response_content_type: Set(response_content_type),
                response_size_bytes: Set(response_size_bytes),
                duration_ms: Set(duration_ms),
                error_message: Set(None),
                response_storage_path: Set(Some(storage_path.display().to_string())),
                response_preview_text: Set(preview),
                created_by: Set(Some(actor_id)),
                ..Default::default()
            }
        }
        Err(error) => http_request_runs::ActiveModel {
            preset_id: Set(preset_id),
            name: Set(normalized
                .name
                .unwrap_or_else(|| format!("{} {}", normalized.method, normalized.url))),
            method: Set(normalized.method),
            url: Set(url.to_string()),
            headers_json: Set(normalized.headers_json),
            query_json: Set(normalized.query_json),
            request_body_type: Set(normalized.body_type),
            request_body_text: Set(normalized.body_text),
            request_body_size_bytes: Set(request_body_size_bytes),
            status: Set("network_error".to_string()),
            response_status_code: Set(None),
            response_headers_json: Set(None),
            response_content_type: Set(None),
            response_size_bytes: Set(0),
            duration_ms: Set(duration_ms),
            error_message: Set(Some(trim_message(&error.to_string(), 1000))),
            response_storage_path: Set(None),
            response_preview_text: Set(None),
            created_by: Set(Some(actor_id)),
            ..Default::default()
        },
    };

    active.insert(&ctx.db).await.map_err(ApiError::from)
}

pub fn normalize_request(params: HttpRequestExecuteParams) -> ApiResult<HttpRequestDraft> {
    let method = params.method.trim().to_uppercase();
    parse_method(&method)?;
    let url = Url::parse(params.url.trim())
        .map_err(|_| ApiError::bad_request("request url is invalid"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::bad_request("request url must use http or https"));
    }
    parse_string_map(params.headers_json.as_deref(), "headers_json")?;
    parse_string_map(params.query_json.as_deref(), "query_json")?;
    let body_type = params.body_type.unwrap_or_else(|| "none".to_string());
    let body_type = match body_type.trim() {
        "" | "none" => "none",
        "json" => {
            if let Some(body) = params
                .body_text
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                serde_json::from_str::<serde_json::Value>(body)
                    .map_err(|_| ApiError::bad_request("request body must be JSON"))?;
            }
            "json"
        }
        "form" => "form",
        "raw" => "raw",
        _ => return Err(ApiError::bad_request("body_type is invalid")),
    }
    .to_string();

    Ok(HttpRequestDraft {
        name: params.name.and_then(|value| non_empty(value.trim())),
        method,
        url: url.to_string(),
        headers_json: normalize_json_map(params.headers_json.as_deref(), "headers_json")?,
        query_json: normalize_json_map(params.query_json.as_deref(), "query_json")?,
        body_type,
        body_text: params.body_text.and_then(|value| non_empty(value.trim())),
        content_type: params
            .content_type
            .and_then(|value| non_empty(value.trim())),
        source_format: None,
    })
}

pub fn parse_import(format: &str, content: &str) -> ApiResult<HttpRequestDraft> {
    match format.trim() {
        "curl" => parse_curl(content),
        "postman" => parse_postman(content),
        "request" => parse_raw_request(content),
        _ => Err(ApiError::bad_request("import format is invalid")),
    }
}

fn parse_method(value: &str) -> ApiResult<Method> {
    match value.trim().to_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        "HEAD" => Ok(Method::HEAD),
        "OPTIONS" => Ok(Method::OPTIONS),
        _ => Err(ApiError::bad_request("request method is invalid")),
    }
}

fn apply_query(url: &mut Url, query_json: Option<&str>) -> ApiResult<()> {
    for (key, value) in parse_string_map(query_json, "query_json")? {
        url.query_pairs_mut().append_pair(&key, &value);
    }
    Ok(())
}

fn parse_string_map(value: Option<&str>, field: &str) -> ApiResult<BTreeMap<String, String>> {
    let Some(raw) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(BTreeMap::new());
    };
    let parsed = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(raw)
        .map_err(|_| ApiError::bad_request(format!("{field} must be JSON object")))?;
    let mut values = BTreeMap::new();
    for (key, value) in parsed {
        let string_value = value
            .as_str()
            .map(ToString::to_string)
            .unwrap_or_else(|| value.to_string());
        values.insert(key, string_value);
    }
    Ok(values)
}

fn normalize_json_map(value: Option<&str>, field: &str) -> ApiResult<Option<String>> {
    let values = parse_string_map(value, field)?;
    if values.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(&values)
            .map(Some)
            .map_err(|_| ApiError::internal("failed to serialize request JSON"))
    }
}

fn serialize_headers(headers: &HeaderMap) -> ApiResult<String> {
    let mut values = BTreeMap::new();
    for (key, value) in headers {
        if let Ok(value) = value.to_str() {
            values.insert(key.as_str().to_string(), value.to_string());
        }
    }
    serde_json::to_string(&values)
        .map_err(|_| ApiError::internal("failed to serialize response headers"))
}

fn write_response_body(bytes: &[u8]) -> ApiResult<PathBuf> {
    let now = Local::now();
    let path = PathBuf::from(RESULT_ROOT)
        .join(now.year().to_string())
        .join(format!("{:02}", now.month()))
        .join(format!("{:02}", now.day()))
        .join(format!("{}.bin", Uuid::new_v4()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| ApiError::internal("failed to prepare response storage"))?;
    }
    fs::write(&path, bytes).map_err(|_| ApiError::internal("failed to save response body"))?;
    Ok(path)
}

fn preview_text(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let slice = &bytes[..bytes.len().min(PREVIEW_LIMIT)];
    Some(String::from_utf8_lossy(slice).to_string())
}

fn parse_curl(content: &str) -> ApiResult<HttpRequestDraft> {
    let tokens = shell_tokens(content);
    if tokens.first().is_none_or(|token| token != "curl") {
        return Err(ApiError::bad_request("curl content must start with curl"));
    }
    let mut method = None;
    let mut headers = BTreeMap::new();
    let mut body = None;
    let mut url = None;
    let mut index = 1;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "-X" | "--request" => {
                index += 1;
                method = tokens.get(index).cloned();
            }
            "-H" | "--header" => {
                index += 1;
                if let Some(header) = tokens.get(index) {
                    if let Some((key, value)) = header.split_once(':') {
                        headers.insert(key.trim().to_string(), value.trim().to_string());
                    }
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => {
                index += 1;
                body = tokens.get(index).cloned();
            }
            value if value.starts_with("http://") || value.starts_with("https://") => {
                url = Some(value.to_string());
            }
            _ => {}
        }
        index += 1;
    }
    let body_type = if body.is_some() { "raw" } else { "none" }.to_string();
    let draft = HttpRequestDraft {
        name: None,
        method: method.unwrap_or_else(|| if body.is_some() { "POST" } else { "GET" }.to_string()),
        url: url.ok_or_else(|| ApiError::bad_request("curl url is required"))?,
        headers_json: map_to_json(headers)?,
        query_json: None,
        body_type,
        body_text: body,
        content_type: None,
        source_format: Some("curl".to_string()),
    };
    normalize_request(draft.into_execute())
}

fn parse_postman(content: &str) -> ApiResult<HttpRequestDraft> {
    let value = serde_json::from_str::<serde_json::Value>(content)
        .map_err(|_| ApiError::bad_request("postman content must be JSON"))?;
    let request = value.get("request").unwrap_or(&value);
    let method = request
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("GET")
        .to_string();
    let url = request
        .get("url")
        .and_then(|url| {
            url.as_str().map(ToString::to_string).or_else(|| {
                url.get("raw")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string)
            })
        })
        .ok_or_else(|| ApiError::bad_request("postman url is required"))?;
    let mut headers = BTreeMap::new();
    if let Some(items) = request.get("header").and_then(serde_json::Value::as_array) {
        for item in items {
            if item.get("disabled").and_then(serde_json::Value::as_bool) == Some(true) {
                continue;
            }
            if let (Some(key), Some(value)) = (
                item.get("key").and_then(serde_json::Value::as_str),
                item.get("value").and_then(serde_json::Value::as_str),
            ) {
                headers.insert(key.to_string(), value.to_string());
            }
        }
    }
    let body_text = request
        .get("body")
        .and_then(|body| body.get("raw"))
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string);
    let body_type = if body_text.is_some() { "raw" } else { "none" }.to_string();
    normalize_request(
        HttpRequestDraft {
            name: value
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            method,
            url,
            headers_json: map_to_json(headers)?,
            query_json: None,
            body_type,
            body_text,
            content_type: None,
            source_format: Some("postman".to_string()),
        }
        .into_execute(),
    )
}

fn parse_raw_request(content: &str) -> ApiResult<HttpRequestDraft> {
    let normalized = content.replace("\r\n", "\n");
    let mut sections = normalized.splitn(2, "\n\n");
    let head = sections.next().unwrap_or_default();
    let body = sections.next().map(ToString::to_string);
    let mut lines = head.lines();
    let first = lines
        .next()
        .ok_or_else(|| ApiError::bad_request("request first line is required"))?;
    let mut first_parts = first.split_whitespace();
    let method = first_parts
        .next()
        .ok_or_else(|| ApiError::bad_request("request method is required"))?
        .to_string();
    let url = first_parts
        .next()
        .ok_or_else(|| ApiError::bad_request("request url is required"))?
        .to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    normalize_request(
        HttpRequestDraft {
            name: None,
            method,
            url,
            headers_json: map_to_json(headers)?,
            query_json: None,
            body_type: if body
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            {
                "raw".to_string()
            } else {
                "none".to_string()
            },
            body_text: body.and_then(|value| non_empty(value.trim())),
            content_type: None,
            source_format: Some("request".to_string()),
        }
        .into_execute(),
    )
}

fn shell_tokens(content: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for char in content.chars() {
        if escaped {
            current.push(char);
            escaped = false;
            continue;
        }
        if char == '\\' {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if char == active_quote {
                quote = None;
            } else {
                current.push(char);
            }
            continue;
        }
        if char == '\'' || char == '"' {
            quote = Some(char);
            continue;
        }
        if char.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(char);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn map_to_json(values: BTreeMap<String, String>) -> ApiResult<Option<String>> {
    if values.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(&values)
            .map(Some)
            .map_err(|_| ApiError::internal("failed to serialize import values"))
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn trim_message(message: &str, limit: usize) -> String {
    message.chars().take(limit).collect()
}

impl HttpRequestDraft {
    fn into_execute(self) -> HttpRequestExecuteParams {
        HttpRequestExecuteParams {
            name: self.name,
            method: self.method,
            url: self.url,
            headers_json: self.headers_json,
            query_json: self.query_json,
            body_type: Some(self.body_type),
            body_text: self.body_text,
            content_type: self.content_type,
        }
    }
}
