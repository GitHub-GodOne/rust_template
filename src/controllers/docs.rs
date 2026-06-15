use std::sync::OnceLock;

use axum::response::Html;
use loco_rs::prelude::*;
use utoipa::OpenApi;

use crate::openapi::ApiDoc;

static OPENAPI_JSON: OnceLock<String> = OnceLock::new();

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

async fn openapi_json() -> Result<Response> {
    Ok((
        [("content-type", "application/json; charset=utf-8")],
        openapi_json_string()?.to_string(),
    )
        .into_response())
}

async fn swagger_ui() -> Result<Response> {
    let spec = openapi_json_string()?;
    Ok(Html(format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>GPT Images API Docs</title>
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
}
