// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Error Types

use thiserror::Error;

/// Errors that can occur during contact exchange.
#[derive(Error, Debug, Clone)]
pub enum ExchangeError {
    #[error("QR code has expired")]
    QRExpired,

    #[error("Invalid QR code format")]
    InvalidQRFormat,

    #[error("Invalid QR signature")]
    InvalidSignature,

    #[error("Proximity verification failed")]
    ProximityFailed,

    #[error("Key agreement failed: {0}")]
    KeyAgreementFailed(String),

    #[error("Exchange session timed out")]
    SessionTimeout,

    #[error("Exchange was interrupted")]
    Interrupted,

    #[error("Contact already exists")]
    DuplicateContact,

    #[error("Invalid protocol version")]
    InvalidProtocolVersion,

    #[error("Invalid session state: {0}")]
    InvalidState(String),

    #[error("Token has expired")]
    TokenExpired,

    #[error("Cryptographic operation failed")]
    CryptoError,

    #[error("Serialization failed")]
    SerializationFailed,

    #[error("Cannot exchange with yourself")]
    SelfExchange,

    #[error("QR code already used")]
    QRAlreadyUsed,

    #[error("Network disconnected during exchange")]
    NetworkDisconnected,

    #[error("Contact is blocked")]
    ContactBlocked,

    #[error("Exchange consent denied by other party")]
    ConsentDenied,

    #[error("Identity mismatch: signing key does not match QR public key")]
    IdentityMismatch,

    #[error("Stale prekey, retrying")]
    StalePrekey,

    #[error("Clock drift detected: {0}s")]
    ClockDrift(i64),

    #[error("Device link QR has expired")]
    DeviceLinkQRExpired,

    #[error("Low battery: exchange may fail")]
    LowBattery,

    #[error("Insufficient storage space")]
    InsufficientStorage,

    #[error("Numeric code mismatch")]
    NumericCodeMismatch,

    #[error("Fingerprint verification required")]
    FingerprintRequired,
}
