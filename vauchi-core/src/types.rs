// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared domain types used across multiple modules.
//!
//! These types are used by exchange, contact, capability, and storage modules.
//! Placing them here avoids circular dependencies and prepares for future crate
//! extraction (vauchi-types).

/// Transport method used for contact exchange.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ExchangeTransport {
    /// QR exchange: both sides display and scan QR codes.
    /// Both use fresh ephemeral X25519 keys for full forward secrecy.
    #[default]
    Qr,
    /// NFC Active (phone-to-phone tap): single tap replaces scan + proximity.
    /// Fresh ephemeral X25519 keys on both sides.
    Nfc,
    /// BLE exchange: GATT-based payload exchange with proximity verification.
    /// Fresh ephemeral X25519 keys on both sides.
    Ble,
}

/// Confidence level of physical proximity during contact exchange.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProximityConfidence {
    /// High confidence: verified by ultrasonic audio or NFC tap.
    High,
    /// Medium confidence: manual user confirmation.
    Medium,
    /// Low confidence: proximity check failed or timed out.
    Low,
    /// Unknown: no proximity check was performed (legacy contacts).
    #[default]
    Unknown,
}

/// Represents device audio capabilities.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AudioCapability {
    /// Device supports full ultrasonic audio (speaker + microphone)
    Full,
    /// Device can only emit ultrasonic audio (no microphone)
    EmitOnly,
    /// Device can only receive ultrasonic audio (no speaker)
    ReceiveOnly,
    /// Device does not support ultrasonic audio
    #[default]
    None,
}
