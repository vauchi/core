// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression: a card UPDATE round-trips over a REAL in-person (QR) exchange.
//!
//! `exchanged_ratchet_roundtrip_tests` proves a real-X3DH mutual-QR ratchet
//! round-trips DIRECT messages on the in-memory ratchet objects. This pins the
//! layer the device sync actually uses but that file does not:
//!
//!   1. the ratchet is **persisted then reloaded** through the production save
//!      seam (`save_exchanged_contact`), as it is on a device between the
//!      exchange and the later sync, and
//!   2. the payload crosses the full **card-update pipeline**
//!      (`prepare_card_update_for_contact` → `process_card_update`: CEK wrap,
//!      signature binding, delta/replay), not a raw `encrypt`/`decrypt`.
//!
//! Every existing card-update round-trip test either uses the SYNTHETIC
//! `setup_ratchets` helper (`SymmetricKey::generate()`, never the X3DH
//! agreement — `exchange_to_update_e2e_test`, `repeat_exchange_rekey_tests`)
//! or the link-mode path; the regular QR/USB exchange + card-update path is a
//! coverage gap. The on-device symptom (`sync.receive_phase received=0
//! rejected=N` — blobs delivered + token-routed but never decrypt) reproduces
//! here with zero relay and zero CLI if this layer is the cause.
//!
//! Problem record: 2026-06-28-sync-delivery-sent-not-received (step 2).
//! Feature: features/sync_updates.feature, features/contact_exchange.feature @qr-mutual

use crate::common;

use common::helpers::create_vauchi_with_card;
use vauchi_core::clock::SystemClock;
use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, MockProximityVerifier};
use vauchi_core::{FieldType, Identity, Vauchi};

/// Duplicate a Vauchi's identity into an owned copy (`Identity` is not `Clone`)
/// so it can drive an `ExchangeSession`, which consumes an owned `Identity`.
/// Mirrors the CLI `exchange complete` export/import dance
/// (cli/src/commands/exchange.rs) — the seam is identical to a device's.
fn owned_identity_copy(wb: &Vauchi) -> Identity {
    let id = wb.identity().expect("identity present");
    let password = "Str0ng-Test-Passw0rd!";
    let backup = id.export_backup(password).expect("export backup");
    Identity::import_backup(&backup, password, 0).expect("import backup")
}

// @scenario: sync_updates :: A card update propagates after an in-person exchange
#[test]
fn in_person_exchange_card_update_round_trips() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@old.com")]);

    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    // ── Drive two real ExchangeSessions through a full mutual-QR exchange to
    //    Complete (mirrors exchanged_ratchet_roundtrip_tests).
    let mut alice_session = ExchangeSession::new_qr(
        owned_identity_copy(&alice),
        alice_card.clone(),
        MockProximityVerifier::success(),
        SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        owned_identity_copy(&bob),
        bob_card.clone(),
        MockProximityVerifier::success(),
        SystemClock::shared(),
    );

    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();
    let bob_qr = bob_session.qr().unwrap().clone();
    alice_session
        .apply(ExchangeEvent::ProcessQR(bob_qr))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card.clone()))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card.clone()))
        .unwrap();

    let bob_at_alice = alice_session
        .extract_contact()
        .expect("alice reached Complete");
    let alice_at_bob = bob_session.extract_contact().expect("bob reached Complete");
    let bob_id = bob_at_alice.id().to_string();
    let alice_id = alice_at_bob.id().to_string();

    let (alice_ratchet, alice_is_initiator) = alice_session
        .build_exchange_ratchet(&bob_at_alice)
        .expect("alice ratchet builds");
    let (bob_ratchet, bob_is_initiator) = bob_session
        .build_exchange_ratchet(&alice_at_bob)
        .expect("bob ratchet builds");
    assert_ne!(
        alice_is_initiator, bob_is_initiator,
        "exactly one side must be the initiator"
    );

    // ── Persist through the real save seam — this serialises the ratchet to
    //    storage and reloads it on prepare/process, exactly as a device does
    //    between the exchange and the later sync.
    alice
        .save_exchanged_contact(&bob_at_alice, &alice_ratchet, alice_is_initiator)
        .unwrap();
    bob.save_exchanged_contact(&alice_at_bob, &bob_ratchet, bob_is_initiator)
        .unwrap();

    // ── Card update. The responder has no sending chain until it receives the
    //    initiator's first message, so the INITIATOR sends first (the device
    //    "initial card" + first sync). This is the exact "Alice edits her
    //    email" scenario that showed received=0 rejected=N on devices.
    if alice_is_initiator {
        common::helpers::assert_card_update_round_trips(&alice, &bob, &bob_id, &alice_id);
    } else {
        common::helpers::assert_card_update_round_trips(&bob, &alice, &alice_id, &bob_id);
    }
}
