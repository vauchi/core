// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Error Types

use crate::crypto::DhError;
use thiserror::Error;

/// Errors that can occur during contact exchange.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
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

    #[error("OOB nonce echo missing or mismatched")]
    OobNonceMismatch,

    #[error("Exchange key mismatch: wire key does not match the OOB-pinned key")]
    ExchangeKeyMismatch,

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

    /// USB/direct-transport encrypted card failed AEAD authentication
    /// (tampered ciphertext or wrong shared key)
    #[error("USB card decryption failed")]
    UsbDecryptionFailed,

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

    #[error("Hardware failure ({transport}): {error}")]
    HardwareFailure { transport: String, error: String },
}

impl ExchangeError {
    /// User-friendly message for display in the exchange failure screen.
    /// Avoids exposing internal details while giving actionable guidance.
    pub fn user_message(&self) -> &str {
        match self {
            Self::QRExpired | Self::TokenExpired | Self::DeviceLinkQRExpired => {
                "The code has expired. Please try again with a fresh code."
            }
            Self::SessionTimeout => "The exchange timed out. Please try again.",
            Self::NfcExpired | Self::BleExpired => "The connection timed out. Please try again.",
            Self::ProximityExpired => {
                "Proximity verification expired. Please move closer and try again."
            }

            Self::InvalidQRFormat | Self::QRAlreadyUsed => {
                "The QR code could not be read. Please ask the other person to show a new one."
            }

            Self::ProximityFailed | Self::ProximityNotVerified => {
                "Could not verify you are nearby. Move closer to the other person and try again."
            }
            Self::NumericCodeMismatch => {
                "The verification codes did not match. This might be a different person's code."
            }
            Self::FingerprintRequired => {
                "Fingerprint verification is required. Please verify the fingerprint shown on both devices."
            }

            Self::DuplicateContact => "This contact already exists in your list.",
            Self::SelfExchange => "You cannot exchange with yourself.",
            Self::SelfLinkingNotAllowed => "This device is already linked to your identity.",
            Self::ContactBlocked => "This contact is blocked. Unblock them first to exchange.",

            Self::ConsentDenied => "The other person declined the exchange.",
            Self::Interrupted => "The exchange was interrupted. Please try again.",

            Self::NetworkDisconnected => {
                "Connection lost during exchange. Check your internet and try again."
            }

            Self::LowBattery => "Battery too low. Please charge your device and try again.",
            Self::InsufficientStorage => "Not enough storage space. Free up space and try again.",

            Self::ClockDrift(_) => {
                "Your device clock appears to be incorrect. Please check your date and time settings."
            }

            Self::BleOutOfRange => "The other device is too far away. Move closer and try again.",
            Self::BleConnectionLost | Self::NfcSessionLost => {
                "Connection lost. Stay close to the other device and try again."
            }
            Self::BleNotAvailable => "Bluetooth is not available. Try using QR code instead.",
            Self::NfcNotSupported => {
                "NFC is not available on this device. Try using QR code instead."
            }
            Self::BleMtuTooSmall => "Bluetooth connection is unstable. Try using QR code instead.",

            Self::HardwareFailure { transport, .. } => match transport.as_str() {
                "BLE" => "Bluetooth encountered an error. Try using QR code instead.",
                "NFC" => "NFC encountered an error. Try using QR code instead.",
                _ => "A hardware error occurred. Please try a different exchange method.",
            },

            Self::InvalidSignature
            | Self::IdentityMismatch
            | Self::OobNonceMismatch
            | Self::ExchangeKeyMismatch
            | Self::KeyAgreementFailed(_)
            | Self::InvalidProtocolVersion
            | Self::InvalidState(_)
            | Self::CryptoError
            | Self::SerializationFailed
            | Self::StalePrekey
            | Self::InvalidNfcFormat
            | Self::NfcChainReassemblyFailed
            | Self::NfcDecryptionFailed
            | Self::NfcCrcMismatch
            | Self::InvalidBleFormat
            | Self::BleDecryptionFailed
            | Self::UsbDecryptionFailed
            | Self::BleHandshakeFailed(_)
            | Self::BleKeyDerivationFailed
            | Self::BleCommitmentMismatch
            | Self::BleChunkReassemblyFailed(_)
            | Self::BleReassemblyLimitExceeded(_)
            | Self::DisplayNameTooLong(_)
            | Self::InvalidDhOutput(_) => {
                "Something went wrong during the exchange. Please try again."
            }
        }
    }
}
