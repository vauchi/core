// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Error Types

use crate::crypto::DhError;
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

    // NFC errors
    #[error("Invalid NFC payload format")]
    InvalidNfcFormat,

    #[error("NFC payload has expired")]
    NfcExpired,

    #[error("NFC session lost during exchange")]
    NfcSessionLost,

    #[error("NFC not supported on this device")]
    NfcNotSupported,

    #[error("NFC APDU chain reassembly failed")]
    NfcChainReassemblyFailed,

    #[error("NFC decryption failed: authentication tag verification failure")]
    NfcDecryptionFailed,

    #[error("NFC CRC16 mismatch after decryption")]
    NfcCrcMismatch,

    // BLE errors
    #[error("Invalid BLE payload format")]
    InvalidBleFormat,

    #[error("BLE payload has expired")]
    BleExpired,

    #[error("BLE device out of range")]
    BleOutOfRange,

    #[error("BLE connection lost during exchange")]
    BleConnectionLost,

    #[error("BLE not available on this device")]
    BleNotAvailable,

    /// BLE encrypted payload failed AEAD authentication (tampered or wrong key)
    #[error("BLE decryption failed")]
    BleDecryptionFailed,

    /// BLE key exchange handshake did not complete
    #[error("BLE handshake failed: {0}")]
    BleHandshakeFailed(String),

    /// BLE ECDH or HKDF key derivation error
    #[error("BLE key derivation failed")]
    BleKeyDerivationFailed,

    /// SHA-256 commitment does not match received encrypted blob
    #[error("BLE commitment mismatch")]
    BleCommitmentMismatch,

    /// Negotiated BLE MTU is below the minimum viable threshold
    #[error("BLE MTU too small")]
    BleMtuTooSmall,

    #[error("Proximity verification required before device linking")]
    ProximityNotVerified,

    #[error("Proximity proof expired (older than 60 seconds)")]
    ProximityExpired,

    #[error("Self-linking not allowed: device name already exists in registry")]
    SelfLinkingNotAllowed,

    #[error("BLE chunk reassembly failed: {0}")]
    BleChunkReassemblyFailed(String),

    #[error("Display name too long ({0} bytes, max 255)")]
    DisplayNameTooLong(usize),

    #[error("BLE reassembly limit exceeded: {0}")]
    BleReassemblyLimitExceeded(String),

    #[error("Non-contributory DH output: {0}")]
    InvalidDhOutput(#[from] DhError),
}
