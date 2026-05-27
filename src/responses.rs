#![allow(clippy::option_if_let_else)]

use axum::{http::StatusCode, response::IntoResponse};
use loco_rs::prelude::{Json, Response};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ApiResponse<T>
where
    T: Serialize,
{
    pub success: bool,
    pub code: String,
    pub message: String,
    pub data: Option<T>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct EmptyData {}

#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PageParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    pub keyword: Option<String>,
}

impl PageParams {
    #[must_use]
    pub fn page(&self) -> u64 {
        self.page.unwrap_or(1).max(1)
    }

    #[must_use]
    pub fn page_size(&self) -> u64 {
        self.page_size.unwrap_or(10).clamp(1, 100)
    }
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct PageResponse<T>
where
    T: Serialize,
{
    pub items: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
}

pub fn ok<T>(data: T) -> Response
where
    T: Serialize,
{
    Json(ApiResponse {
        success: true,
        code: "OK".to_string(),
        message: "ok".to_string(),
        data: Some(data),
    })
    .into_response()
}

#[must_use]
pub fn empty() -> Response {
    ok(EmptyData {})
}

#[must_use]
pub fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ApiResponse::<EmptyData> {
            success: false,
            code: code.to_string(),
            message: message.to_string(),
            data: None,
        }),
    )
        .into_response()
}
