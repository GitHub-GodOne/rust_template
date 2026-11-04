#![allow(clippy::missing_errors_doc)]
//! Small CSV downloads (users, work orders, payment orders): built in memory,
//! UTF-8 with BOM so Excel opens Chinese text correctly.

use axum::{body::Body, http::header, response::Response};
use chrono::Local;

use crate::errors::{ApiError, ApiResult};

/// Upper bound for one in-memory export.
pub const MAX_ROWS: u64 = 50_000;

/// One CSV field. Values a spreadsheet would run as a formula are prefixed
/// with `'`.
#[must_use]
pub fn field(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[must_use]
pub fn document(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = String::from("\u{feff}");
    out.push_str(
        &headers
            .iter()
            .map(|h| field(h))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("\r\n");
    for row in rows {
        out.push_str(
            &row.iter()
                .map(|value| field(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push_str("\r\n");
    }
    out
}

/// Errors out before loading when the filtered set is too big.
pub fn ensure_within_limit(total: u64) -> ApiResult<()> {
    if total > MAX_ROWS {
        return Err(ApiError::bad_request(format!(
            "符合条件的数据有 {total} 条，超过单次导出上限 {MAX_ROWS} 条，请增加筛选条件"
        )));
    }
    Ok(())
}

/// `name-20261019-153000.csv` download response.
pub fn response(name: &str, headers: &[&str], rows: &[Vec<String>]) -> ApiResult<Response> {
    let filename = format!("{name}-{}.csv", Local::now().format("%Y%m%d-%H%M%S"));
    Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .header("x-total-count", rows.len().to_string())
        .body(Body::from(document(headers, rows)))
        .map_err(|error| ApiError::internal(error.to_string()))
}

/// Local time as shown in the admin UI.
#[must_use]
pub fn time(value: &chrono::DateTime<chrono::FixedOffset>) -> String {
    value
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[must_use]
pub fn optional_time(value: Option<&chrono::DateTime<chrono::FixedOffset>>) -> String {
    value.map(time).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{document, field};

    #[test]
    fn fields_are_quoted_and_formula_safe() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("=SUM(A1)"), "'=SUM(A1)");
        let csv = document(&["名称", "值"], &[vec!["a".into(), "1,2".into()]]);
        assert_eq!(csv, "\u{feff}名称,值\r\na,\"1,2\"\r\n");
    }
}
