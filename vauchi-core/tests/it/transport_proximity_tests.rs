// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::{TransportProximity, TrustMetrics, VerifierEventLog, VerifierMethod};
use vauchi_core::{ExchangeTransport, ProximityConfidence};

// @internal
#[test]
fn transport_proximity_for_usb_is_physical() {
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Usb),
        TransportProximity::Physical
    );
}

// @internal
#[test]
fn transport_proximity_for_nfc_is_contact_range() {
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Nfc),
        TransportProximity::ContactRange
    );
}

// @internal
#[test]
fn transport_proximity_for_ble_is_proximate() {
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Ble),
        TransportProximity::Proximate
    );
}

// @internal
#[test]
fn transport_proximity_for_qr_is_none() {
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Qr),
        TransportProximity::None
    );
}

// @internal
#[test]
fn transport_proximity_for_audio_is_none() {
    assert_eq!(
        TransportProximity::for_transport(ExchangeTransport::Audio),
        TransportProximity::None
    );
}

// @internal
#[test]
fn trust_metrics_serde_roundtrip() {
    let metrics = TrustMetrics {
        transport: ExchangeTransport::Ble,
        proximity: ProximityConfidence::High,
        verifier_method: Some(VerifierMethod::Ultrasonic),
        verifier_log: VerifierEventLog::new(),
        transport_proximity: TransportProximity::Proximate,
        timestamp: 1711324800,
    };

    let json = serde_json::to_string(&metrics).expect("serialize");
    let deserialized: TrustMetrics = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(deserialized.transport, ExchangeTransport::Ble);
    assert_eq!(deserialized.proximity, ProximityConfidence::High);
    assert_eq!(
        deserialized.verifier_method,
        Some(VerifierMethod::Ultrasonic)
    );
    assert_eq!(
        deserialized.transport_proximity,
        TransportProximity::Proximate
    );
    assert_eq!(deserialized.timestamp, 1711324800);
}

// @internal
#[test]
fn trust_metrics_strong_transport_is_strong() {
    assert!(TransportProximity::Physical.is_strong());
    assert!(TransportProximity::ContactRange.is_strong());
    assert!(!TransportProximity::Proximate.is_strong());
    assert!(!TransportProximity::None.is_strong());
}
