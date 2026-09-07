#![allow(clippy::missing_errors_doc)]

use std::ops::Range;

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};

use crate::{
    errors::{ApiError, ApiResult},
    services::storage::ObjectByteStream,
};

pub fn requested_range(headers: &HeaderMap, total_size: usize) -> ApiResult<Option<Range<usize>>> {
    let Some(value) = headers.get(header::RANGE) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid Range header"))?;
    let bytes = value
        .strip_prefix("bytes=")
        .ok_or_else(|| ApiError::bad_request("only byte ranges are supported"))?;
    if bytes.contains(',') {
        return Err(ApiError::bad_request(
            "multiple byte ranges are not supported",
        ));
    }
    let (start, end) = bytes
        .split_once('-')
        .ok_or_else(|| ApiError::bad_request("invalid Range header"))?;
    if total_size == 0 {
        return Err(ApiError::bad_request("range is not satisfiable"));
    }

    if start.is_empty() {
        let suffix = end
            .parse::<usize>()
            .map_err(|_| ApiError::bad_request("invalid Range header"))?;
        if suffix == 0 {
            return Err(ApiError::bad_request("range is not satisfiable"));
        }
        return Ok(Some(total_size.saturating_sub(suffix)..total_size));
    }

    let start = start
        .parse::<usize>()
        .map_err(|_| ApiError::bad_request("invalid Range header"))?;
    if start >= total_size {
        return Err(ApiError::bad_request("range is not satisfiable"));
    }
    let end = if end.is_empty() {
        total_size - 1
    } else {
        end.parse::<usize>()
            .map_err(|_| ApiError::bad_request("invalid Range header"))?
            .min(total_size - 1)
    };
    if end < start {
        return Err(ApiError::bad_request("range is not satisfiable"));
    }
    Ok(Some(start..end + 1))
}

pub fn streaming_response(
    content: ObjectByteStream,
    mut headers: HeaderMap,
    partial: bool,
) -> ApiResult<Response> {
    let content_length = content.range.end.saturating_sub(content.range.start);
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.to_string())
            .map_err(|_| ApiError::internal("failed to build file response"))?,
    );
    if partial {
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!(
                "bytes {}-{}/{}",
                content.range.start,
                content.range.end.saturating_sub(1),
                content.total_size
            ))
            .map_err(|_| ApiError::internal("failed to build file response"))?,
        );
    }

    let mut response = Response::new(Body::from_stream(content.stream));
    *response.status_mut() = if partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    *response.headers_mut() = headers;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::storage;
    use uuid::Uuid;

    #[test]
    fn parses_standard_and_suffix_ranges() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=10-19"));
        assert_eq!(requested_range(&headers, 100).unwrap(), Some(10..20));

        headers.insert(header::RANGE, HeaderValue::from_static("bytes=-8"));
        assert_eq!(requested_range(&headers, 100).unwrap(), Some(92..100));

        headers.insert(header::RANGE, HeaderValue::from_static("bytes=95-"));
        assert_eq!(requested_range(&headers, 100).unwrap(), Some(95..100));
    }

    #[test]
    fn rejects_unsatisfiable_ranges() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=100-"));
        assert!(requested_range(&headers, 100).is_err());
    }

    #[tokio::test]
    async fn streams_a_local_file_range() {
        let path = std::env::temp_dir().join(format!(
            "gpt-images-range-test-{}.txt",
            Uuid::new_v4().simple()
        ));
        std::fs::write(&path, b"hello files").unwrap();

        let content = storage::stream_local_path(path.clone(), Some(6..11))
            .await
            .unwrap();
        let response = streaming_response(content, HeaderMap::new(), true).unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers().get(header::CONTENT_RANGE).unwrap(),
            "bytes 6-10/11"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"files");

        std::fs::remove_file(path).unwrap();
    }
}
