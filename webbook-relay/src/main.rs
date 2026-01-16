//! WebBook Relay Server
//!
//! A lightweight relay server for forwarding encrypted contact card updates.

mod config;
mod handler;
mod rate_limit;
mod recovery_storage;
mod storage;

use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tracing::{info, error};

use config::RelayConfig;
use rate_limit::RateLimiter;
use recovery_storage::{RecoveryProofStore, MemoryRecoveryProofStore, SqliteRecoveryProofStore};
use storage::{BlobStore, create_blob_store, StorageBackend};

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("webbook_relay=info".parse().unwrap())
        )
        .init();

    // Load configuration
    let config = RelayConfig::from_env();
    info!("Starting WebBook Relay Server");
    info!("Listening on {}", config.listen_addr);
    info!("Storage backend: {:?}", config.storage_backend);

    // Initialize shared state
    let storage: Arc<dyn BlobStore> = Arc::from(
        create_blob_store(config.storage_backend, Some(&config.data_dir))
    );

    // Initialize recovery proof storage
    let recovery_storage: Arc<dyn RecoveryProofStore> = match config.storage_backend {
        StorageBackend::Memory => Arc::new(MemoryRecoveryProofStore::new()),
        StorageBackend::Sqlite => {
            let path = config.data_dir.join("recovery_proofs.db");
            Arc::new(SqliteRecoveryProofStore::open(&path).expect("Failed to open recovery proof database"))
        }
    };

    let rate_limiter = Arc::new(RateLimiter::new(config.rate_limit_per_min));

    // Start cleanup task for blobs
    let cleanup_storage = storage.clone();
    let blob_ttl = config.blob_ttl();
    let cleanup_interval = config.cleanup_interval();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(cleanup_interval).await;
            let removed = cleanup_storage.cleanup_expired(blob_ttl);
            if removed > 0 {
                info!("Cleaned up {} expired blobs", removed);
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

    // Start TCP listener
    let listener = TcpListener::bind(&config.listen_addr).await.expect("Failed to bind");

    // Accept connections
    while let Ok((stream, addr)) = listener.accept().await {
        let storage = storage.clone();
        let recovery_storage = recovery_storage.clone();
        let rate_limiter = rate_limiter.clone();
        let max_message_size = config.max_message_size;

        tokio::spawn(async move {
            match accept_async(stream).await {
                Ok(ws_stream) => {
                    info!("New connection from {}", addr);
                    handler::handle_connection(ws_stream, storage, recovery_storage, rate_limiter, max_message_size).await;
                    info!("Connection closed: {}", addr);
                }
                Err(e) => {
                    error!("WebSocket handshake failed for {}: {}", addr, e);
                }
            }
        });
    }
}
