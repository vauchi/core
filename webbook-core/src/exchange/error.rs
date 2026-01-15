//! Exchange Error Types

use thiserror::Error;

/// Errors that can occur during contact exchange.
#[derive(Error, Debug)]
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
}
