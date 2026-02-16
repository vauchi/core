// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mobile-friendly error types.

/// Error type for platform keychain callback interface.
///
/// UniFFI requires a named error enum for callback interfaces (String is not supported).
/// Mobile platforms return this from keychain operations.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum KeychainError {
    #[error("{msg}")]
    OperationFailed { msg: String },
}

/// Mobile-friendly error type.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    #[error("Library not initialized")]
    NotInitialized,

    #[error("Already initialized")]
    AlreadyInitialized,

    #[error("Identity not found")]
    IdentityNotFound,

    #[error("Contact not found: {0}")]
    ContactNotFound(String),

    #[error("Invalid QR code")]
    InvalidQrCode,

    #[error("Exchange failed: {0}")]
    ExchangeFailed(String),

    #[error("Sync failed: {0}")]
    SyncFailed(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Crypto error: {0}")]
    CryptoError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("GDPR error: {0}")]
    GdprError(String),

    #[error("Deletion not allowed: {0}")]
    DeletionNotAllowed(String),

    #[error("Shred error: {0}")]
    ShredError(String),

    #[error("Init error: {0}")]
    InitError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<vauchi_core::StorageError> for MobileError {
    fn from(err: vauchi_core::StorageError) -> Self {
        MobileError::StorageError(err.to_string())
    }
}

impl From<vauchi_core::VauchiError> for MobileError {
    fn from(err: vauchi_core::VauchiError) -> Self {
        match err {
            vauchi_core::VauchiError::ContactNotFound(id) => MobileError::ContactNotFound(id),
            vauchi_core::VauchiError::Storage(e) => MobileError::StorageError(e.to_string()),
            other => MobileError::Internal(other.to_string()),
        }
    }
}
