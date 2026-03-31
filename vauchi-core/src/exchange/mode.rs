// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange mode catalog.
//!
//! Defines the [`ExchangeMode`] enum (all 10 supported modes) along with
//! supporting type enums and the static [`ModeConfig`] catalog that describes
//! transport, bootstrap, proximity signals, timeouts, and device requirements
//! for every mode.

use serde::{Deserialize, Serialize};
use std::time::Duration;

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
    pub fn all() -> &'static [Self] {
        &[
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

    /// Static configuration for this mode.
    pub fn config(self) -> &'static ModeConfig {
        match self {
            Self::Glance => &MODE_GLANCE,
            Self::Hover => &MODE_HOVER,
            Self::Bump => &MODE_BUMP,
            Self::Shake => &MODE_SHAKE,
            Self::Magic => &MODE_MAGIC,
            Self::TapTap => &MODE_TAP_TAP,
            Self::TapHoverShake => &MODE_TAP_HOVER_SHAKE,
            Self::Broadcast => &MODE_BROADCAST,
            Self::Web => &MODE_WEB,
            Self::Link => &MODE_LINK,
        }
    }
}

// ── Supporting enums ────────────────────────────────────────────────────────

/// UI grouping for mode selection screen.
///
/// Not serialized — used for display grouping only, not persisted.
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
#[non_exhaustive]
pub enum DataTransport {
    QrMultiStage,
    Ble,
    Relay,
}

/// How the two peers discover / bootstrap the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
#[non_exhaustive]
pub enum ProximityMethod {
    Audio,
    NfcRange,
    Accelerometer,
    Impact,
}

/// Whether the exchange requires both parties to be physically present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExchangeContext {
    InPerson,
    Remote,
    RemoteAsync,
}

/// Device capability required to perform a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeviceRequirement {
    Camera,
    Ble,
    Nfc,
    Microphone,
    Speaker,
    Accelerometer,
    Internet,
}

// ── ModeConfig ──────────────────────────────────────────────────────────────

/// Complete static configuration for one exchange mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeConfig {
    pub data_transport: DataTransport,
    pub bootstrap: BootstrapMethod,
    pub proximity: &'static [ProximityMethod],
    pub fallback_transport: Option<DataTransport>,
    pub context: ExchangeContext,
    pub timeout: Duration,
    pub requires: &'static [DeviceRequirement],
}

// ── Static catalog ──────────────────────────────────────────────────────────

static MODE_GLANCE: ModeConfig = ModeConfig {
    data_transport: DataTransport::QrMultiStage,
    bootstrap: BootstrapMethod::QrMutualScan,
    proximity: &[],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(60),
    requires: &[DeviceRequirement::Camera],
};

static MODE_HOVER: ModeConfig = ModeConfig {
    data_transport: DataTransport::QrMultiStage,
    bootstrap: BootstrapMethod::QrMutualScan,
    proximity: &[ProximityMethod::Audio],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(60),
    requires: &[
        DeviceRequirement::Camera,
        DeviceRequirement::Microphone,
        DeviceRequirement::Speaker,
    ],
};

static MODE_BUMP: ModeConfig = ModeConfig {
    data_transport: DataTransport::Ble,
    bootstrap: BootstrapMethod::BleDiscovery,
    proximity: &[ProximityMethod::Impact],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(30),
    requires: &[DeviceRequirement::Ble, DeviceRequirement::Accelerometer],
};

static MODE_SHAKE: ModeConfig = ModeConfig {
    data_transport: DataTransport::Ble,
    bootstrap: BootstrapMethod::BleDiscovery,
    proximity: &[ProximityMethod::Accelerometer],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(60),
    requires: &[DeviceRequirement::Ble, DeviceRequirement::Accelerometer],
};

static MODE_MAGIC: ModeConfig = ModeConfig {
    data_transport: DataTransport::Ble,
    bootstrap: BootstrapMethod::BleDiscovery,
    proximity: &[ProximityMethod::Audio],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(60),
    requires: &[
        DeviceRequirement::Ble,
        DeviceRequirement::Microphone,
        DeviceRequirement::Speaker,
    ],
};

static MODE_TAP_TAP: ModeConfig = ModeConfig {
    data_transport: DataTransport::Ble,
    bootstrap: BootstrapMethod::NfcBootstrap,
    proximity: &[ProximityMethod::NfcRange],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(30),
    requires: &[DeviceRequirement::Ble, DeviceRequirement::Nfc],
};

static MODE_TAP_HOVER_SHAKE: ModeConfig = ModeConfig {
    data_transport: DataTransport::Ble,
    bootstrap: BootstrapMethod::NfcAndBle,
    proximity: &[
        ProximityMethod::NfcRange,
        ProximityMethod::Audio,
        ProximityMethod::Accelerometer,
    ],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(90),
    requires: &[
        DeviceRequirement::Ble,
        DeviceRequirement::Nfc,
        DeviceRequirement::Microphone,
        DeviceRequirement::Speaker,
        DeviceRequirement::Accelerometer,
    ],
};

// Broadcast: BLE is the preferred data transport but relay serves
// as fallback — BLE is deliberately not in `requires` because the
// mode functions (degraded) without it. See spec footnote.
static MODE_BROADCAST: ModeConfig = ModeConfig {
    data_transport: DataTransport::Ble,
    bootstrap: BootstrapMethod::QrOneToMany,
    proximity: &[],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(600),
    requires: &[DeviceRequirement::Camera],
};

static MODE_WEB: ModeConfig = ModeConfig {
    data_transport: DataTransport::QrMultiStage,
    bootstrap: BootstrapMethod::QrRemoteCamera,
    proximity: &[],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::Remote,
    timeout: Duration::from_secs(300),
    requires: &[DeviceRequirement::Camera, DeviceRequirement::Internet],
};

static MODE_LINK: ModeConfig = ModeConfig {
    data_transport: DataTransport::Relay,
    bootstrap: BootstrapMethod::UrlShare,
    proximity: &[],
    fallback_transport: None,
    context: ExchangeContext::RemoteAsync,
    timeout: Duration::from_secs(7 * 24 * 60 * 60),
    requires: &[DeviceRequirement::Internet],
};

// ── Tests ───────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: static catalog constants (MODE_GLANCE…MODE_LINK) are not visible outside this module; tests verify exact field values against them
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_mode_has_ten_variants() {
        let all = ExchangeMode::all();
        assert_eq!(all.len(), 10);
        assert_eq!(all[0], ExchangeMode::Glance);
        assert_eq!(all[9], ExchangeMode::Link);
    }

    #[test]
    fn exchange_mode_serde_roundtrip() {
        for &mode in ExchangeMode::all() {
            let json = serde_json::to_string(&mode).expect("serialize");
            let back: ExchangeMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(mode, back);
        }
    }

    #[test]
    fn data_transport_has_three_variants() {
        let transports = [
            DataTransport::QrMultiStage,
            DataTransport::Ble,
            DataTransport::Relay,
        ];
        assert_eq!(transports.len(), 3);
        for t in transports {
            let json = serde_json::to_string(&t).expect("serialize");
            let back: DataTransport = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(t, back);
        }
    }

    #[test]
    fn exchange_context_serde_roundtrip() {
        let contexts = [
            ExchangeContext::InPerson,
            ExchangeContext::Remote,
            ExchangeContext::RemoteAsync,
        ];
        for ctx in contexts {
            let json = serde_json::to_string(&ctx).expect("serialize");
            let back: ExchangeContext = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(ctx, back);
        }
    }

    #[test]
    fn exchange_mode_display_names() {
        assert_eq!(ExchangeMode::Glance.display_name(), "Glance");
        assert_eq!(ExchangeMode::Hover.display_name(), "Hover");
        assert_eq!(ExchangeMode::Bump.display_name(), "Bump");
        assert_eq!(ExchangeMode::Shake.display_name(), "Shake");
        assert_eq!(ExchangeMode::Magic.display_name(), "Magic");
        assert_eq!(ExchangeMode::TapTap.display_name(), "Tap tap");
        assert_eq!(
            ExchangeMode::TapHoverShake.display_name(),
            "Tap hover shake"
        );
        assert_eq!(ExchangeMode::Broadcast.display_name(), "Broadcast");
        assert_eq!(ExchangeMode::Web.display_name(), "Web");
        assert_eq!(ExchangeMode::Link.display_name(), "Link");
    }

    #[test]
    fn exchange_mode_category() {
        assert_eq!(ExchangeMode::Glance.category(), ModeCategory::Quick);
        assert_eq!(ExchangeMode::Bump.category(), ModeCategory::Quick);
        assert_eq!(ExchangeMode::Hover.category(), ModeCategory::Standard);
        assert_eq!(ExchangeMode::Magic.category(), ModeCategory::Standard);
        assert_eq!(ExchangeMode::Shake.category(), ModeCategory::Standard);
        assert_eq!(ExchangeMode::TapTap.category(), ModeCategory::Fun);
        assert_eq!(ExchangeMode::TapHoverShake.category(), ModeCategory::Fun);
        assert_eq!(ExchangeMode::Broadcast.category(), ModeCategory::Group);
        assert_eq!(ExchangeMode::Web.category(), ModeCategory::Remote);
        assert_eq!(ExchangeMode::Link.category(), ModeCategory::Remote);
    }

    #[test]
    fn every_mode_has_config() {
        for &mode in ExchangeMode::all() {
            let cfg = mode.config();
            assert!(cfg.timeout > Duration::ZERO, "{:?} has zero timeout", mode);
        }
    }

    #[test]
    fn hover_config_uses_qr_staged_with_audio_proximity() {
        let cfg = ExchangeMode::Hover.config();
        assert_eq!(cfg.data_transport, DataTransport::QrMultiStage);
        assert_eq!(cfg.bootstrap, BootstrapMethod::QrMutualScan);
        assert_eq!(cfg.proximity, &[ProximityMethod::Audio]);
        assert_eq!(cfg.fallback_transport, Some(DataTransport::Relay));
        assert_eq!(cfg.context, ExchangeContext::InPerson);
        assert_eq!(cfg.timeout, Duration::from_secs(60));
    }

    #[test]
    fn link_config_uses_relay_with_no_fallback() {
        let cfg = ExchangeMode::Link.config();
        assert_eq!(cfg.data_transport, DataTransport::Relay);
        assert_eq!(cfg.bootstrap, BootstrapMethod::UrlShare);
        assert_eq!(cfg.fallback_transport, None);
        assert_eq!(cfg.context, ExchangeContext::RemoteAsync);
        assert_eq!(cfg.timeout, Duration::from_secs(7 * 24 * 60 * 60));
    }

    #[test]
    fn tap_hover_shake_has_three_proximity_methods() {
        let cfg = ExchangeMode::TapHoverShake.config();
        assert_eq!(cfg.proximity.len(), 3);
        assert!(cfg.proximity.contains(&ProximityMethod::NfcRange));
        assert!(cfg.proximity.contains(&ProximityMethod::Audio));
        assert!(cfg.proximity.contains(&ProximityMethod::Accelerometer));
    }

    #[test]
    fn broadcast_config_has_long_timeout() {
        let cfg = ExchangeMode::Broadcast.config();
        assert_eq!(cfg.timeout, Duration::from_secs(600));
        assert!(cfg.timeout > Duration::from_secs(300));
    }

    #[test]
    fn all_in_person_modes_have_relay_fallback() {
        for &mode in ExchangeMode::all() {
            let cfg = mode.config();
            if cfg.context == ExchangeContext::InPerson {
                assert_eq!(
                    cfg.fallback_transport,
                    Some(DataTransport::Relay),
                    "{:?} is InPerson but has no Relay fallback",
                    mode
                );
            }
        }
    }

    #[test]
    fn mode_requirements_match_config() {
        let hover = ExchangeMode::Hover.config();
        assert!(hover.requires.contains(&DeviceRequirement::Camera));
        assert!(hover.requires.contains(&DeviceRequirement::Microphone));
        assert!(hover.requires.contains(&DeviceRequirement::Speaker));

        let link = ExchangeMode::Link.config();
        assert!(link.requires.contains(&DeviceRequirement::Internet));
        assert!(!link.requires.contains(&DeviceRequirement::Camera));
    }
}
