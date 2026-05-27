use axum::{http::StatusCode, response::IntoResponse};
use loco_rs::{model::ModelError, Error};
use sea_orm::DbErr;

use crate::responses;

pub type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "UNAUTHORIZED",
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "BAD_REQUEST",
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "FORBIDDEN",
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INTERNAL_ERROR",
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        responses::error(self.status, self.code, &self.message)
    }
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        tracing::error!(error = err.to_string(), "request failed");
        Self::internal("request failed")
    }
}

impl From<ModelError> for ApiError {
    fn from(err: ModelError) -> Self {
        match err {
            ModelError::EntityNotFound => Self::bad_request("entity not found"),
            ModelError::EntityAlreadyExists => Self::bad_request("entity already exists"),
            _ => {
                tracing::error!(error = err.to_string(), "model operation failed");
                Self::internal("model operation failed")
            }
        }
    }
}

impl From<DbErr> for ApiError {
    fn from(err: DbErr) -> Self {
        tracing::error!(error = err.to_string(), "database operation failed");
        Self::internal("database operation failed")
    }
}
