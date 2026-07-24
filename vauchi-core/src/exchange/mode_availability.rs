// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mode availability checking and recommendation.
//!
//! Determines which [`ExchangeMode`]s are usable on a given device by mapping
//! each [`DeviceRequirement`] to a field of [`DeviceCapabilities`], then
//! recommends the most capable available mode.

use crate::exchange::capability::readiness::requirement_present;
use crate::exchange::capability::types::DeviceCapabilities;
use crate::exchange::capability::{RequirementReadiness, TransportReadiness};
use crate::exchange::mode::{DeviceRequirement, ExchangeMode};

/// Availability status of an [`ExchangeMode`] on a specific device.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModeAvailability {
    /// All required hardware is present and fully functional.
    Available,
    /// Usable but with reduced functionality (reserved for future degraded-mode logic).
    Degraded { reason: String },
    /// All required hardware is present, but the OS permission for `requirement`
    /// was denied. Recoverable — a grant affordance can re-prompt. Distinct from
    /// `Unavailable` (hardware absent), which has no grant path.
    PermissionRequired { requirement: DeviceRequirement },
    /// One or more required hardware capabilities are absent.
    Unavailable { reason: String },
}

/// Check whether `mode` can run on `caps`.
///
/// Each [`DeviceRequirement`] in `mode.config().requires` is evaluated against
/// the corresponding `caps` field. If any requirement is unmet the mode is
/// `Unavailable` with a comma-separated list of missing hardware names.
/// Otherwise `Available` is returned.
pub fn check_mode_availability(mode: ExchangeMode, caps: &DeviceCapabilities) -> ModeAvailability {
    let mut missing: Vec<&'static str> = Vec::new();

    for req in mode.config().requires {
        // Presence join is the single source of truth in `readiness`
        // (shared with the TransportReadiness ledger — no duplicate map).
        if !requirement_present(*req, caps) {
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

/// Like [`check_mode_availability`] but factoring in runtime OS permission (the
/// [`TransportReadiness`] ledger), not just hardware presence.
///
/// Hardware absence is the hard blocker (`Unavailable`) and dominates a denial —
/// granting cannot conjure absent hardware. With all hardware present, a denied
/// required transport yields `PermissionRequired { requirement }` (grantable;
/// the first denied requirement, so the picker can offer one grant affordance).
/// Otherwise `Available`.
pub fn check_mode_availability_with_readiness(
    mode: ExchangeMode,
    caps: &DeviceCapabilities,
    readiness: &TransportReadiness,
) -> ModeAvailability {
    let mut missing: Vec<&'static str> = Vec::new();
    let mut denied: Option<DeviceRequirement> = None;

    for req in mode.config().requires {
        match readiness.requirement_readiness(*req, caps) {
            RequirementReadiness::HardwareAbsent => missing.push(requirement_name(req)),
            RequirementReadiness::PermissionDenied => {
                denied.get_or_insert(*req);
            }
            RequirementReadiness::Ready => {}
        }
    }

    if !missing.is_empty() {
        return ModeAvailability::Unavailable {
            reason: format!("Requires {}", missing.join(", ")),
        };
    }
    match denied {
        Some(requirement) => ModeAvailability::PermissionRequired { requirement },
        None => ModeAvailability::Available,
    }
}

/// Recommend the best [`ExchangeMode`] for `caps`.
///
/// Modes are tried in priority order: Hover, Magic, Shake, TapHoverShake,
/// TapTap, Bump, Glance, Broadcast, Web, Link. The first `Available` mode is
/// returned. Link is always available when `has_internet` is true; if nothing
/// else works, [`ExchangeMode::Link`] is returned as the ultimate fallback even
/// if Link itself is unavailable (no internet). The UI layer is responsible for
/// showing the mode as unavailable in that case.
pub fn recommend_mode(caps: &DeviceCapabilities) -> ExchangeMode {
    const PRIORITY: &[ExchangeMode] = &[
        ExchangeMode::Hover,
        ExchangeMode::Magic,
        ExchangeMode::Shake,
        ExchangeMode::TapHoverShake,
        ExchangeMode::TapTap,
        ExchangeMode::Bump,
        ExchangeMode::Glance,
        ExchangeMode::Link,
    ];

    for &mode in PRIORITY {
        if check_mode_availability(mode, caps) == ModeAvailability::Available {
            return mode;
        }
    }

    // Final fallback — returned even if Link itself is unavailable (no
    // internet). The UI layer shows it as unavailable in that case.
    ExchangeMode::Link
}

fn requirement_name(req: &DeviceRequirement) -> &'static str {
    match req {
        DeviceRequirement::Camera => "camera",
        DeviceRequirement::Ble => "BLE",
        DeviceRequirement::Nfc => "NFC",
        DeviceRequirement::Microphone => "microphone",
        DeviceRequirement::Speaker => "speaker",
        DeviceRequirement::Accelerometer => "accelerometer",
        DeviceRequirement::Internet => "internet",
        DeviceRequirement::UsbPort => "USB port",
    }
}

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
            has_usb_port: true,
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

    // @internal
    #[test]
    fn readiness_all_granted_is_available() {
        // Empty ledger = all Unknown = optimistically usable.
        let caps = full_caps();
        let led = TransportReadiness::new();
        assert_eq!(
            check_mode_availability_with_readiness(ExchangeMode::Glance, &caps, &led),
            ModeAvailability::Available
        );
    }

    // @internal
    #[test]
    fn readiness_denied_present_transport_is_permission_required() {
        // Glance requires Ble + Camera. Camera present but denied →
        // PermissionRequired{Camera} (grantable), not Unavailable.
        let caps = full_caps();
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Camera);
        assert_eq!(
            check_mode_availability_with_readiness(ExchangeMode::Glance, &caps, &led),
            ModeAvailability::PermissionRequired {
                requirement: DeviceRequirement::Camera
            }
        );
    }

    // @internal
    #[test]
    fn readiness_absent_hardware_dominates_a_denial() {
        // Camera absent AND BLE denied → Unavailable (hardware absence wins;
        // granting BLE cannot conjure a camera).
        let caps = DeviceCapabilities {
            has_camera: false,
            ..full_caps()
        };
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Ble);
        assert_eq!(
            check_mode_availability_with_readiness(ExchangeMode::Glance, &caps, &led),
            ModeAvailability::Unavailable {
                reason: "Requires camera".to_string()
            }
        );
    }

    #[test]
    fn no_nfc_makes_tap_tap_unavailable_but_not_tap_hover_shake() {
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
        // TapHoverShake ships the multi-stage-QR ritual — no NFC involved
        // (exchange-mode-contract-truth record, owner decision 2026-07-20).
        assert_eq!(
            check_mode_availability(ExchangeMode::TapHoverShake, &caps),
            ModeAvailability::Available
        );
    }

    #[test]
    fn desktop_only_camera_internet_modes() {
        // Desktop: camera + internet, no BLE / NFC / accelerometer, no audio
        let caps = DeviceCapabilities {
            has_camera: true,
            has_internet: true,
            ..Default::default()
        };
        assert!(
            matches!(
                check_mode_availability(ExchangeMode::Glance, &caps),
                ModeAvailability::Unavailable { .. }
            ),
            "G3: Glance now requires BLE (one-sided QR + BLE transfer)"
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
    fn recommend_picks_glance_for_camera_and_ble() {
        // G3: Glance now needs camera + BLE (one-sided QR + BLE transfer). A
        // device with both but no audio / accelerometer / NFC can't do
        // Hover / Magic / Shake / Bump / TapTap, so Glance is the
        // recommendation (above Link in priority).
        let caps = DeviceCapabilities {
            has_camera: true,
            has_ble: true,
            ..Default::default()
        };
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
