// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::reciprocity::{ConfirmationChannel, Reciprocity};
use vauchi_core::storage::Storage;

#[test]
fn reciprocity_serde_roundtrip() {
    for variant in [
        Reciprocity::Confirmed,
        Reciprocity::Pending,
        Reciprocity::Unreciprocated,
        Reciprocity::Unknown,
    ] {
        let json = serde_json::to_string(&variant).expect("serialize");
        let back: Reciprocity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(variant, back, "roundtrip failed for {json}");
    }
}

#[test]
fn confirmation_channel_serde_roundtrip() {
    for variant in [
        ConfirmationChannel::Audio,
        ConfirmationChannel::Ble,
        ConfirmationChannel::RelayEscrow,
        ConfirmationChannel::RelaySync,
    ] {
        let json = serde_json::to_string(&variant).expect("serialize");
        let back: ConfirmationChannel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(variant, back, "roundtrip failed for {json}");
    }
}

#[test]
fn reciprocity_snake_case_serialization() {
    assert_eq!(
        serde_json::to_string(&Reciprocity::Unreciprocated).unwrap(),
        "\"unreciprocated\""
    );
    assert_eq!(
        serde_json::to_string(&ConfirmationChannel::RelayEscrow).unwrap(),
        "\"relay_escrow\""
    );
}

fn make_test_contact() -> Contact {
    Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    )
}

#[test]
fn contact_reciprocity_defaults_to_unknown() {
    let contact = make_test_contact();
    assert_eq!(contact.reciprocity(), Reciprocity::Unknown);
}

#[test]
fn contact_set_reciprocity() {
    let mut contact = make_test_contact();
    contact.set_reciprocity(Reciprocity::Confirmed);
    assert_eq!(contact.reciprocity(), Reciprocity::Confirmed);
}

#[test]
fn contact_set_confirmation_channel() {
    let mut contact = make_test_contact();
    contact.set_confirmation_channel(ConfirmationChannel::RelayEscrow);
    assert_eq!(
        contact.confirmation_channel(),
        Some(ConfirmationChannel::RelayEscrow)
    );
}

#[test]
fn reciprocity_storage_roundtrip() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = make_test_contact();
    let id = contact.id().to_string();

    contact.set_reciprocity(Reciprocity::Pending);
    contact.set_confirmation_channel(ConfirmationChannel::RelayEscrow);
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded.reciprocity(), Reciprocity::Pending);
    assert_eq!(
        loaded.confirmation_channel(),
        Some(ConfirmationChannel::RelayEscrow)
    );
}

#[test]
fn reciprocity_storage_null_defaults_to_unknown() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = make_test_contact();
    let id = contact.id().to_string();

    // Save without setting reciprocity
    storage.save_contact(&contact).unwrap();

    let loaded = storage.load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded.reciprocity(), Reciprocity::Unknown);
    assert_eq!(loaded.confirmation_channel(), None);
}

// ── Task 4: Confirmation escrow key derivation ──

use vauchi_core::exchange::confirmation_escrow::ConfirmationEscrowKeys;
use vauchi_core::exchange::escrow::EscrowRole;

#[test]
fn confirmation_escrow_keys_derive_produces_different_slots() {
    let shared_secret = [42u8; 32];
    let keys = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    assert_ne!(keys.our_slot, keys.their_slot);
    assert!(!keys.gate_hash.is_empty());
}

#[test]
fn confirmation_escrow_keys_roles_are_symmetric() {
    let shared_secret = [42u8; 32];
    let init = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    let resp = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Responder);
    // Same gate
    assert_eq!(init.gate_hash, resp.gate_hash);
    // Swapped slots
    assert_eq!(init.our_slot, resp.their_slot);
    assert_eq!(init.their_slot, resp.our_slot);
}

#[test]
fn confirmation_escrow_keys_differ_from_card_escrow() {
    use vauchi_core::exchange::escrow::EscrowKeys;
    let shared_secret = [42u8; 32];
    let card = EscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    let confirm = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    assert_ne!(card.gate_hash, confirm.gate_hash);
}

// ── Task 5: Token derivation in key agreement ──

use vauchi_core::exchange::proximity::MockProximityVerifier;
use vauchi_core::exchange::session::{ExchangeEvent, ExchangeSession};
use vauchi_core::identity::Identity;

#[test]
fn key_agreement_derives_confirmation_tokens() {
    let identity_a = Identity::create("Alice");
    let card_a = ContactCard::new("Alice");
    let mut session_a =
        ExchangeSession::new_qr(identity_a, card_a, MockProximityVerifier::success());

    let identity_b = Identity::create("Bob");
    let card_b = ContactCard::new("Bob");
    let mut session_b =
        ExchangeSession::new_qr(identity_b, card_b, MockProximityVerifier::success());

    // Both start QR
    session_a.apply(ExchangeEvent::StartQR).unwrap();
    session_b.apply(ExchangeEvent::StartQR).unwrap();

    // A scans B's QR
    let qr_b = session_b.qr().unwrap().clone();
    session_a.apply(ExchangeEvent::ProcessQR(qr_b)).unwrap();
    session_a.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Before key agreement, tokens should be None
    assert!(session_a.our_confirmation_token().is_none());

    // Perform key agreement
    session_a.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    // After key agreement, tokens should be populated
    let our_token = session_a
        .our_confirmation_token()
        .expect("our_confirmation_token should be set after key agreement");
    let their_token = session_a
        .expected_their_token()
        .expect("expected_their_token should be set after key agreement");

    // Tokens must be different (asymmetric — bound to different identity keys)
    assert_ne!(our_token, their_token);

    // Escrow keys should also be populated
    let (gate, our_slot, their_slot) = session_a
        .confirmation_escrow()
        .expect("confirmation escrow should be set after key agreement");
    assert!(!gate.is_empty());
    assert_ne!(our_slot, their_slot);
}
