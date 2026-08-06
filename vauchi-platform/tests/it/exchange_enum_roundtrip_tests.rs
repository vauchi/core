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
