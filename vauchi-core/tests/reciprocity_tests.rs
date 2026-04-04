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
