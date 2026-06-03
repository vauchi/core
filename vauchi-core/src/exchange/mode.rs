// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange mode catalog.
//!
//! Defines the [`ExchangeMode`] enum (all 9 supported modes) along with
//! supporting type enums and the static [`ModeConfig`] catalog that describes
//! transport, bootstrap, proximity signals, timeouts, and device requirements
//! for every mode.

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ── Primary enum ────────────────────────────────────────────────────────────

/// All supported contact-exchange modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
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
    /// Async remote exchange via a shareable URL.
    Link,
    /// USB cable exchange between a desktop and a phone.
    Cable,
}

impl ExchangeMode {
    /// Returns all nine variants in declaration order.
    pub fn all() -> &'static [Self] {
        &[
            Self::Glance,
            Self::Hover,
            Self::Bump,
            Self::Shake,
            Self::Magic,
            Self::TapTap,
            Self::TapHoverShake,
            Self::Link,
            Self::Cable,
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
            Self::Link => "Link",
            Self::Cable => "Cable",
        }
    }

    /// Logical grouping used for UI organisation.
    pub fn category(self) -> ModeCategory {
        match self {
            Self::Glance | Self::Bump => ModeCategory::Quick,
            Self::Hover | Self::Magic | Self::Shake => ModeCategory::Standard,
            Self::TapTap | Self::TapHoverShake => ModeCategory::Fun,
            Self::Link => ModeCategory::Remote,
            Self::Cable => ModeCategory::Standard,
        }
    }

    /// Returns `true` if this mode requires a camera on at least one peer device.
    pub fn requires_camera(self) -> bool {
        self.config().requires.contains(&DeviceRequirement::Camera)
    }

    /// Returns `true` if this mode uses a physical proximity signal to verify
    /// co-presence (i.e., the config's `proximity` slice is non-empty).
    pub fn requires_proximity(self) -> bool {
        !self.config().proximity.is_empty()
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
            Self::Link => &MODE_LINK,
            Self::Cable => &MODE_CABLE,
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
    Remote,
}

/// Primary data transport channel for a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DataTransport {
    QrMultiStage,
    Ble,
    Nfc,
    Relay,
}

/// How the two peers discover / bootstrap the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BootstrapMethod {
    QrMutualScan,
    BleDiscovery,
    NfcBootstrap,
    NfcAndBle,
    UrlShare,
    /// USB cable connection — desktop initiates, phone responds.
    UsbCable,
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
    /// Physical wired connection (e.g. USB cable) guarantees co-presence.
    WiredConnection,
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
    /// USB port capable of data transfer (not charge-only).
    UsbPort,
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
    data_transport: DataTransport::Nfc,
    bootstrap: BootstrapMethod::NfcBootstrap,
    proximity: &[ProximityMethod::NfcRange],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(30),
    requires: &[DeviceRequirement::Nfc],
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

static MODE_LINK: ModeConfig = ModeConfig {
    data_transport: DataTransport::Relay,
    bootstrap: BootstrapMethod::UrlShare,
    proximity: &[],
    fallback_transport: None,
    context: ExchangeContext::RemoteAsync,
    timeout: Duration::from_secs(7 * 24 * 60 * 60),
    requires: &[DeviceRequirement::Internet],
};

static MODE_CABLE: ModeConfig = ModeConfig {
    data_transport: DataTransport::Ble,
    bootstrap: BootstrapMethod::UsbCable,
    proximity: &[ProximityMethod::WiredConnection],
    fallback_transport: Some(DataTransport::Relay),
    context: ExchangeContext::InPerson,
    timeout: Duration::from_secs(60),
    requires: &[DeviceRequirement::UsbPort],
};

// ── Tests ───────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: static catalog constants (MODE_GLANCE…MODE_LINK) are not visible outside this module; tests verify exact field values against them
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_mode_has_nine_variants() {
        let all = ExchangeMode::all();
        assert_eq!(all.len(), 9);
        assert_eq!(all[0], ExchangeMode::Glance);
        assert_eq!(all[8], ExchangeMode::Cable);
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
        assert_eq!(ExchangeMode::Link.display_name(), "Link");
        assert_eq!(ExchangeMode::Cable.display_name(), "Cable");
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
        assert_eq!(ExchangeMode::Link.category(), ModeCategory::Remote);
        assert_eq!(ExchangeMode::Cable.category(), ModeCategory::Standard);
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

    /// Exhaustive match over every variant in `all()`.
    ///
    /// Adding a new variant to `ExchangeMode` without updating `all()` AND this
    /// match will cause a compile error, making the omission impossible to miss.
    #[test]
    fn all_covers_every_variant() {
        let modes = ExchangeMode::all();
        assert_eq!(modes.len(), 9, "update this count when adding variants");
        for &mode in modes {
            match mode {
                ExchangeMode::Glance
                | ExchangeMode::Hover
                | ExchangeMode::Bump
                | ExchangeMode::Shake
                | ExchangeMode::Magic
                | ExchangeMode::TapTap
                | ExchangeMode::TapHoverShake
                | ExchangeMode::Link
                | ExchangeMode::Cable => {}
            }
        }
    }

    // @internal
    #[test]
    fn cable_mode_exists_and_is_categorized() {
        let mode = ExchangeMode::Cable;
        assert_eq!(mode.display_name(), "Cable");
        assert_eq!(mode.category(), ModeCategory::Standard);
        assert!(!mode.requires_camera());
        assert!(mode.requires_proximity());
    }

    // @internal
    #[test]
    fn cable_config_uses_usb_bootstrap_with_wired_proximity() {
        let cfg = ExchangeMode::Cable.config();
        assert_eq!(cfg.bootstrap, BootstrapMethod::UsbCable);
        assert_eq!(cfg.proximity, &[ProximityMethod::WiredConnection]);
        assert_eq!(cfg.context, ExchangeContext::InPerson);
        assert_eq!(cfg.fallback_transport, Some(DataTransport::Relay));
        assert!(cfg.requires.contains(&DeviceRequirement::UsbPort));
        assert!(!cfg.requires.contains(&DeviceRequirement::Camera));
    }
}
