// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange session metadata enums (transport, proximity, audio, origin).
//!
//! A neutral leaf module: shared by `exchange`, `contact`, `storage`, and
//! `api` without depending on any of them, so these enums never pull the
//! heavy `exchange` module into a lightweight consumer.

/// Where an event originated — local device or synced from another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum EventOrigin {
    /// Event happened on this device.
    Local,
    /// Event arrived via sync from another device.
    Synced,
}

/// Transport method used for contact exchange.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExchangeTransport {
    /// QR exchange: both sides display and scan QR codes.
    /// Both use fresh ephemeral X25519 keys for full forward secrecy.
    #[default]
    #[serde(alias = "Qr")]
    Qr,
    /// NFC Active (phone-to-phone tap): single tap replaces scan + proximity.
    /// Fresh ephemeral X25519 keys on both sides.
    #[serde(alias = "Nfc")]
    Nfc,
    /// BLE exchange: GATT-based payload exchange with proximity verification.
    /// Fresh ephemeral X25519 keys on both sides.
    #[serde(alias = "Ble")]
    Ble,
    /// USB cable exchange: TCP over physical cable connection.
    Usb,
    /// Audio data channel exchange: ultrasonic or audible payload transfer.
    Audio,
    /// Multi-stage QR family (Hover / Glance screen ritual): animated
    /// mutual-QR bootstrap. Reader support ships ahead of any writer
    /// (consolidation Step 2e) — decoding is fail-closed on unknown
    /// variants, so every released reader must know this name before
    /// the first persist path stamps it. No writer exists yet.
    MultiStage,
    /// Asynchronous link-mode exchange: relay-mediated, initiated by sharing
    /// a `vauchi://exchange?pk=…&n=…` URL. Both sides write `Link` at finalize
    /// time — labels the exchange semantics (asynchronous, relay-mediated),
    /// not the URL's delivery channel (SMS / email / messenger — unobservable).
    /// Problem record: `2026-04-27-deep-link-responder-flow`.
    Link,
}

/// Confidence level of physical proximity during contact exchange.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
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
#[non_exhaustive]
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
