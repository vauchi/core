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
