// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! T0 + T0.5: Transport confidence correctness tests.
//!
//! Verifies that each transport assigns the correct ProximityConfidence level:
//!   NFC  → High  (physical tap IS the proximity proof)
//!   BLE  → Medium (RSSI is a heuristic, not a proof)
//!   Ultrasonic → High (cryptographic challenge-response)
//!   Manual → Medium (user assertion)
//!
//! Also verifies NFC is correctly represented in transport negotiation.

#![cfg(feature = "testing")]

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::transport::negotiation::negotiate_transport;
use vauchi_core::exchange::transport::{TransportCaps, TransportType};
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeSession, ExchangeState, MockBLEVerifier, MockProximityVerifier,
    ProximityConfidence, ProximityVerifier,
};
use vauchi_core::identity::Identity;

// ===== B1: BLE confidence must be Medium, not High =====

/// BLE RSSI estimates distance but does not cryptographically prove proximity.
/// RSSI can be amplified, relayed, or is inaccurate through walls.
/// BLE verifiers must return Medium confidence, requiring manual confirmation.
// @internal
#[test]
fn ble_verifier_confidence_is_medium_not_high() {
    let verifier = MockBLEVerifier::success_at_distance(1.0);
    assert_eq!(
        verifier.confidence_level(),
        ProximityConfidence::Medium,
        "BLE RSSI is a heuristic — confidence must be Medium, not High"
    );
}

// @internal
#[test]
fn ble_verifier_failure_still_reports_medium_confidence() {
    let verifier = MockBLEVerifier::failure();
    assert_eq!(
        verifier.confidence_level(),
        ProximityConfidence::Medium,
        "BLE verifier capability is Medium regardless of success/failure"
    );
}

// ===== B2: NFC must appear in TRANSPORT_MAP negotiation =====

/// When both peers advertise NFC_TRIGGER, negotiate_transport() must select NFC.
/// NFC was previously absent from TRANSPORT_MAP entirely.
// @internal
#[test]
fn nfc_selected_when_both_peers_have_nfc_trigger() {
    let ours = TransportCaps::NFC_TRIGGER | TransportCaps::STATIC_QR;
    let theirs = TransportCaps::NFC_TRIGGER | TransportCaps::STATIC_QR;
    assert_eq!(
        negotiate_transport(&ours, &theirs),
        TransportType::Nfc,
        "NFC must be selectable via negotiate_transport()"
    );
}

/// NFC must be preferred over BLE when both are available.
/// Physical tap beats RSSI heuristic.
// @internal
#[test]
fn nfc_preferred_over_ble_in_negotiation() {
    let ours = TransportCaps::NFC_TRIGGER | TransportCaps::BLE | TransportCaps::STATIC_QR;
    let theirs = TransportCaps::NFC_TRIGGER | TransportCaps::BLE | TransportCaps::STATIC_QR;
    assert_eq!(
        negotiate_transport(&ours, &theirs),
        TransportType::Nfc,
        "NFC (physical tap) must be preferred over BLE (RSSI heuristic)"
    );
}

/// TCP must still be preferred over NFC (TCP has higher throughput for data).
// @internal
#[test]
fn tcp_still_preferred_over_nfc() {
    let ours = TransportCaps::TCP | TransportCaps::NFC_TRIGGER;
    let theirs = TransportCaps::TCP | TransportCaps::NFC_TRIGGER;
    assert_eq!(
        negotiate_transport(&ours, &theirs),
        TransportType::Tcp,
        "TCP must remain higher priority than NFC"
    );
}

// ===== B3: NFC priority must be above BLE =====

/// NFC provides physical adjacency proof — it must have higher priority than BLE.
// @internal
#[test]
fn nfc_priority_above_ble() {
    assert!(
        TransportType::Nfc.priority() > TransportType::Ble.priority(),
        "NFC priority ({}) must be > BLE priority ({})",
        TransportType::Nfc.priority(),
        TransportType::Ble.priority()
    );
}

/// NFC must sit between TCP and BLE in the priority chain.
// @internal
#[test]
fn nfc_priority_between_tcp_and_ble() {
    let nfc = TransportType::Nfc.priority();
    let tcp = TransportType::Tcp.priority();
    let ble = TransportType::Ble.priority();
    assert!(
        ble < nfc && nfc < tcp,
        "Priority chain must be BLE({}) < NFC({}) < TCP({})",
        ble,
        nfc,
        tcp
    );
}

// ===== B4: NFC must skip proximity verifier and set High directly =====

/// NFC tap IS the proximity proof. The code must not run a separate
/// ProximityVerifier on top of an NFC exchange. Even if the verifier
/// would fail (e.g., no audio hardware), NFC should complete with High.
// @internal
#[test]
fn nfc_exchange_sets_high_confidence_without_running_verifier() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    // Pass a FAILING proximity verifier — if NFC runs it, the test will
    // detect wrong confidence. NFC must skip the verifier entirely.
    let failing_verifier = MockProximityVerifier::failure();
    let mut session = ExchangeSession::new_nfc(
        alice,
        alice_card,
        failing_verifier,
        vauchi_core::clock::SystemClock::shared(),
    );

    let bob_ephemeral = vauchi_core::exchange::X3DHKeyPair::generate();
    let bob_nfc = vauchi_core::exchange::ExchangeNfc::generate(
        &bob,
        &bob_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );
    let bob_payload = bob_nfc.to_bytes().to_vec();

    // NFC tap delivers Bob's payload
    let tap_result = session.apply(ExchangeEvent::NfcTapComplete {
        their_payload: bob_payload,
    });
    assert!(tap_result.is_ok(), "NFC tap should succeed");

    // even though the proximity verifier would fail
    let ka_result = session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        ka_result.is_ok(),
        "Key agreement must succeed for NFC despite failing verifier"
    );

    assert!(
        matches!(session.state(), ExchangeState::AwaitingCardExchange { .. }),
        "NFC should reach AwaitingCardExchange state, got {:?}",
        session.state()
    );

    // Complete the exchange with Bob's card to check final confidence
    let complete_result = session.apply(ExchangeEvent::CompleteExchange(bob_card));
    assert!(
        complete_result.is_ok(),
        "Exchange completion should succeed"
    );

    // The contact should have High confidence — NFC tap proves adjacency
    match session.state() {
        ExchangeState::Complete { contact } => {
            assert_eq!(
                *contact.proximity_confidence(),
                ProximityConfidence::High,
                "NFC exchange must set High confidence (tap = adjacency proof), \
                 not {:?} from running the failing verifier",
                contact.proximity_confidence()
            );
        }
        other => panic!("Expected Complete state, got {:?}", other),
    }
}
