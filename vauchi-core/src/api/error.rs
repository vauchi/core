// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! API Error Types
//!
//! Unified error type for the Vauchi API layer.

use thiserror::Error;

use crate::contact_card::ValidationError;
use crate::exchange::ExchangeError;
use crate::network::NetworkError;
use crate::storage::StorageError;
use crate::sync::device_sync::DeviceSyncError;
use crate::sync::SyncError;

/// Unified error type for Vauchi operations.
#[derive(Error, Debug)]
pub enum VauchiError {
    /// Contact card validation failed.
    #[error("validation error: {0}")]
    Validation(#[from] ValidationError),

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
}

/// Result type for Vauchi operations.
pub type VauchiResult<T> = Result<T, VauchiError>;
