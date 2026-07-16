// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! API Error Types
//!
//! Unified error type for the Vauchi API layer.

use thiserror::Error;

use crate::api::sync::SyncError;
use crate::contact::ContactError;
use crate::contact_card::{ContactCardError, ValidationError};
use crate::exchange::ExchangeError;
use crate::network::NetworkError;
use crate::storage::StorageError;
use crate::sync::device_sync::DeviceSyncError;

/// Unified error type for Vauchi operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum VauchiError {
    /// Contact card validation failed.
    #[error("validation error: {0}")]
    Validation(#[from] ValidationError),

    /// Contact card operation failed (max fields, avatar too large, etc.).
    #[error("contact card error: {0}")]
    ContactCard(#[from] ContactCardError),

    /// Contact-level invariant violation (e.g. recovery-trust requires
    /// in-person verified contact).
    #[error("contact error: {0}")]
    Contact(#[from] ContactError),

    /// Key exchange failed.
    #[error("exchange error: {0}")]
    Exchange(#[from] ExchangeError),

    /// Storage operation failed.
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    /// Sync operation failed.
    #[error("sync error: {0}")]
    Sync(#[from] SyncError),

    /// Device sync operation failed.
    #[error("device sync error: {0}")]
    DeviceSync(#[from] DeviceSyncError),

    /// Network operation failed.
    #[error("network error: {0}")]
    Network(#[from] NetworkError),

    /// Contact not found.
    #[error("contact not found: {0}")]
    ContactNotFound(String),

    /// Identity not initialized.
    #[error("identity not initialized")]
    IdentityNotInitialized,

    /// Already initialized.
    #[error("already initialized")]
    AlreadyInitialized,

    /// Invalid operation in current state.
    #[error("invalid state: {0}")]
    InvalidState(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Configuration(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Cryptographic operation failed.
    #[error("crypto error: {0}")]
    Crypto(String),

    /// Resource not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// Signature verification failed.
    #[error("invalid signature")]
    SignatureInvalid,

    /// Replay attack detected (duplicate nonce or stale timestamp).
    #[error("replay attack detected: duplicate nonce")]
    ReplayDetected,

    /// Contact is blocked.
    #[error("contact is blocked: {0}")]
    ContactBlocked(String),

    /// Contact limit reached.
    #[error("contact limit reached: maximum {0} contacts allowed")]
    ContactLimitReached(usize),
}

impl VauchiError {
    /// True when local storage cannot be read with the configured key:
    /// a field failed authenticated decryption, the database file is not
    /// readable as SQLite, or stored bytes fail to parse after decryption.
    /// Hosts branch on this — never on error text — to offer their native
    /// start-fresh recovery hint (ADR-045 Amendment 1).
    pub fn is_unreadable_storage(&self) -> bool {
        let Self::Storage(storage_err) = self else {
            return false;
        };
        match storage_err {
            StorageError::Encryption(_) | StorageError::InvalidData(_) => true,
            StorageError::Database(rusqlite::Error::SqliteFailure(ffi_err, _)) => {
                ffi_err.code == rusqlite::ErrorCode::NotADatabase
            }
            _ => false,
        }
    }
}

/// Result type for Vauchi operations.
pub type VauchiResult<T> = Result<T, VauchiError>;

// INLINE_TEST_REQUIRED: constructing rusqlite::Error values (SQLITE_NOTADB)
// is only possible inside the crate — rusqlite is a private dependency not
// visible to tests/it.
#[cfg(test)]
mod tests {
    use super::*;

    fn not_a_database_error() -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_NOTADB),
            Some("file is not a database".into()),
        )
    }

    // @internal
    #[test]
    fn field_decryption_failure_is_unreadable_storage() {
        let err = VauchiError::Storage(StorageError::Encryption(
            "Decryption failed: data may be corrupted or wrong key".into(),
        ));
        assert!(err.is_unreadable_storage());
    }

    // @internal
    #[test]
    fn not_a_database_file_is_unreadable_storage() {
        let err = VauchiError::Storage(StorageError::Database(not_a_database_error()));
        assert!(err.is_unreadable_storage());
    }

    // @internal
    #[test]
    fn corrupt_stored_bytes_are_unreadable_storage() {
        let err = VauchiError::Storage(StorageError::InvalidData("truncated row".into()));
        assert!(err.is_unreadable_storage());
    }

    // @internal
    #[test]
    fn other_errors_are_not_unreadable_storage() {
        let cases = [
            VauchiError::Storage(StorageError::DiskFull),
            VauchiError::Storage(StorageError::NotFound("key".into())),
            VauchiError::ContactNotFound("id".into()),
            VauchiError::Network(NetworkError::NotConnected),
            VauchiError::Crypto("exchange decrypt failed".into()),
        ];
        for err in cases {
            assert!(!err.is_unreadable_storage(), "false positive for {err:?}");
        }
    }
}
