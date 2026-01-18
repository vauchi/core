//! Vauchi Relay Server
//!
//! A lightweight relay server for forwarding encrypted contact card updates.
//! Provides:
//! - WebSocket endpoint for encrypted blob storage and delivery
//! - HTTP endpoints for health checks and Prometheus metrics
//! - Rate limiting and abuse prevention
//! - Recovery proof storage for contact recovery

mod config;
mod connection_limit;
mod handler;
mod http;
mod metrics;
mod rate_limit;
mod recovery_storage;
mod storage;

use std::sync::Arc;
use std::time::Instant;

use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tracing::{error, info};

use config::RelayConfig;
use connection_limit::ConnectionLimiter;
use http::{create_router, HttpState};
use metrics::RelayMetrics;
use rate_limit::RateLimiter;
use recovery_storage::{MemoryRecoveryProofStore, RecoveryProofStore, SqliteRecoveryProofStore};
use storage::{create_blob_store, BlobStore, StorageBackend};

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vauchi_relay=info".parse().unwrap()),
        )
        .init();

    // Load configuration
    let config = RelayConfig::from_env();
    info!(
        "Starting Vauchi Relay Server v{}",
        env!("CARGO_PKG_VERSION")
    );
    info!("WebSocket: {}", config.listen_addr);
    info!("HTTP (health/metrics): {}:8081", config.listen_addr.ip());
    info!("Storage backend: {:?}", config.storage_backend);

    // Initialize metrics
    let metrics = RelayMetrics::new();

    // Initialize shared state
    let storage: Arc<dyn BlobStore> = Arc::from(create_blob_store(
        config.storage_backend,
        Some(&config.data_dir),
    ));

    // Initialize recovery proof storage
    let recovery_storage: Arc<dyn RecoveryProofStore> = match config.storage_backend {
        StorageBackend::Memory => Arc::new(MemoryRecoveryProofStore::new()),
        StorageBackend::Sqlite => {
            let path = config.data_dir.join("recovery_proofs.db");
            Arc::new(
                SqliteRecoveryProofStore::open(&path)
                    .expect("Failed to open recovery proof database"),
            )
        }
    };

    let rate_limiter = Arc::new(RateLimiter::new(config.rate_limit_per_min));
    let connection_limiter = ConnectionLimiter::new(config.max_connections);
    let start_time = Instant::now();

    // Start HTTP server for health/metrics
    let http_state = HttpState {
        metrics: metrics.clone(),
        storage: storage.clone(),
        start_time,
    };
    let http_router = create_router(http_state);

    // Parse HTTP listen address (same host, port 8081)
    let http_addr = format!("{}:8081", config.listen_addr.ip());

    let http_listener = TcpListener::bind(&http_addr)
        .await
        .expect("Failed to bind HTTP listener");

    tokio::spawn(async move {
        info!("HTTP server listening on {}", http_addr);
        axum::serve(http_listener, http_router).await.unwrap();
    });

    // Start cleanup task for blobs
    let cleanup_storage = storage.clone();
    let cleanup_metrics = metrics.clone();
    let blob_ttl = config.blob_ttl();
    let cleanup_interval = config.cleanup_interval();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(cleanup_interval).await;
            let removed = cleanup_storage.cleanup_expired(blob_ttl);
            if removed > 0 {
                info!("Cleaned up {} expired blobs", removed);
                cleanup_metrics.blobs_expired.inc_by(removed as u64);
            }
        }
    });

    // Start cleanup task for recovery proofs
    let cleanup_recovery = recovery_storage.clone();
    tokio::spawn(async move {
        loop {
            // Check every hour for expired proofs
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            let removed = cleanup_recovery.cleanup_expired();
            if removed > 0 {
                info!("Cleaned up {} expired recovery proofs", removed);
            }
        }
    });

    // Start cleanup task for rate limiter (remove stale client buckets)
    let cleanup_rate_limiter = rate_limiter.clone();
    tokio::spawn(async move {
        loop {
            // Clean up every 10 minutes, removing clients idle for 30 minutes
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            let removed = cleanup_rate_limiter.cleanup_inactive(std::time::Duration::from_secs(1800));
            if removed > 0 {
                info!("Cleaned up {} stale rate limiter entries", removed);
            }
        }
    });

    // Start TCP listener for WebSocket
    let listener = TcpListener::bind(&config.listen_addr)
        .await
        .expect("Failed to bind WebSocket listener");

    info!("WebSocket server listening on {}", config.listen_addr);

    // Accept connections
    while let Ok((stream, addr)) = listener.accept().await {
        // Enforce connection limit
        let connection_guard = match connection_limiter.try_acquire() {
            Some(guard) => guard,
            None => {
                tracing::warn!(
                    "Connection rejected from {}: at max capacity ({}/{})",
                    addr,
                    connection_limiter.active_count(),
                    config.max_connections
                );
                metrics.connection_errors.inc();
                // Drop the stream to close the connection
                drop(stream);
                continue;
            }
        };

        let storage = storage.clone();
        let recovery_storage = recovery_storage.clone();
        let rate_limiter = rate_limiter.clone();
        let metrics = metrics.clone();
        let max_message_size = config.max_message_size;

        tokio::spawn(async move {
            // Keep the guard alive for the duration of the connection
            let _guard = connection_guard;

            // Peek at the first bytes to detect HTTP health check vs WebSocket
            let mut peek_buf = [0u8; 64];
            match stream.peek(&mut peek_buf).await {
                Ok(n) if n > 0 => {
                    let peek_str = String::from_utf8_lossy(&peek_buf[..n]);

                    // Check for HTTP GET /health or /up request (non-WebSocket)
                    if (peek_str.starts_with("GET /health") || peek_str.starts_with("GET /up"))
                        && !peek_str.contains("Upgrade:")
                    {
                        // Respond with health check JSON
                        let uptime = start_time.elapsed().as_secs();
                        let health_response = format!(
                            r#"{{"status":"healthy","version":"{}","uptime_seconds":{}}}"#,
                            env!("CARGO_PKG_VERSION"),
                            uptime
                        );
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            health_response.len(),
                            health_response
                        );
                        let _ = stream.try_write(response.as_bytes());
                        return;
                    }
                }
                _ => {}
            }

            // Proceed with WebSocket handshake
            match accept_async(stream).await {
                Ok(ws_stream) => {
                    info!("New connection from {}", addr);
                    metrics.connections_total.inc();
                    metrics.connections_active.inc();

                    handler::handle_connection(
                        ws_stream,
                        storage,
                        recovery_storage,
                        rate_limiter,
                        max_message_size,
                    )
                    .await;

                    metrics.connections_active.dec();
                    info!("Connection closed: {}", addr);
                }
                Err(e) => {
                    error!("WebSocket handshake failed for {}: {}", addr, e);
                    metrics.connection_errors.inc();
                }
            }
            // _guard dropped here, releasing the connection slot
        });
    }
}
