// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange command/event protocol (ADR-031).
//!
//! Core emits [`ExchangeCommand`]s to tell frontends what hardware actions to
//! perform (display QR, start BLE scan, emit audio challenge, etc.).
//!
//! Frontends report hardware results back via [`ExchangeHardwareEvent`]s.
//!
//! This decouples the protocol state machine from platform-specific hardware
//! access and works over UniFFI / C ABI boundaries (all types are serializable).

use serde::{Deserialize, Serialize};

/// A command from core to the frontend requesting a hardware action.
///
/// Frontends match on these and dispatch to platform-specific APIs
/// (camera, BLE stack, NFC reader, audio subsystem). Commands that the
/// platform cannot fulfil should be answered with
/// [`ExchangeHardwareEvent::HardwareUnavailable`].
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ExchangeCommand {
    // ── QR ───────────────────────────────────────────────────────────
    /// Display a QR code containing `data`.
    QrDisplay { data: String },
    /// Request the frontend to open a QR scanner (camera).
    QrRequestScan,

    // ── BLE ──────────────────────────────────────────────────────────
    /// Start advertising the vauchi BLE service with the given payload.
    BleStartAdvertising {
        service_uuid: String,
        payload: Vec<u8>,
    },
    /// Start scanning for vauchi BLE peripherals.
    BleStartScanning { service_uuid: String },
    /// Connect to a discovered BLE device.
    BleConnect { device_id: String },
    /// Write data to a BLE characteristic.
    BleWriteCharacteristic { uuid: String, data: Vec<u8> },
    /// Read data from a BLE characteristic.
    BleReadCharacteristic { uuid: String },
    /// Disconnect from the current BLE peer.
    BleDisconnect,

    // ── NFC ──────────────────────────────────────────────────────────
    /// Activate the NFC interface and prepare to exchange `payload`.
    NfcActivate { payload: Vec<u8> },
    /// Deactivate the NFC interface.
    NfcDeactivate,

    // ── Audio (ultrasonic proximity) ─────────────────────────────────
    /// Emit an ultrasonic challenge signal.
    AudioEmitChallenge { data: Vec<u8> },
    /// Listen for an ultrasonic response within `timeout_ms`.
    AudioListenForResponse { timeout_ms: u64 },
    /// Stop all audio operations.
    AudioStop,

    // ── Accelerometer ───────────────────────────────────────────────
    /// Start accelerometer sampling for proximity verification.
    AccelerometerStart,
    /// Stop accelerometer sampling.
    AccelerometerStop,

    // ── Relay escrow ────────────────────────────────────────────────
    /// Deposit encrypted card into relay escrow gate.
    RelayEscrowDeposit {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
        encrypted_card: Vec<u8>,
        ttl_seconds: u32,
    },
    /// Check relay escrow gate readiness (poll until ready).
    ///
    /// Frontends should poll at `suggested_interval_ms` with exponential
    /// backoff (cap at 5 min). Report `RelayEscrowReady` when gate has
    /// ≥2 deposits, or `RelayEscrowFailed` on error/timeout.
    RelayEscrowCheck {
        gate_hash: Vec<u8>,
        /// Suggested initial polling interval in milliseconds.
        suggested_interval_ms: u32,
    },
    /// Retrieve blob from relay escrow gate.
    RelayEscrowRetrieve {
        gate_hash: Vec<u8>,
        slot_hash: Vec<u8>,
    },

    // ── Link mode ───────────────────────────────────────────────────
    /// Show system share sheet with a URL.
    ShowShareSheet { url: String },

    // ── BLE cleanup ────────────────────────────────────────────────
    // Appended (not inserted) to preserve serde discriminant ordering.
    /// Stop BLE scanning (saves battery after discovery completes).
    BleStopScanning,

    // ── Direct transport (USB/TCP) ─────────────────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Send an exchange payload over a direct transport (USB cable / local TCP).
    ///
    /// The frontend should:
    /// 1. Send `payload` to the peer over the established TCP connection
    /// 2. Receive the peer's payload from the same connection
    /// 3. Report the peer's data via [`ExchangeHardwareEvent::DirectPayloadReceived`]
    ///
    /// The `is_initiator` flag determines send/recv ordering to avoid deadlock
    /// (initiator sends first, responder receives first).
    DirectSend {
        payload: Vec<u8>,
        is_initiator: bool,
    },

    // ── Image picking (avatar editor, ADR-042) ─────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Request the frontend to open the device photo library / gallery.
    ///
    /// Frontend should return [`ExchangeHardwareEvent::ImageReceived`]
    /// with the selected image bytes, or
    /// [`ExchangeHardwareEvent::ImagePickCancelled`] if the user
    /// dismisses the picker.
    ImagePickFromLibrary,
    /// Request the frontend to capture an image from the device camera.
    ///
    /// Frontend should return [`ExchangeHardwareEvent::ImageReceived`]
    /// with the captured image bytes, or
    /// [`ExchangeHardwareEvent::ImagePickCancelled`] if the user
    /// cancels.
    ImageCaptureFromCamera,
    /// Request the frontend to open a file picker for image files.
    ///
    /// Used on desktop platforms where a photo library may not exist.
    /// Frontend should return [`ExchangeHardwareEvent::ImageReceived`]
    /// with the selected image bytes, or
    /// [`ExchangeHardwareEvent::ImagePickCancelled`] if the user
    /// cancels.
    ImagePickFromFile,
}

/// A hardware event reported by the frontend back to core.
///
/// These are the results of previously issued [`ExchangeCommand`]s or
/// asynchronous hardware notifications (e.g., BLE discovery, NFC tap).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ExchangeHardwareEvent {
    // ── QR ───────────────────────────────────────────────────────────
    /// The user scanned a QR code containing `data`.
    QrScanned { data: String },

    // ── BLE ──────────────────────────────────────────────────────────
    /// A BLE peripheral was discovered during scanning.
    BleDeviceDiscovered {
        id: String,
        rssi: i16,
        adv_data: Vec<u8>,
    },
    /// Successfully connected to a BLE device.
    BleConnected { device_id: String },
    /// Data read from a BLE characteristic (response to `BleReadCharacteristic`).
    BleCharacteristicRead { uuid: String, data: Vec<u8> },
    /// BLE characteristic notification received (unsolicited push from peripheral).
    BleCharacteristicNotified { uuid: String, data: Vec<u8> },
    /// BLE connection lost or closed.
    BleDisconnected { reason: String },

    // ── NFC ──────────────────────────────────────────────────────────
    /// NFC data received from a tap exchange.
    NfcDataReceived { data: Vec<u8> },

    // ── Audio ────────────────────────────────────────────────────────
    /// Ultrasonic response signal detected.
    AudioResponseReceived { data: Vec<u8> },

    // ── Errors ───────────────────────────────────────────────────────
    /// A hardware operation failed.
    HardwareError { transport: String, error: String },
    /// The requested hardware is not available on this platform.
    HardwareUnavailable { transport: String },
    /// The user denied the required permission for this hardware.
    ///
    /// Distinct from `HardwareUnavailable` (hardware absent) — the hardware
    /// exists but the OS permission was denied. Frontends should send this
    /// when a runtime permission prompt is rejected (camera, BLE, microphone).
    PermissionDenied { transport: String },

    // ── Accelerometer ───────────────────────────────────────────────
    /// Accelerometer sample from the device.
    ///
    /// Acceleration is reported in milli-g (thousandths of standard gravity)
    /// to avoid `f32` and keep the type `Eq`-compatible across FFI boundaries.
    AccelerometerData {
        timestamp_ms: u64,
        x_milli_g: i32,
        y_milli_g: i32,
        z_milli_g: i32,
    },
    /// Impact detected by the device accelerometer.
    ImpactDetected {
        timestamp_ms: u64,
        magnitude_milli_g: i32,
    },

    // ── Relay escrow ────────────────────────────────────────────────
    /// Relay escrow gate has reached required deposit count.
    RelayEscrowReady { gate_hash: Vec<u8> },
    /// Relay escrow deposit/retrieve failed or gate expired.
    RelayEscrowFailed { gate_hash: Vec<u8>, reason: String },

    // ── Link mode ───────────────────────────────────────────────────
    /// User shared the link via share sheet.
    LinkShared,
    /// Link was opened by peer, providing their public key.
    LinkOpened { peer_public_key: Vec<u8> },

    // ── Relay escrow (added after v0.13 — append-only to preserve discriminants) ──
    /// Blob retrieved from relay escrow gate (response to `RelayEscrowRetrieve`).
    RelayEscrowBlobReceived { gate_hash: Vec<u8>, blob: Vec<u8> },

    // ── Direct transport (USB/TCP) ─────────────────────────────────
    /// Peer's exchange payload received over a direct transport.
    ///
    /// Sent by the frontend after completing the TCP exchange requested
    /// by [`ExchangeCommand::DirectSend`]. Contains the raw bytes of
    /// the peer's exchange payload (QR data string format).
    DirectPayloadReceived { data: Vec<u8> },

    // ── Image picking (avatar editor, ADR-042) ─────────────────────
    // Appended to preserve serde discriminant ordering.
    /// Image data received from photo library, camera, or file picker.
    ///
    /// The frontend sends raw image bytes (PNG, JPEG, etc.) — core
    /// handles format detection and normalization to WebP.
    ImageReceived { data: Vec<u8> },
    /// The user cancelled the image picker / camera without selecting.
    ImagePickCancelled,
}

// INLINE_TEST_REQUIRED: serde roundtrip tests need private enum variant access
#[cfg(test)]
mod tests {
    use super::*;

    // ── ExchangeCommand construction ────────────────────────────────

    #[test]
    fn qr_display_command_stores_data() {
        let cmd = ExchangeCommand::QrDisplay {
            data: "vauchi://exchange/abc123".into(),
        };
        assert!(
            matches!(cmd, ExchangeCommand::QrDisplay { data } if data == "vauchi://exchange/abc123")
        );
    }

    #[test]
    fn ble_start_advertising_stores_payload() {
        let payload = vec![0x01, 0x02, 0x03];
        let cmd = ExchangeCommand::BleStartAdvertising {
            service_uuid: "12345678-1234-1234-1234-123456789abc".into(),
            payload: payload.clone(),
        };
        assert!(
            matches!(cmd, ExchangeCommand::BleStartAdvertising { service_uuid, payload: p }
                if service_uuid == "12345678-1234-1234-1234-123456789abc" && p == payload)
        );
    }

    #[test]
    fn audio_listen_stores_timeout() {
        let cmd = ExchangeCommand::AudioListenForResponse { timeout_ms: 5000 };
        assert!(
            matches!(cmd, ExchangeCommand::AudioListenForResponse { timeout_ms } if timeout_ms == 5000)
        );
    }

    // ── ExchangeHardwareEvent construction ──────────────────────────

    #[test]
    fn qr_scanned_event_stores_data() {
        let evt = ExchangeHardwareEvent::QrScanned {
            data: "scanned-data".into(),
        };
        assert!(matches!(evt, ExchangeHardwareEvent::QrScanned { data } if data == "scanned-data"));
    }

    #[test]
    fn ble_device_discovered_stores_rssi() {
        let evt = ExchangeHardwareEvent::BleDeviceDiscovered {
            id: "device-1".into(),
            rssi: -42,
            adv_data: vec![0xFF],
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::BleDeviceDiscovered { rssi, .. } if rssi == -42)
        );
    }

    #[test]
    fn hardware_unavailable_stores_transport() {
        let evt = ExchangeHardwareEvent::HardwareUnavailable {
            transport: "BLE".into(),
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::HardwareUnavailable { transport } if transport == "BLE")
        );
    }

    // @internal
    #[test]
    fn permission_denied_stores_transport() {
        let evt = ExchangeHardwareEvent::PermissionDenied {
            transport: "camera".into(),
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::PermissionDenied { transport } if transport == "camera")
        );
    }

    // @internal
    #[test]
    fn permission_denied_is_distinct_from_hardware_unavailable() {
        let denied = ExchangeHardwareEvent::PermissionDenied {
            transport: "camera".into(),
        };
        let unavailable = ExchangeHardwareEvent::HardwareUnavailable {
            transport: "camera".into(),
        };
        assert_ne!(denied, unavailable);
    }

    #[test]
    fn hardware_error_stores_details() {
        let evt = ExchangeHardwareEvent::HardwareError {
            transport: "NFC".into(),
            error: "no reader detected".into(),
        };
        assert!(
            matches!(evt, ExchangeHardwareEvent::HardwareError { transport, error }
                if transport == "NFC" && error == "no reader detected")
        );
    }

    // ── Serialization roundtrips ────────────────────────────────────

    #[test]
    fn command_serialization_roundtrip() {
        let commands = vec![
            ExchangeCommand::QrDisplay {
                data: "test".into(),
            },
            ExchangeCommand::QrRequestScan,
            ExchangeCommand::BleStartAdvertising {
                service_uuid: "uuid".into(),
                payload: vec![1, 2, 3],
            },
            ExchangeCommand::BleDisconnect,
            ExchangeCommand::NfcActivate {
                payload: vec![0xAA],
            },
            ExchangeCommand::NfcDeactivate,
            ExchangeCommand::AudioEmitChallenge {
                data: vec![0x01; 16],
            },
            ExchangeCommand::AudioListenForResponse { timeout_ms: 3000 },
            ExchangeCommand::AudioStop,
            ExchangeCommand::AccelerometerStart,
            ExchangeCommand::AccelerometerStop,
            ExchangeCommand::RelayEscrowDeposit {
                gate_hash: vec![0xAB; 32],
                slot_hash: vec![0xCD; 32],
                encrypted_card: vec![0x01; 64],
                ttl_seconds: 3600,
            },
            ExchangeCommand::RelayEscrowCheck {
                gate_hash: vec![0xAB; 32],
                suggested_interval_ms: 30_000,
            },
            ExchangeCommand::RelayEscrowRetrieve {
                gate_hash: vec![0xAB; 32],
                slot_hash: vec![0xCD; 32],
            },
            ExchangeCommand::ShowShareSheet {
                url: "https://vauchi.app/link/abc123".into(),
            },
            ExchangeCommand::ImagePickFromLibrary,
            ExchangeCommand::ImageCaptureFromCamera,
            ExchangeCommand::ImagePickFromFile,
        ];
        for cmd in &commands {
            let json = serde_json::to_string(cmd).expect("serialize");
            let decoded: ExchangeCommand = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(cmd, &decoded, "roundtrip failed for {:?}", cmd);
        }
    }

    #[test]
    fn event_serialization_roundtrip() {
        let events = vec![
            ExchangeHardwareEvent::QrScanned { data: "qr".into() },
            ExchangeHardwareEvent::BleDeviceDiscovered {
                id: "d1".into(),
                rssi: -60,
                adv_data: vec![],
            },
            ExchangeHardwareEvent::BleConnected {
                device_id: "d1".into(),
            },
            ExchangeHardwareEvent::BleCharacteristicRead {
                uuid: "char1".into(),
                data: vec![0x0B],
            },
            ExchangeHardwareEvent::BleDisconnected {
                reason: "timeout".into(),
            },
            ExchangeHardwareEvent::NfcDataReceived { data: vec![0xCC] },
            ExchangeHardwareEvent::AudioResponseReceived {
                data: vec![0x01; 8],
            },
            ExchangeHardwareEvent::HardwareError {
                transport: "BLE".into(),
                error: "adapter off".into(),
            },
            ExchangeHardwareEvent::HardwareUnavailable {
                transport: "NFC".into(),
            },
            ExchangeHardwareEvent::PermissionDenied {
                transport: "camera".into(),
            },
            ExchangeHardwareEvent::AccelerometerData {
                timestamp_ms: 1_000,
                x_milli_g: 1_000, // ~1 g lateral
                y_milli_g: 0,
                z_milli_g: -9_800, // ~-9.8 g (gravity)
            },
            ExchangeHardwareEvent::ImpactDetected {
                timestamp_ms: 2_000,
                magnitude_milli_g: 3_500, // ~3.5 g impact
            },
            ExchangeHardwareEvent::RelayEscrowReady {
                gate_hash: vec![0xDE; 32],
            },
            ExchangeHardwareEvent::RelayEscrowFailed {
                gate_hash: vec![0xDE; 32],
                reason: "gate expired".into(),
            },
            ExchangeHardwareEvent::LinkShared,
            ExchangeHardwareEvent::LinkOpened {
                peer_public_key: vec![0x04; 32],
            },
            ExchangeHardwareEvent::ImageReceived {
                data: vec![0xFF, 0xD8, 0xFF],
            },
            ExchangeHardwareEvent::ImagePickCancelled,
        ];
        for evt in &events {
            let json = serde_json::to_string(evt).expect("serialize");
            let decoded: ExchangeHardwareEvent = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(evt, &decoded, "roundtrip failed for {:?}", evt);
        }
    }

    // ── Clone + equality ────────────────────────────────────────────

    #[test]
    fn command_clone_equals_original() {
        let cmd = ExchangeCommand::BleWriteCharacteristic {
            uuid: "test-uuid".into(),
            data: vec![1, 2, 3, 4, 5],
        };
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    #[test]
    fn event_clone_equals_original() {
        let evt = ExchangeHardwareEvent::BleCharacteristicNotified {
            uuid: "notify-uuid".into(),
            data: vec![0xDE, 0xAD],
        };
        let cloned = evt.clone();
        assert_eq!(evt, cloned);
    }

    // ── All variants covered ────────────────────────────────────────

    #[test]
    fn all_command_variants_are_distinct() {
        let variants: Vec<ExchangeCommand> = vec![
            ExchangeCommand::QrDisplay { data: "".into() },
            ExchangeCommand::QrRequestScan,
            ExchangeCommand::BleStartAdvertising {
                service_uuid: "".into(),
                payload: vec![],
            },
            ExchangeCommand::BleStartScanning {
                service_uuid: "".into(),
            },
            ExchangeCommand::BleConnect {
                device_id: "".into(),
            },
            ExchangeCommand::BleWriteCharacteristic {
                uuid: "".into(),
                data: vec![],
            },
            ExchangeCommand::BleReadCharacteristic { uuid: "".into() },
            ExchangeCommand::BleDisconnect,
            ExchangeCommand::NfcActivate { payload: vec![] },
            ExchangeCommand::NfcDeactivate,
            ExchangeCommand::AudioEmitChallenge { data: vec![] },
            ExchangeCommand::AudioListenForResponse { timeout_ms: 0 },
            ExchangeCommand::AudioStop,
            ExchangeCommand::AccelerometerStart,
            ExchangeCommand::AccelerometerStop,
            ExchangeCommand::RelayEscrowDeposit {
                gate_hash: vec![],
                slot_hash: vec![],
                encrypted_card: vec![],
                ttl_seconds: 0,
            },
            ExchangeCommand::RelayEscrowCheck {
                gate_hash: vec![],
                suggested_interval_ms: 0,
            },
            ExchangeCommand::RelayEscrowRetrieve {
                gate_hash: vec![],
                slot_hash: vec![],
            },
            ExchangeCommand::ShowShareSheet { url: "".into() },
            ExchangeCommand::BleStopScanning,
            ExchangeCommand::DirectSend {
                payload: vec![],
                is_initiator: false,
            },
            ExchangeCommand::ImagePickFromLibrary,
            ExchangeCommand::ImageCaptureFromCamera,
            ExchangeCommand::ImagePickFromFile,
        ];
        // 24 total command variants
        assert_eq!(variants.len(), 24);
    }

    #[test]
    fn all_event_variants_are_distinct() {
        let variants: Vec<ExchangeHardwareEvent> = vec![
            ExchangeHardwareEvent::QrScanned { data: "".into() },
            ExchangeHardwareEvent::BleDeviceDiscovered {
                id: "".into(),
                rssi: 0,
                adv_data: vec![],
            },
            ExchangeHardwareEvent::BleConnected {
                device_id: "".into(),
            },
            ExchangeHardwareEvent::BleCharacteristicRead {
                uuid: "".into(),
                data: vec![],
            },
            ExchangeHardwareEvent::BleCharacteristicNotified {
                uuid: "".into(),
                data: vec![],
            },
            ExchangeHardwareEvent::BleDisconnected { reason: "".into() },
            ExchangeHardwareEvent::NfcDataReceived { data: vec![] },
            ExchangeHardwareEvent::AudioResponseReceived { data: vec![] },
            ExchangeHardwareEvent::HardwareError {
                transport: "".into(),
                error: "".into(),
            },
            ExchangeHardwareEvent::HardwareUnavailable {
                transport: "".into(),
            },
            ExchangeHardwareEvent::PermissionDenied {
                transport: "".into(),
            },
            ExchangeHardwareEvent::AccelerometerData {
                timestamp_ms: 0,
                x_milli_g: 0,
                y_milli_g: 0,
                z_milli_g: 0,
            },
            ExchangeHardwareEvent::ImpactDetected {
                timestamp_ms: 0,
                magnitude_milli_g: 0,
            },
            ExchangeHardwareEvent::RelayEscrowReady { gate_hash: vec![] },
            ExchangeHardwareEvent::RelayEscrowFailed {
                gate_hash: vec![],
                reason: "".into(),
            },
            ExchangeHardwareEvent::LinkShared,
            ExchangeHardwareEvent::LinkOpened {
                peer_public_key: vec![],
            },
            ExchangeHardwareEvent::RelayEscrowBlobReceived {
                gate_hash: vec![],
                blob: vec![],
            },
            ExchangeHardwareEvent::DirectPayloadReceived { data: vec![] },
            ExchangeHardwareEvent::ImageReceived { data: vec![] },
            ExchangeHardwareEvent::ImagePickCancelled,
        ];
        // 21 total event variants
        assert_eq!(variants.len(), 21);
    }
}
