// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mobile-friendly error types (ADR-044).
//!
//! `MobileError` is a flat struct-variant enum whose variants match the UI
//! branches frontends actually take. Each variant either stands alone
//! (self-describing — e.g. `WrongPassword`) or carries the minimum fields
//! needed for its UI decision. Pattern-match on the variant, not on the
//! message.
//!
//! See `_private/docs/decisions/2026-04-20-adr-044-mobile-error-typing.md`
//! for rationale and the list of rejected designs.

/// Error type for platform keychain callback interface.
///
/// UniFFI requires a named error enum for callback interfaces (String is not supported).
/// Mobile platforms return this from keychain operations.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum KeychainError {
    #[error("{msg}")]
    OperationFailed { msg: String },
}

/// Mobile-friendly error type surfaced across the UniFFI boundary.
///
/// Marked `#[non_exhaustive]`: adding a variant is non-breaking, but
/// Swift/Kotlin consumers must include a default arm.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[non_exhaustive]
pub enum MobileError {
    /// Supplied password/PIN did not match the stored credential.
    /// Frontends show an inline "Incorrect password" hint and keep the
    /// dialog open.
    #[error("Incorrect password")]
    WrongPassword,

    /// Decryption produced an authentication-tag failure. Typically
    /// irrecoverable — the file/blob is either corrupt or was encrypted
    /// with a different key. Disable retry.
    #[error("Decryption failed")]
    DecryptFailed,

    /// Input failed a validation rule owned by core. `field` is a stable
    /// id (or empty when not applicable). `message` is English and is for
    /// logs / the escape-hatch display path only — frontends localize
    /// via their own `t()` keyed by variant name.
    #[error("Invalid input on '{field}': {message}")]
    InvalidInput { field: String, message: String },

    /// Transient network failure (unreachable relay, DNS, timeout).
    /// Frontends show a "Check your connection" banner and enable retry.
    #[error("Network unavailable")]
    NetworkUnavailable,

    /// Non-transient network failure reported by the relay (4xx/5xx).
    #[error("Relay error {status}: {message}")]
    RelayError { status: u16, message: String },

    /// Relay rate-limit in effect. Frontends show a cooldown timer and
    /// schedule automatic retry after `retry_after_secs`.
    #[error("Rate limited — retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    /// Local storage failure (SQLite, disk full, permission denied,
    /// corrupt keychain).
    #[error("Storage error: {message}")]
    StorageError { message: String },

    /// Escape hatch. Frontends display `message` verbatim and log the
    /// full error. Promote recurring uses of `Other` to a dedicated
    /// variant in a follow-up ADR amendment when a real UI branch
    /// appears.
    #[error("{message}")]
    Other { message: String },
}

impl MobileError {
    /// Convenience constructor for `Other { message }`. Used by call
    /// sites that previously built `MobileError::Internal(...)`,
    /// `MobileError::ExchangeFailed(...)`, etc. — all of which collapse
    /// into the `Other` escape hatch per ADR-044.
    #[inline]
    pub fn other(message: impl Into<String>) -> Self {
        MobileError::Other {
            message: message.into(),
        }
    }

    /// Convenience constructor for `InvalidInput { field: "", message }`.
    /// Call sites that know the field id pass it directly; the rest use
    /// this shim so the migration stays mechanical.
    #[inline]
    pub fn invalid_input(message: impl Into<String>) -> Self {
        MobileError::InvalidInput {
            field: String::new(),
            message: message.into(),
        }
    }

    /// Convenience constructor for `StorageError { message }`.
    #[inline]
    pub fn storage(message: impl Into<String>) -> Self {
        MobileError::StorageError {
            message: message.into(),
        }
    }
}

/// Acquire a mutex lock, converting poison errors to `MobileError::Other`.
pub(crate) fn lock_or<T>(
    mutex: &std::sync::Mutex<T>,
) -> Result<std::sync::MutexGuard<'_, T>, MobileError> {
    mutex
        .lock()
        .map_err(|_| MobileError::other(LOCK_POISON_MSG))
}

/// Consistent message for lock-poison errors.
pub(crate) const LOCK_POISON_MSG: &str = "lock poisoned";

impl From<vauchi_core::network::NetworkError> for MobileError {
    fn from(err: vauchi_core::network::NetworkError) -> Self {
        match err {
            vauchi_core::network::NetworkError::RateLimited { retry_after_secs } => {
                MobileError::RateLimited { retry_after_secs }
            }
            other => MobileError::other(other.to_string()),
        }
    }
}

impl From<vauchi_core::StorageError> for MobileError {
    fn from(err: vauchi_core::StorageError) -> Self {
        // Use user_message() to strip internal details (F9 audit fix)
        MobileError::storage(err.user_message().to_string())
    }
}

impl From<vauchi_core::VauchiError> for MobileError {
    fn from(err: vauchi_core::VauchiError) -> Self {
        match err {
            vauchi_core::VauchiError::Storage(e) => {
                MobileError::storage(e.user_message().to_string())
            }
            other => MobileError::other(other.to_string()),
        }
    }
}
