// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport readiness ledger — the capability/permission split.
//!
//! A transport's usability has two independent dimensions:
//!
//! - **Hardware presence** — static, known at startup, lives in
//!   [`DeviceCapabilities`]. A device either has a camera/BLE radio/NFC
//!   controller or it doesn't.
//! - **OS permission** — dynamic, per-session, learned only from runtime
//!   events. The hardware exists but the OS may have denied access.
//!
//! [`check_mode_availability`](super::super::mode_availability::check_mode_availability)
//! historically modelled only presence, so a mode whose hardware exists but
//! whose permission was denied still showed as `Available` and the exchange
//! entered a wait state that could never complete (the device-verified
//! infinite "Searching…", `2026-06-11-exchange-waits-forever-without-capabilities`
//! Phase 2 / Option B).
//!
//! [`TransportReadiness`] is the core-owned, **transient** (never persisted)
//! ledger of the permission dimension, keyed on [`DeviceRequirement`]. It
//! generalises the per-engine `CameraGate` (one transport, one engine) to a
//! device-wide map. Presence is *not* stored here — it stays the single source
//! of truth in [`DeviceCapabilities`] and is joined in at read time via
//! [`requirement_present`].
//!
//! ## Re-learning a grant (ADR-030/031)
//!
//! There is no "permission granted" event — core consumes events, it never
//! queries the OS. A denial is recorded from [`Event::PermissionDenied`]; the
//! only path back to [`PermissionState::Granted`] is an explicit user grant
//! action (the camera-gate precedent: tapping "grant" optimistically clears the
//! denial and re-triggers the hardware). Hence [`note_granted`] is driven by an
//! affordance, not an event, and the permission dimension is last-write-wins.
//!
//! [`DeviceCapabilities`]: super::types::DeviceCapabilities
//! [`Event::PermissionDenied`]: crate::platform::Event::PermissionDenied
//! [`note_granted`]: TransportReadiness::note_granted

use crate::exchange::capability::types::DeviceCapabilities;
use crate::exchange::mode::DeviceRequirement;
use crate::types::AudioCapability;
use std::collections::HashMap;

/// Runtime OS-permission state for a transport requirement.
///
/// Distinct from hardware presence (which lives in [`DeviceCapabilities`]).
/// `Unknown` is the default: not yet observed, treated optimistically as usable
/// until a denial arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionState {
    /// Never observed. Optimistically usable (we have not been told otherwise).
    #[default]
    Unknown,
    /// The user granted access (via a grant affordance — see module docs).
    Granted,
    /// The OS denied access. Recoverable: a grant affordance can re-prompt.
    Denied,
}

/// Combined readiness of one transport requirement = presence × permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequirementReadiness {
    /// Hardware present and permission not denied — usable (a grant `Unknown`
    /// is treated optimistically as usable).
    Ready,
    /// Hardware present but the OS permission was denied — recoverable via a
    /// grant prompt.
    PermissionDenied,
    /// Hardware absent — there is no grant path, only another mode.
    HardwareAbsent,
}

/// Transient, core-owned ledger of per-requirement OS-permission state.
///
/// Only permission-bearing requirements (camera, BLE, microphone) ever carry a
/// non-`Unknown` entry; presence-only requirements (USB, internet,
/// accelerometer, speaker) have no permission concept and read as `Unknown`
/// (i.e. never block on permission).
#[derive(Debug, Clone, Default)]
pub struct TransportReadiness {
    permissions: HashMap<DeviceRequirement, PermissionState>,
}

impl TransportReadiness {
    /// Empty ledger — every requirement `Unknown` (optimistically usable).
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an OS permission denial for `req`.
    pub fn note_denied(&mut self, req: DeviceRequirement) {
        self.permissions.insert(req, PermissionState::Denied);
    }

    /// Record a user-initiated grant for `req` — the only path back to
    /// [`PermissionState::Granted`] (no "granted" event exists; see module docs).
    pub fn note_granted(&mut self, req: DeviceRequirement) {
        self.permissions.insert(req, PermissionState::Granted);
    }

    /// Ingest a platform event's free-string `transport` label as a denial,
    /// mapping it to the requirement it gates (see `requirement_for_transport`
    /// for the exact set: camera, ble/BLE/bluetooth, nfc, microphone/mic).
    /// Case-insensitive; unrecognised or non-permission-bearing labels — e.g.
    /// `"location"` (the ADR-051 capture-geolocation permission, not a
    /// transport) — are ignored.
    pub fn note_permission_denied(&mut self, transport: &str) {
        if let Some(req) = requirement_for_transport(transport) {
            self.note_denied(req);
        }
    }

    /// Permission state for `req` (`Unknown` if never observed).
    pub fn permission(&self, req: DeviceRequirement) -> PermissionState {
        self.permissions.get(&req).copied().unwrap_or_default()
    }

    /// Combined readiness of `req` = presence (`caps`) × permission (`self`).
    pub fn requirement_readiness(
        &self,
        req: DeviceRequirement,
        caps: &DeviceCapabilities,
    ) -> RequirementReadiness {
        if !requirement_present(req, caps) {
            return RequirementReadiness::HardwareAbsent;
        }
        match self.permission(req) {
            PermissionState::Denied => RequirementReadiness::PermissionDenied,
            PermissionState::Granted | PermissionState::Unknown => RequirementReadiness::Ready,
        }
    }
}

/// Whether the hardware backing `req` is present in `caps`.
///
/// The single source of truth for the requirement → capability join, shared
/// with [`check_mode_availability`](super::super::mode_availability::check_mode_availability)
/// so presence is never modelled twice.
pub(crate) fn requirement_present(req: DeviceRequirement, caps: &DeviceCapabilities) -> bool {
    match req {
        DeviceRequirement::Camera => caps.has_camera,
        DeviceRequirement::Ble => caps.has_ble,
        DeviceRequirement::Nfc => caps.has_nfc,
        DeviceRequirement::Microphone => {
            matches!(
                caps.audio,
                AudioCapability::Full | AudioCapability::ReceiveOnly
            )
        }
        DeviceRequirement::Speaker => {
            matches!(
                caps.audio,
                AudioCapability::Full | AudioCapability::EmitOnly
            )
        }
        DeviceRequirement::Accelerometer => caps.has_accelerometer,
        DeviceRequirement::Internet => caps.has_internet,
        DeviceRequirement::UsbPort => caps.has_usb_port,
    }
}

/// Map a platform event's free-string transport label to the
/// [`DeviceRequirement`] it gates. `None` for unrecognised or
/// presence-only (non-permission-bearing) transports.
fn requirement_for_transport(transport: &str) -> Option<DeviceRequirement> {
    match transport.to_ascii_lowercase().as_str() {
        "camera" => Some(DeviceRequirement::Camera),
        // Android emits "ble" (BleFailure), iOS "BLE" (case folded above);
        // "bluetooth" is a defensive alias. NOTE: "location" is deliberately
        // NOT here — it is the ADR-051 capture-geolocation permission, not a
        // transport. Android BLE scanning uses BLUETOOTH_SCAN with
        // `neverForLocation` (AndroidManifest), so a location denial must not
        // gate BLE.
        "ble" | "bluetooth" => Some(DeviceRequirement::Ble),
        // NFC needs an OS permission on Android 12+ (Nearby Devices); the NFC
        // engine emits "nfc" on denial.
        "nfc" => Some(DeviceRequirement::Nfc),
        "microphone" | "mic" => Some(DeviceRequirement::Microphone),
        _ => None,
    }
}

// INLINE_TEST_REQUIRED: exercises the private requirement_for_transport mapping
// and the presence × permission join against module-internal logic.
#[cfg(test)]
mod tests {
    use super::*;

    fn caps_all() -> DeviceCapabilities {
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

    // @internal
    #[test]
    fn default_permission_is_unknown_and_reads_ready_when_present() {
        let led = TransportReadiness::new();
        assert_eq!(
            led.permission(DeviceRequirement::Camera),
            PermissionState::Unknown
        );
        assert_eq!(
            led.requirement_readiness(DeviceRequirement::Camera, &caps_all()),
            RequirementReadiness::Ready
        );
    }

    // @internal
    #[test]
    fn denied_then_present_reads_permission_denied() {
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Camera);
        assert_eq!(
            led.permission(DeviceRequirement::Camera),
            PermissionState::Denied
        );
        assert_eq!(
            led.requirement_readiness(DeviceRequirement::Camera, &caps_all()),
            RequirementReadiness::PermissionDenied
        );
    }

    // @internal
    #[test]
    fn absent_hardware_reads_hardware_absent_regardless_of_permission() {
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Camera);
        let caps = DeviceCapabilities {
            has_camera: false,
            ..caps_all()
        };
        // Absence dominates: no grant path when the hardware isn't there.
        assert_eq!(
            led.requirement_readiness(DeviceRequirement::Camera, &caps),
            RequirementReadiness::HardwareAbsent
        );
    }

    // @internal
    #[test]
    fn grant_overrides_a_prior_denial_last_write_wins() {
        let mut led = TransportReadiness::new();
        led.note_denied(DeviceRequirement::Ble);
        led.note_granted(DeviceRequirement::Ble);
        assert_eq!(
            led.permission(DeviceRequirement::Ble),
            PermissionState::Granted
        );
        assert_eq!(
            led.requirement_readiness(DeviceRequirement::Ble, &caps_all()),
            RequirementReadiness::Ready
        );
    }

    // @internal
    #[test]
    fn permission_denied_event_label_maps_to_requirement() {
        let mut led = TransportReadiness::new();
        led.note_permission_denied("Camera"); // case-insensitive
        led.note_permission_denied("ble");
        led.note_permission_denied("MICROPHONE");
        led.note_permission_denied("nfc");
        assert_eq!(
            led.permission(DeviceRequirement::Camera),
            PermissionState::Denied
        );
        assert_eq!(
            led.permission(DeviceRequirement::Ble),
            PermissionState::Denied
        );
        assert_eq!(
            led.permission(DeviceRequirement::Microphone),
            PermissionState::Denied
        );
        assert_eq!(
            led.permission(DeviceRequirement::Nfc),
            PermissionState::Denied
        );
    }

    // @internal
    #[test]
    fn ios_uppercase_ble_alias_maps_to_ble() {
        // iOS emits "BLE"; Android emits "ble". Both must land on the same key.
        let mut led = TransportReadiness::new();
        led.note_permission_denied("BLE");
        assert_eq!(
            led.permission(DeviceRequirement::Ble),
            PermissionState::Denied
        );
    }

    // @internal
    #[test]
    fn unrecognised_or_presence_only_transport_label_is_ignored() {
        let mut led = TransportReadiness::new();
        led.note_permission_denied("usb");
        led.note_permission_denied("internet");
        led.note_permission_denied("gibberish");
        // "location" is the ADR-051 capture-geolocation permission, not a
        // transport — it must not gate BLE (or anything).
        led.note_permission_denied("location");
        assert_eq!(
            led.permission(DeviceRequirement::UsbPort),
            PermissionState::Unknown
        );
        assert_eq!(
            led.permission(DeviceRequirement::Internet),
            PermissionState::Unknown
        );
        assert_eq!(
            led.permission(DeviceRequirement::Ble),
            PermissionState::Unknown,
            "location denial must not deny BLE"
        );
    }
}
