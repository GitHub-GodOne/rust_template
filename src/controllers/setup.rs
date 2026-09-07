use axum::http::{header, HeaderMap};
use loco_rs::prelude::*;

use crate::{responses, setup::SetupStatusData};

#[must_use]
pub fn routes() -> Routes {
    Routes::new().add("/api/setup/status", get(status))
}

async fn status(State(ctx): State<AppContext>, headers: HeaderMap) -> Result<Response> {
    Ok(responses::ok(SetupStatusData {
        configured: true,
        phase: "ready".to_string(),
        environment: ctx.environment.to_string(),
        config_file: None,
        port: request_port(&headers).or_else(|| u16::try_from(ctx.config.server.port).ok()),
    }))
}

fn request_port(headers: &HeaderMap) -> Option<u16> {
    let host = headers.get(header::HOST)?.to_str().ok()?;
    host.rsplit_once(':')?.1.parse().ok()
}

#[cfg(test)]
mod tests {
    use axum::http::{header, HeaderMap, HeaderValue};

    use super::request_port;

    #[test]
    fn reads_runtime_port_from_host_header() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:5150"));
        assert_eq!(request_port(&headers), Some(5150));
    }
}
