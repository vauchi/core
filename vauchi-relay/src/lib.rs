pub mod config;
pub mod connection_limit;
pub mod handler;
pub mod http;
pub mod metrics;
pub mod rate_limit;
pub mod recovery_storage;
pub mod storage;

use config::RelayConfig;
use metrics::RelayMetrics;
use recovery_storage::{MemoryRecoveryProofStore, RecoveryProofStore, SqliteRecoveryProofStore};
use std::sync::Arc;
use storage::{create_blob_store, BlobStore, StorageBackend};
use tokio::net::TcpListener;

/// Test helper to start a relay server for integration tests
pub async fn test_start(config: RelayConfig, storage: Arc<dyn BlobStore>) -> TcpListener {
    TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test listener")
}
