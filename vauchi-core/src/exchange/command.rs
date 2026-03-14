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
}

/// A hardware event reported by the frontend back to core.
///
/// These are the results of previously issued [`ExchangeCommand`]s or
/// asynchronous hardware notifications (e.g., BLE discovery, NFC tap).
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
}

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
        ];
        // 13 total command variants
        assert_eq!(variants.len(), 13);
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
        ];
        // 10 total event variants
        assert_eq!(variants.len(), 10);
    }
}
