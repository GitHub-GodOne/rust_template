use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

use crate::models::_entities::operation_logs;

#[derive(Debug)]
pub struct LogInput<'a> {
    pub log_type: &'a str,
    pub level: &'a str,
    pub module: &'a str,
    pub action: &'a str,
    pub message: &'a str,
    pub user_id: Option<i32>,
    pub operator: Option<String>,
    pub method: Option<&'a str>,
    pub path: Option<&'a str>,
    pub status: Option<i32>,
    pub error_message: Option<String>,
}

/// Writes a detailed log.
///
/// Inside a request it also picks up the client IP, user agent and duration,
/// follows that feature's audit switch, and stops the audit middleware from
/// adding a second, generic entry.
pub async fn record(db: &DatabaseConnection, input: LogInput<'_>) {
    let request = crate::services::audit::current_request();
    if request.as_ref().is_some_and(|(enabled, ..)| !enabled) {
        return;
    }
    let (ip, user_agent, duration_ms) = request
        .map(|(_, ip, user_agent, duration)| (ip, user_agent, Some(duration)))
        .unwrap_or_default();
    let log = operation_logs::ActiveModel {
        ip: Set(ip),
        user_agent: Set(user_agent),
        duration_ms: Set(duration_ms),
        log_type: Set(input.log_type.to_string()),
        level: Set(input.level.to_string()),
        module: Set(input.module.to_string()),
        action: Set(input.action.to_string()),
        message: Set(input.message.to_string()),
        user_id: Set(input.user_id),
        operator: Set(input.operator),
        method: Set(input.method.map(str::to_string)),
        path: Set(input.path.map(str::to_string)),
        status: Set(input.status),
        error_message: Set(input.error_message),
        ..Default::default()
    };

    if let Err(err) = log.insert(db).await {
        tracing::error!(error = err.to_string(), "failed to record operation log");
    }
}
