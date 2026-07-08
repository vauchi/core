// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{TransportProximity, TrustMetrics};
use vauchi_core::{ExchangeTransport, ProximityConfidence};

fn make_contact(mutate: impl FnOnce(&mut Contact)) -> Contact {
    let mut c = Contact::from_exchange(
        [42u8; 32],
        ContactCard::new("Prop"),
        SymmetricKey::generate(),
        0,
    );
    mutate(&mut c);
    c
}

fn arb_exchange_transport() -> impl Strategy<Value = ExchangeTransport> {
    prop_oneof![
        Just(ExchangeTransport::Qr),
        Just(ExchangeTransport::Nfc),
        Just(ExchangeTransport::Ble),
        Just(ExchangeTransport::Usb),
        Just(ExchangeTransport::Audio),
    ]
}

fn arb_proximity_confidence() -> impl Strategy<Value = ProximityConfidence> {
    prop_oneof![
        Just(ProximityConfidence::High),
        Just(ProximityConfidence::Medium),
        Just(ProximityConfidence::Low),
        Just(ProximityConfidence::Unknown),
    ]
}

proptest! {
// @internal
    #[test]
    fn trust_metrics_serde_roundtrip(
        transport in arb_exchange_transport(),
        proximity in arb_proximity_confidence(),
        timestamp in 0u64..u64::MAX,
    ) {
        let metrics = TrustMetrics::new(transport, proximity, timestamp);
        let json = serde_json::to_string(&metrics).unwrap();
        let deserialized: TrustMetrics = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(deserialized.transport, transport);
        prop_assert_eq!(deserialized.proximity, proximity);
        prop_assert_eq!(deserialized.timestamp, timestamp);
        prop_assert_eq!(
            deserialized.transport_proximity,
            TransportProximity::for_transport(transport)
        );
    }

// @internal
    #[test]
    fn transport_proximity_derivation_is_deterministic(
        transport in arb_exchange_transport(),
    ) {
        let a = TransportProximity::for_transport(transport);
        let b = TransportProximity::for_transport(transport);
        prop_assert_eq!(a, b);
    }

// @internal
    #[test]
    fn strong_transport_always_gives_high_trust_without_recovery(
        transport in prop_oneof![Just(ExchangeTransport::Usb), Just(ExchangeTransport::Nfc)],
        proximity in arb_proximity_confidence(),
    ) {
        use vauchi_core::contact::trust::TrustLevel;

        let metrics = TrustMetrics::new(transport, proximity, 0);
        prop_assert!(metrics.transport_proximity.is_strong());

        let mut contact = make_contact(|_| {});
        contact.set_trust_metrics(Some(metrics));
        prop_assert_eq!(contact.trust_level(), TrustLevel::High);
    }
}

// --- CC-14: Adversarial deserialization tests for TrustMetrics ---

// @internal
#[test]
fn trust_metrics_rejects_empty_json() {
    let result = serde_json::from_str::<TrustMetrics>("");
    assert!(result.is_err());
}

// @internal
#[test]
fn trust_metrics_rejects_null() {
    let result = serde_json::from_str::<TrustMetrics>("null");
    assert!(result.is_err());
}

// @internal
#[test]
fn trust_metrics_rejects_truncated_json() {
    let metrics = TrustMetrics::new(ExchangeTransport::Ble, ProximityConfidence::High, 0);
    let json = serde_json::to_string(&metrics).unwrap();
    let truncated = &json[..json.len() / 2];
    let result = serde_json::from_str::<TrustMetrics>(truncated);
    assert!(result.is_err());
}

// @internal
#[test]
fn trust_metrics_unknown_transport_variant() {
    let json =
        r#"{"transport":"quantum","proximity":"high","transport_proximity":"none","timestamp":0}"#;
    let result = serde_json::from_str::<TrustMetrics>(json);
    assert!(result.is_err());
}
