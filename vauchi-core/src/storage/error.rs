//! Storage error types.

use thiserror::Error;

/// Storage error types.
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),
}

/// Pending update status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    Pending,
    Sending,
    Failed { error: String, retry_at: u64 },
}

/// A pending sync update.
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub id: String,
    pub contact_id: String,
    pub update_type: String,
    pub payload: Vec<u8>,
    pub created_at: u64,
    pub retry_count: u32,
    pub status: UpdateStatus,
}
