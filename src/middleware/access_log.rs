//! One log line per request with the client's address, so connectivity
//! problems can be traced to a network path (LAN, tunnel, proxy).

use std::{
    net::SocketAddr,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, Request},
    http::HeaderMap,
    middleware::Next,
    response::Response,
    Router as AXRouter,
};
use futures_util::Stream;
use loco_rs::{app::AppContext, controller::middleware::MiddlewareLayer, Result};

pub struct AccessLogMiddleware;

impl MiddlewareLayer for AccessLogMiddleware {
    fn name(&self) -> &'static str {
        "access_log"
    }

    fn config(&self) -> serde_json::Result<serde_json::Value> {
        Ok(serde_json::json!({ "enabled": true }))
    }

    fn apply(&self, app: AXRouter<AppContext>) -> Result<AXRouter<AppContext>> {
        Ok(app.layer(axum::middleware::from_fn(log_request)))
    }
}

/// The TCP peer of the connection, when the server was started with
/// connect info (the normal `cargo loco start` path).
#[must_use]
pub fn peer_address(request: &Request) -> Option<SocketAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| *address)
}

/// Client address reported by a proxy in front of us (cloudflared, nginx).
#[must_use]
pub fn forwarded_for(headers: &HeaderMap) -> Option<String> {
    ["cf-connecting-ip", "x-forwarded-for", "x-real-ip"]
        .iter()
        .find_map(|name| headers.get(*name)?.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// Best guess at the real client: the forwarded address, else the peer.
#[must_use]
pub fn client_ip(request: &Request) -> Option<String> {
    forwarded_for(request.headers())
        .or_else(|| peer_address(request).map(|address| address.ip().to_string()))
}

async fn log_request(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let peer = peer_address(&request).map(|address| address.to_string());
    let forwarded = forwarded_for(request.headers());
    let started = Instant::now();
    let response = next.run(request).await;
    // Frontend bundles are big: log when the body has actually been delivered
    // (or the client gave up), which is what a slow network shows up as.
    if path.starts_with("/static/") {
        return track_transfer(
            response,
            TransferLog {
                path,
                peer,
                started,
                sent: 0,
                ended: false,
            },
        );
    }
    tracing::info!(
        method = %method,
        path,
        status = response.status().as_u16(),
        latency_ms = started.elapsed().as_millis(),
        peer = peer.as_deref().unwrap_or("-"),
        forwarded = forwarded.as_deref().unwrap_or("-"),
        "request"
    );
    response
}

struct TransferLog {
    path: String,
    peer: Option<String>,
    started: Instant,
    sent: usize,
    ended: bool,
}

impl Drop for TransferLog {
    fn drop(&mut self) {
        tracing::info!(
            path = self.path,
            peer = self.peer.as_deref().unwrap_or("-"),
            bytes = self.sent,
            transfer_ms = self.started.elapsed().as_millis(),
            complete = self.ended,
            "static transfer"
        );
    }
}

/// Body stream that counts bytes and notices whether it reached the end.
struct CountingStream<S> {
    inner: S,
    log: TransferLog,
}

impl<S> Stream for CountingStream<S>
where
    S: Stream<Item = Result<Bytes, axum::Error>> + Unpin,
{
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll = Pin::new(&mut self.inner).poll_next(cx);
        match &poll {
            Poll::Ready(Some(Ok(chunk))) => self.log.sent += chunk.len(),
            Poll::Ready(None) => self.log.ended = true,
            _ => {}
        }
        poll
    }
}

fn track_transfer(response: Response, log: TransferLog) -> Response {
    let (parts, body) = response.into_parts();
    let stream = CountingStream {
        inner: body.into_data_stream(),
        log,
    };
    // A declared Content-Length stays in the headers; the stream yields
    // exactly that many bytes.
    Response::from_parts(parts, Body::from_stream(stream))
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request as HttpRequest};

    use super::*;

    #[test]
    fn prefers_the_forwarded_client_over_the_peer() {
        let mut request = HttpRequest::builder()
            .header("x-forwarded-for", "203.0.113.9, 10.0.0.1")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([172, 20, 10, 2], 51000))));
        assert_eq!(client_ip(&request).as_deref(), Some("203.0.113.9"));

        request.headers_mut().remove("x-forwarded-for");
        assert_eq!(client_ip(&request).as_deref(), Some("172.20.10.2"));
        assert_eq!(
            peer_address(&request)
                .map(|address| address.to_string())
                .as_deref(),
            Some("172.20.10.2:51000")
        );
    }
}
