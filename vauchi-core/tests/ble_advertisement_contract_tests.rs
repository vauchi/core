// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::{VAUCHI_BLE_SERVICE_UUID, advertised_token};
use vauchi_core::platform::{
    BleAdvertisementObservation as BleAdvertisement, Event, event_from_json,
};

fn uuid(value: &str) -> [u8; 16] {
    *uuid::Uuid::parse_str(value).unwrap().as_bytes()
}

fn service() -> [u8; 16] {
    uuid(VAUCHI_BLE_SERVICE_UUID)
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[test]
fn core_decodes_current_and_legacy_advertisement_tokens() {
    assert_eq!(
        advertised_token(&[service(), uuid("00001234-0000-1000-8000-00805f9b34fb")]),
        Some(vec![0x12, 0x34])
    );
    assert_eq!(
        advertised_token(&[service(), uuid("12345678-0000-1000-8000-00805f9b34fb")]),
        Some(vec![0x12, 0x34, 0x56, 0x78])
    );
    assert_eq!(advertised_token(&[service()]), None);
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn unrelated_and_ambiguous_uuid_lists_do_not_select_a_peer_token() {
    let token = uuid("00001234-0000-1000-8000-00805f9b34fb");
    let other = uuid("00005678-0000-1000-8000-00805f9b34fb");
    assert_eq!(advertised_token(&[token]), None);
    assert_eq!(advertised_token(&[service(), token, other]), None);
    assert_eq!(
        advertised_token(&[service(), token, token]),
        Some(vec![0x12, 0x34])
    );
}

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn raw_advertisement_uses_the_canonical_event_envelope() {
    let observation = BleAdvertisement::new("device".into(), -42, vec![service()]).unwrap();
    let event = Event::BleAdvertisementObserved(observation);
    let encoded = serde_json::to_string(&event).unwrap();
    assert_eq!(event_from_json(&encoded).unwrap(), event);
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        serde_json::json!({"BleAdvertisementObserved": {"id":"device", "rssi":-42, "service_uuids":[service()]}})
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn typed_and_json_observations_enforce_device_and_uuid_bounds() {
    for id in [String::new(), "x".repeat(129), "bad\nidentity".into()] {
        assert_eq!(
            BleAdvertisement::new(id.clone(), -42, vec![])
                .unwrap_err()
                .to_string(),
            "invalid native BLE device identifier"
        );
        let encoded = serde_json::json!({"BleAdvertisementObserved": {"id":id,"rssi":-42,"service_uuids":[]}}).to_string();
        assert_eq!(
            event_from_json(&encoded).unwrap_err(),
            vauchi_core::platform::EventJsonError::Malformed
        );
    }
    let valid = BleAdvertisement::new("x".repeat(128), -42, vec![service(); 32]).unwrap();
    assert_eq!(valid.service_uuids().len(), 32);
    assert_eq!(
        BleAdvertisement::new("device".into(), -42, vec![service(); 33])
            .unwrap_err()
            .to_string(),
        "BLE advertisement has more than 32 service UUIDs"
    );
    let encoded = serde_json::json!({"BleAdvertisementObserved": {"id":"device","rssi":-42,"service_uuids":vec![service();33]}}).to_string();
    assert_eq!(
        event_from_json(&encoded).unwrap_err(),
        vauchi_core::platform::EventJsonError::Malformed
    );
}

proptest::proptest! {
    // @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
    #[test]
    fn arbitrary_token_bytes_are_preserved(value in proptest::prelude::any::<u32>()) {
        let token = uuid::Uuid::from_fields(value, 0, 0x1000, &[0x80,0,0,0x80,0x5f,0x9b,0x34,0xfb]);
        let bytes = value.to_be_bytes();
        let expected: Vec<u8> = bytes.into_iter().skip(if value <= u16::MAX.into() {2} else {0}).collect();
        proptest::prop_assert_eq!(advertised_token(&[service(), *token.as_bytes()]), Some(expected));
    }
}
