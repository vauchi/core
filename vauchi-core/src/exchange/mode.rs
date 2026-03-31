// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange mode catalog.
//!
//! Defines the [`ExchangeMode`] enum (all 10 supported modes) along with
//! supporting type enums describing transport, bootstrap, proximity signals,
//! context, and device requirements.

use serde::{Deserialize, Serialize};

// ── Primary enum ────────────────────────────────────────────────────────────

/// All supported contact-exchange modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExchangeMode {
    /// Quick visual scan — both devices display and scan QR codes simultaneously.
    Glance,
    /// QR + ultrasonic audio proximity confirmation.
    Hover,
    /// BLE + physical impact detection.
    Bump,
    /// BLE + accelerometer shake gesture.
    Shake,
    /// BLE + ambient audio fingerprint match.
    Magic,
    /// BLE bootstrapped via NFC tap.
    TapTap,
    /// NFC + BLE + audio + accelerometer multi-factor in-person exchange.
    TapHoverShake,
    /// One-to-many QR broadcast for group exchanges.
    Broadcast,
    /// Remote exchange via QR scanned through a browser camera.
    Web,
    /// Async remote exchange via a shareable URL.
    Link,
}

impl ExchangeMode {
    /// Returns all ten variants in declaration order.
    pub fn all() -> [Self; 10] {
        [
            Self::Glance,
            Self::Hover,
            Self::Bump,
            Self::Shake,
            Self::Magic,
            Self::TapTap,
            Self::TapHoverShake,
            Self::Broadcast,
            Self::Web,
            Self::Link,
        ]
    }

    /// Human-readable display name for this mode.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Glance => "Glance",
            Self::Hover => "Hover",
            Self::Bump => "Bump",
            Self::Shake => "Shake",
            Self::Magic => "Magic",
            Self::TapTap => "Tap tap",
            Self::TapHoverShake => "Tap hover shake",
            Self::Broadcast => "Broadcast",
            Self::Web => "Web",
            Self::Link => "Link",
        }
    }

    /// Logical grouping used for UI organisation.
    pub fn category(self) -> ModeCategory {
        match self {
            Self::Glance | Self::Bump => ModeCategory::Quick,
            Self::Hover | Self::Magic | Self::Shake => ModeCategory::Standard,
            Self::TapTap | Self::TapHoverShake => ModeCategory::Fun,
            Self::Broadcast => ModeCategory::Group,
            Self::Web | Self::Link => ModeCategory::Remote,
        }
    }
}

// ── Supporting enums ────────────────────────────────────────────────────────

/// UI grouping for exchange modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeCategory {
    Quick,
    Standard,
    Fun,
    Group,
    Remote,
}

/// Primary data transport channel for a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataTransport {
    QrMultiStage,
    Ble,
    Relay,
}

/// How the two peers discover / bootstrap the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapMethod {
    QrMutualScan,
    QrOneToMany,
    QrRemoteCamera,
    BleDiscovery,
    NfcBootstrap,
    NfcAndBle,
    UrlShare,
}

/// Physical proximity signal used to verify co-presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProximityMethod {
    Audio,
    NfcRange,
    Accelerometer,
    Impact,
}

/// Whether the exchange requires both parties to be physically present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeContext {
    InPerson,
    Remote,
    RemoteAsync,
}

/// Device capability required to perform a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRequirement {
    Camera,
    Ble,
    Nfc,
    Microphone,
    Speaker,
    Accelerometer,
    Internet,
}
