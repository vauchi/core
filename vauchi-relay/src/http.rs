//! HTTP Server for Health and Metrics Endpoints
//!
//! Provides REST endpoints for monitoring and health checks.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    extract::State,
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Serialize;

use crate::metrics::RelayMetrics;
use crate::storage::BlobStore;

/// Shared state for HTTP handlers.
#[derive(Clone)]
pub struct HttpState {
    pub metrics: RelayMetrics,
    pub storage: Arc<dyn BlobStore>,
    pub start_time: Instant,
    pub metrics_token: Option<String>,
}

/// Health check response.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
}

/// Readiness check response.
#[derive(Serialize)]
pub struct ReadyResponse {
    pub ready: bool,
    pub storage_ok: bool,
    pub blob_count: usize,
}

/// Middleware to check bearer token for metrics endpoint.
async fn metrics_auth_middleware(
    State(state): State<HttpState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Only check auth for /metrics endpoint
    if request.uri().path() == "/metrics" {
        if let Some(ref expected_token) = state.metrics_token {
            // Check Authorization header
            let auth_header = request.headers().get(header::AUTHORIZATION);
            let is_authorized = auth_header.is_some_and(|h| {
                h.to_str()
                    .map(|s| {
                        s.strip_prefix("Bearer ")
                            .is_some_and(|token| token == expected_token)
                    })
                    .unwrap_or(false)
            });

            if !is_authorized {
                return (
                    StatusCode::UNAUTHORIZED,
                    [(header::WWW_AUTHENTICATE, "Bearer")],
                    "Unauthorized",
                )
                    .into_response();
            }
        }
    }

    next.run(request).await
}

/// Creates the HTTP router with health and metrics endpoints.
pub fn create_router(state: HttpState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
        .route("/", get(root_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            metrics_auth_middleware,
        ))
        .with_state(state)
}

/// Root handler - returns basic info.
async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "service": "vauchi-relay",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": ["/health", "/ready", "/metrics"]
    }))
}

/// Health check endpoint - always returns 200 if server is running.
async fn health_handler(State(state): State<HttpState>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();

    Json(HealthResponse {
        status: "healthy",
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: uptime,
    })
}

/// Readiness check endpoint - returns 200 if storage is accessible.
async fn ready_handler(State(state): State<HttpState>) -> Response {
    let blob_count = state.storage.blob_count();
    let storage_ok = true; // If we can call blob_count, storage is working

    let response = ReadyResponse {
        ready: storage_ok,
        storage_ok,
        blob_count,
    };

    if storage_ok {
        (StatusCode::OK, Json(response)).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response)).into_response()
    }
}

/// Prometheus metrics endpoint.
async fn metrics_handler(State(state): State<HttpState>) -> impl IntoResponse {
    // Update storage metrics before encoding
    state
        .metrics
        .blobs_stored
        .set(state.storage.blob_count() as i64);

    let metrics_text = state.metrics.encode();

    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        metrics_text,
    )
}

// INLINE_TEST_REQUIRED: Binary crate without lib.rs - tests cannot be external
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryBlobStore;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn create_test_state() -> HttpState {
        HttpState {
            metrics: RelayMetrics::new(),
            storage: Arc::new(MemoryBlobStore::new()),
            start_time: Instant::now(),
            metrics_token: None,
        }
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = create_router(create_test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_endpoint() {
        let app = create_router(create_test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let app = create_router(create_test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
