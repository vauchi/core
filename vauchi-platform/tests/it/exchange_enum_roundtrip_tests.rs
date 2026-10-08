// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Roundtrip tests for the `MobileCommand` / `MobileEvent` UniFFI mirror
//! enums (ADR-031). These exercise the public `From` conversions between
//! the mobile-facing enums and `vauchi_core::{Command, Event}` — the
//! command/event bridge that survived the slice-32m cycle-thread
//! retirement. Public API, so they live in `tests/` rather than inline.

use vauchi_core::{Command, Event};
use vauchi_platform::{MobileCommand, MobileEvent};

// @internal
#[test]
fn direct_send_roundtrips_through_mobile_enum() {
    let cmd = Command::DirectSend {
        payload: vec![1, 2, 3],
        is_initiator: true,
    };
    let mobile: MobileCommand = cmd.into();
    match mobile {
        MobileCommand::DirectSend {
            payload,
            is_initiator,
        } => {
            assert_eq!(payload, vec![1, 2, 3]);
            assert!(is_initiator);
        }
        other => panic!("expected DirectSend, got {other:?}"),
    }
}

// @internal
#[test]
fn direct_payload_received_roundtrips_through_mobile_enum() {
    let evt = MobileEvent::DirectPayloadReceived {
        data: vec![4, 5, 6],
    };
    let core: Event = evt.into();
    match core {
        Event::DirectPayloadReceived { data } => {
            assert_eq!(data, vec![4, 5, 6]);
        }
        other => panic!("expected DirectPayloadReceived, got {other:?}"),
    }
}

// @internal
#[test]
fn location_result_roundtrips_through_mobile_enum() {
    let evt = MobileEvent::LocationResult {
        latitude: 47.37,
        longitude: 8.54,
        accuracy_meters: Some(12.0),
    };
    let core: Event = evt.into();
    match core {
        Event::LocationResult {
            latitude,
            longitude,
            accuracy_meters,
        } => {
            assert!((latitude - 47.37).abs() < 1e-9);
            assert!((longitude - 8.54).abs() < 1e-9);
            assert_eq!(accuracy_meters, Some(12.0));
        }
        other => panic!("expected LocationResult, got {other:?}"),
    }
}

// @internal
#[test]
fn hardware_event_json_emits_the_canonical_envelope() {
    // ADR-066 admits one public dispatch path. The codec must produce JSON
    // the canonical reader accepts, carrying the same event the typed
    // conversion produces — byte-bearing payloads included.
    let mobile = MobileEvent::BleCharacteristicNotified {
        device_id: "peer-1".into(),
        direction: vauchi_platform::MobileBleLinkDirection::Outbound,
        uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567897".into(),
        data: vec![0xde, 0xad, 0xbe, 0xef],
    };
    let expected: Event = mobile.clone().into();

    let json = vauchi_platform::hardware_event_json(mobile);
    let parsed =
        vauchi_core::event_from_json(&json).expect("canonical reader accepts codec output");

    assert_eq!(
        serde_json::to_value(&parsed).expect("serialize parsed"),
        serde_json::to_value(&expected).expect("serialize expected"),
        "codec must carry the identical event through the canonical envelope",
    );
}

// @internal
#[test]
fn hardware_event_json_qr_scan_uses_the_canonical_tag() {
    let json = vauchi_platform::hardware_event_json(MobileEvent::QrScanned {
        data: "vauchi://link?token=abc123".into(),
    });
    assert_eq!(
        json, r#"{"QrScanned":{"data":"vauchi://link?token=abc123"}}"#,
        "shells must be able to feed codec output straight to dispatch_json",
    );
}

// @internal
#[test]
fn nfc_activate_carries_framed_apdus_through_mobile_enum() {
    let cmd = Command::NfcActivate {
        payload: vec![0xAA],
        apdus: vec![vec![0x00, 0xA4], vec![0x00, 0xE0]],
    };
    let mobile: MobileCommand = cmd.into();
    match mobile {
        MobileCommand::NfcActivate { payload, apdus } => {
            assert_eq!(payload, vec![0xAA]);
            assert_eq!(apdus, vec![vec![0x00, 0xA4], vec![0x00, 0xE0]]);
        }
        other => panic!("expected NfcActivate, got {other:?}"),
    }
}

// @internal
#[test]
fn nfc_send_apdu_carries_framed_apdus_through_mobile_enum() {
    let cmd = Command::NfcSendApdu {
        data: vec![0x90, 0x00],
        apdus: vec![vec![0x90, 0x00]],
    };
    let mobile: MobileCommand = cmd.into();
    match mobile {
        MobileCommand::NfcSendApdu { data, apdus } => {
            assert_eq!(data, vec![0x90, 0x00]);
            assert_eq!(apdus, vec![vec![0x90, 0x00]]);
        }
        other => panic!("expected NfcSendApdu, got {other:?}"),
    }
}

// @internal
#[test]
fn nfc_apdu_received_roundtrips_through_mobile_enum() {
    let evt = MobileEvent::NfcApduReceived {
        bytes: vec![0x6A, 0x82],
    };
    let core: Event = evt.into();
    assert_eq!(
        core,
        Event::NfcApduReceived {
            bytes: vec![0x6A, 0x82]
        }
    );
}

// @internal
#[test]
fn nfc_failed_roundtrips_through_mobile_enum() {
    let evt = MobileEvent::NfcFailed {
        reason: "tag lost".into(),
    };
    let core: Event = evt.into();
    assert_eq!(
        core,
        Event::NfcFailed {
            reason: "tag lost".into()
        }
    );
}

/// One sample of every `MobileEvent` variant. The exhaustive match in
/// `variant_name` has no wildcard arm, so a new variant does not compile
/// until it has a sample here.
fn every_mobile_event() -> Vec<MobileEvent> {
    use vauchi_platform::MobileBleLinkDirection::{Inbound, Outbound};
    vec![
        MobileEvent::QrScanned {
            data: "vauchi://q".into(),
        },
        MobileEvent::LocalNetworkAddressChanged {
            address: Some("192.0.2.7".into()),
        },
        MobileEvent::LocalNetworkAddressChanged { address: None },
        MobileEvent::BleDeviceDiscovered {
            id: "d1".into(),
            rssi: -61,
            adv_data: vec![0, 255, 7],
        },
        MobileEvent::BleConnected {
            device_id: "d1".into(),
            direction: Outbound,
        },
        MobileEvent::BleCharacteristicRead {
            device_id: "d1".into(),
            direction: Inbound,
            uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".into(),
            data: vec![1, 2, 3],
        },
        MobileEvent::BleCharacteristicNotified {
            device_id: "d1".into(),
            direction: Outbound,
            uuid: "a1b2c3d4-e5f6-7890-abcd-ef1234567891".into(),
            data: vec![],
        },
        MobileEvent::BleDisconnected {
            device_id: "d1".into(),
            direction: Inbound,
            reason: "gone".into(),
        },
        MobileEvent::NfcDataReceived {
            data: vec![0xca, 0xfe],
        },
        MobileEvent::NfcApduReceived {
            bytes: vec![0x00, 0xa4, 0x04, 0x00],
        },
        MobileEvent::NfcFailed {
            reason: "tag lost".into(),
        },
        MobileEvent::AudioSamplesRecorded {
            samples: vec![0.0, -0.5, 0.25],
            sample_rate: 44_100,
        },
        MobileEvent::AccelerometerData {
            timestamp_ms: 1_700_000_000_000,
            x_milli_g: -12,
            y_milli_g: 980,
            z_milli_g: 3,
        },
        MobileEvent::ImpactDetected {
            timestamp_ms: 42,
            magnitude_milli_g: 2_500,
        },
        MobileEvent::RelayEscrowReady {
            gate_hash: vec![9; 32],
        },
        MobileEvent::RelayEscrowBlobReceived {
            gate_hash: vec![8; 32],
            blob: vec![1, 0, 1],
        },
        MobileEvent::RelayEscrowFailed {
            gate_hash: vec![7; 32],
            reason: "timeout".into(),
        },
        MobileEvent::LinkShared,
        MobileEvent::LinkOpened {
            peer_public_key: vec![5; 32],
        },
        MobileEvent::DirectPayloadReceived { data: vec![4, 4] },
        MobileEvent::DirectCardReceived {
            ciphertext: vec![3, 3, 3],
        },
        MobileEvent::ImageReceived {
            data: vec![0x52, 0x49, 0x46, 0x46],
        },
        MobileEvent::ImagePickCancelled,
        MobileEvent::FilePickedFromUser {
            bytes: vec![0x7b, 0x7d],
            filename: "backup.vauchi".into(),
        },
        MobileEvent::FilePickCancelledByUser,
        MobileEvent::BiometricUnlockSucceeded,
        MobileEvent::HardwareError {
            transport: "ble".into(),
            error: "radio off".into(),
        },
        MobileEvent::HardwareUnavailable {
            transport: "nfc".into(),
        },
        MobileEvent::PermissionDenied {
            transport: "camera".into(),
        },
        MobileEvent::LocationResult {
            latitude: 47.3769,
            longitude: 8.5417,
            accuracy_meters: Some(12.5),
        },
        MobileEvent::LocationResult {
            latitude: -33.9,
            longitude: 151.2,
            accuracy_meters: None,
        },
    ]
}

fn variant_name(event: &MobileEvent) -> &'static str {
    match event {
        MobileEvent::QrScanned { .. } => "QrScanned",
        MobileEvent::LocalNetworkAddressChanged { .. } => "LocalNetworkAddressChanged",
        MobileEvent::BleDeviceDiscovered { .. } => "BleDeviceDiscovered",
        MobileEvent::BleConnected { .. } => "BleConnected",
        MobileEvent::BleCharacteristicRead { .. } => "BleCharacteristicRead",
        MobileEvent::BleCharacteristicNotified { .. } => "BleCharacteristicNotified",
        MobileEvent::BleDisconnected { .. } => "BleDisconnected",
        MobileEvent::NfcDataReceived { .. } => "NfcDataReceived",
        MobileEvent::NfcApduReceived { .. } => "NfcApduReceived",
        MobileEvent::NfcFailed { .. } => "NfcFailed",
        MobileEvent::AudioSamplesRecorded { .. } => "AudioSamplesRecorded",
        MobileEvent::AccelerometerData { .. } => "AccelerometerData",
        MobileEvent::ImpactDetected { .. } => "ImpactDetected",
        MobileEvent::RelayEscrowReady { .. } => "RelayEscrowReady",
        MobileEvent::RelayEscrowBlobReceived { .. } => "RelayEscrowBlobReceived",
        MobileEvent::RelayEscrowFailed { .. } => "RelayEscrowFailed",
        MobileEvent::LinkShared => "LinkShared",
        MobileEvent::LinkOpened { .. } => "LinkOpened",
        MobileEvent::DirectPayloadReceived { .. } => "DirectPayloadReceived",
        MobileEvent::DirectCardReceived { .. } => "DirectCardReceived",
        MobileEvent::ImageReceived { .. } => "ImageReceived",
        MobileEvent::ImagePickCancelled => "ImagePickCancelled",
        MobileEvent::FilePickedFromUser { .. } => "FilePickedFromUser",
        MobileEvent::FilePickCancelledByUser => "FilePickCancelledByUser",
        MobileEvent::BiometricUnlockSucceeded => "BiometricUnlockSucceeded",
        MobileEvent::HardwareError { .. } => "HardwareError",
        MobileEvent::HardwareUnavailable { .. } => "HardwareUnavailable",
        MobileEvent::PermissionDenied { .. } => "PermissionDenied",
        MobileEvent::LocationResult { .. } => "LocationResult",
    }
}

// android, iOS and macOS hand-mirrored this JSON per variant; they move
// to `hardware_event_json` (vauchi/private#547), so the codec carries the
// contract for every variant, not a sample of two.
// @internal
#[test]
fn hardware_event_json_carries_every_mobile_event_through_the_canonical_reader() {
    let events = every_mobile_event();
    let covered: std::collections::BTreeSet<_> = events.iter().map(variant_name).collect();
    assert_eq!(
        covered.len(),
        29,
        "a sample for each MobileEvent variant: {covered:?}"
    );

    for mobile in events {
        let name = variant_name(&mobile);
        let expected: Event = mobile.clone().into();

        let json = vauchi_platform::hardware_event_json(mobile);
        let parsed = vauchi_core::event_from_json(&json)
            .unwrap_or_else(|e| panic!("{name}: canonical reader rejects {json}: {e:?}"));

        assert_eq!(
            serde_json::to_value(&parsed).expect("serialize parsed"),
            serde_json::to_value(&expected).expect("serialize expected"),
            "{name}: codec output differs from the typed conversion",
        );
    }
}
