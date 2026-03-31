// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mode availability checking and recommendation.
//!
//! Determines which [`ExchangeMode`]s are usable on a given device by mapping
//! each [`DeviceRequirement`] to a field of [`DeviceCapabilities`], then
//! recommends the most capable available mode.

use crate::exchange::capability::types::DeviceCapabilities;
use crate::exchange::mode::{DeviceRequirement, ExchangeMode};
use crate::types::AudioCapability;

// ── Availability result ──────────────────────────────────────────────────────

/// Availability status of an [`ExchangeMode`] on a specific device.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModeAvailability {
    /// All required hardware is present and fully functional.
    Available,
    /// Mode can run in a reduced capacity (reserved for future degradation logic).
    Degraded { reason: String },
    /// One or more required hardware capabilities are absent.
    Unavailable { reason: String },
}

// ── Public functions ─────────────────────────────────────────────────────────

/// Check whether `mode` can run on `caps`.
///
/// Each [`DeviceRequirement`] in `mode.config().requires` is evaluated against
/// the corresponding `caps` field. If any requirement is unmet the mode is
/// `Unavailable` with a comma-separated list of missing hardware names.
/// Otherwise `Available` is returned.
pub fn check_mode_availability(mode: ExchangeMode, caps: &DeviceCapabilities) -> ModeAvailability {
    let mut missing: Vec<&'static str> = Vec::new();

    for req in mode.config().requires {
        let satisfied = match req {
            DeviceRequirement::Camera => caps.has_camera,
            DeviceRequirement::Ble => caps.has_ble,
            DeviceRequirement::Nfc => caps.has_nfc,
            DeviceRequirement::Microphone => matches!(
                caps.audio,
                AudioCapability::Full | AudioCapability::ReceiveOnly
            ),
            DeviceRequirement::Speaker => matches!(
                caps.audio,
                AudioCapability::Full | AudioCapability::EmitOnly
            ),
            DeviceRequirement::Accelerometer => caps.has_accelerometer,
            DeviceRequirement::Internet => caps.has_internet,
        };

        if !satisfied {
            missing.push(requirement_name(req));
        }
    }

    if missing.is_empty() {
        ModeAvailability::Available
    } else {
        ModeAvailability::Unavailable {
            reason: format!("Requires {}", missing.join(", ")),
        }
    }
}

/// Recommend the best [`ExchangeMode`] for `caps`.
///
/// Modes are tried in priority order: Hover, Magic, Shake, TapHoverShake,
/// TapTap, Bump, Glance, Broadcast, Web, Link. The first `Available` mode is
/// returned. Link is always available when `has_internet` is true; if nothing
/// else works, Link is returned as the final fallback regardless.
pub fn recommend_mode(caps: &DeviceCapabilities) -> ExchangeMode {
    const PRIORITY: &[ExchangeMode] = &[
        ExchangeMode::Hover,
        ExchangeMode::Magic,
        ExchangeMode::Shake,
        ExchangeMode::TapHoverShake,
        ExchangeMode::TapTap,
        ExchangeMode::Bump,
        ExchangeMode::Glance,
        ExchangeMode::Broadcast,
        ExchangeMode::Web,
        ExchangeMode::Link,
    ];

    for &mode in PRIORITY {
        if check_mode_availability(mode, caps) == ModeAvailability::Available {
            return mode;
        }
    }

    // Final fallback — Link only needs internet, and even without internet it
    // is the least-hardware mode.
    ExchangeMode::Link
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn requirement_name(req: &DeviceRequirement) -> &'static str {
    match req {
        DeviceRequirement::Camera => "camera",
        DeviceRequirement::Ble => "BLE",
        DeviceRequirement::Nfc => "NFC",
        DeviceRequirement::Microphone => "microphone",
        DeviceRequirement::Speaker => "speaker",
        DeviceRequirement::Accelerometer => "accelerometer",
        DeviceRequirement::Internet => "internet",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: tests exercise check_mode_availability and recommend_mode against specific
// DeviceCapabilities configurations that cannot be tested from outside this module without
// duplicating the requirement-to-capability mapping logic.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::capability::types::DeviceCapabilities;
    use crate::types::AudioCapability;

    fn full_caps() -> DeviceCapabilities {
        DeviceCapabilities {
            has_camera: true,
            has_ble: true,
            has_nfc: true,
            audio: AudioCapability::Full,
            has_accelerometer: true,
            has_internet: true,
            ..Default::default()
        }
    }

    #[test]
    fn full_device_all_modes_available() {
        let caps = full_caps();
        for &mode in ExchangeMode::all() {
            assert_eq!(
                check_mode_availability(mode, &caps),
                ModeAvailability::Available,
                "{:?} should be Available on a full-capability device",
                mode
            );
        }
    }

    #[test]
    fn no_nfc_makes_tap_modes_unavailable() {
        let caps = DeviceCapabilities {
            has_nfc: false,
            ..full_caps()
        };
        assert_eq!(
            check_mode_availability(ExchangeMode::TapTap, &caps),
            ModeAvailability::Unavailable {
                reason: "Requires NFC".to_string()
            }
        );
        assert!(matches!(
            check_mode_availability(ExchangeMode::TapHoverShake, &caps),
            ModeAvailability::Unavailable { .. }
        ));
    }

    #[test]
    fn desktop_only_camera_internet_modes() {
        // Desktop: camera + internet, no BLE / NFC / accelerometer, no audio
        let caps = DeviceCapabilities {
            has_camera: true,
            has_internet: true,
            ..Default::default()
        };
        assert_eq!(
            check_mode_availability(ExchangeMode::Glance, &caps),
            ModeAvailability::Available,
            "Glance needs only camera"
        );
        assert_eq!(
            check_mode_availability(ExchangeMode::Web, &caps),
            ModeAvailability::Available,
            "Web needs camera + internet"
        );
        assert_eq!(
            check_mode_availability(ExchangeMode::Link, &caps),
            ModeAvailability::Available,
            "Link needs only internet"
        );
        assert!(
            matches!(
                check_mode_availability(ExchangeMode::Bump, &caps),
                ModeAvailability::Unavailable { .. }
            ),
            "Bump requires BLE"
        );
        assert!(
            matches!(
                check_mode_availability(ExchangeMode::Hover, &caps),
                ModeAvailability::Unavailable { .. }
            ),
            "Hover requires audio"
        );
    }

    #[test]
    fn unavailable_reason_names_missing_hardware() {
        let caps = DeviceCapabilities {
            has_camera: false,
            ..Default::default()
        };
        let avail = check_mode_availability(ExchangeMode::Glance, &caps);
        match avail {
            ModeAvailability::Unavailable { reason } => {
                assert!(
                    reason.contains("camera"),
                    "reason should mention 'camera', got: {}",
                    reason
                );
            }
            other => panic!("expected Unavailable, got {:?}", other),
        }
    }

    #[test]
    fn recommend_picks_hover_for_full_device() {
        let caps = full_caps();
        assert_eq!(recommend_mode(&caps), ExchangeMode::Hover);
    }

    #[test]
    fn recommend_picks_glance_for_camera_only() {
        // Desktop: camera + internet only — Hover/Magic/Shake all need audio or BLE
        let caps = DeviceCapabilities {
            has_camera: true,
            has_internet: true,
            ..Default::default()
        };
        // Glance only needs camera, comes before Web in priority (after BLE modes)
        assert_eq!(recommend_mode(&caps), ExchangeMode::Glance);
    }

    #[test]
    fn recommend_picks_link_for_no_hardware() {
        // Internet only — no camera, no BLE, no NFC, no audio, no accelerometer
        let caps = DeviceCapabilities {
            has_internet: true,
            ..Default::default()
        };
        assert_eq!(recommend_mode(&caps), ExchangeMode::Link);
    }

    #[test]
    fn link_always_available_with_internet() {
        let caps = DeviceCapabilities {
            has_internet: true,
            ..Default::default()
        };
        assert_eq!(
            check_mode_availability(ExchangeMode::Link, &caps),
            ModeAvailability::Available
        );
    }
}
