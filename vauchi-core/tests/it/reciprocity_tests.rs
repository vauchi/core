// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::reciprocity::{ConfirmationChannel, Reciprocity};
use vauchi_core::storage::Storage;

// @internal
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

// @internal
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

// @internal
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
        0,
    )
}

// @internal
#[test]
fn contact_reciprocity_defaults_to_unknown() {
    let contact = make_test_contact();
    assert_eq!(contact.reciprocity(0), Reciprocity::Unknown);
}

// @internal
#[test]
fn contact_set_reciprocity() {
    let mut contact = make_test_contact();
    contact.set_reciprocity(Reciprocity::Confirmed);
    assert_eq!(contact.reciprocity(0), Reciprocity::Confirmed);
}

// @internal
#[test]
fn contact_set_confirmation_channel() {
    let mut contact = make_test_contact();
    contact.set_confirmation_channel(ConfirmationChannel::RelayEscrow);
    assert_eq!(
        contact.confirmation_channel(),
        Some(ConfirmationChannel::RelayEscrow)
    );
}

// @internal
#[test]
fn reciprocity_storage_roundtrip() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut contact = make_test_contact();
    let id = contact.id().to_string();

    contact.set_reciprocity(Reciprocity::Pending);
    contact.set_confirmation_channel(ConfirmationChannel::RelayEscrow);
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded.reciprocity(0), Reciprocity::Pending);
    assert_eq!(
        loaded.confirmation_channel(),
        Some(ConfirmationChannel::RelayEscrow)
    );
}

// @internal
#[test]
fn reciprocity_storage_null_defaults_to_unknown() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = make_test_contact();
    let id = contact.id().to_string();

    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage.contacts().load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded.reciprocity(0), Reciprocity::Unknown);
    assert_eq!(loaded.confirmation_channel(), None);
}

// ── Stretch: storage query + confirmation_state persistence ──

// @internal
#[test]
fn list_contacts_by_reciprocity_filters_correctly() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let mut c1 = make_contact_with_timestamp(1000);
    c1.set_reciprocity(Reciprocity::Pending);
    storage.contacts().save_contact(&c1).unwrap();

    let mut c2 = Contact::from_sync_data(
        [2u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        1000,
        false,
        VisibilityRules::new(),
    );
    c2.set_reciprocity(Reciprocity::Confirmed);
    storage.contacts().save_contact(&c2).unwrap();

    let pending = storage
        .contacts()
        .list_contacts_by_reciprocity("pending")
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].display_name(), "Alice");

    let confirmed = storage
        .contacts()
        .list_contacts_by_reciprocity("confirmed")
        .unwrap();
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].display_name(), "Bob");

    let empty = storage
        .contacts()
        .list_contacts_by_reciprocity("unknown")
        .unwrap();
    assert!(empty.is_empty());
}

// @internal
#[test]
fn confirmation_state_persistence_roundtrip() {
    use vauchi_app::ui::reciprocity_confirmer::ConfirmationState;

    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = make_test_contact();
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let state = ConfirmationState {
        our_token: [0xAA; 32],
        expected_their_token: [0xBB; 32],
        gate_hash: "gate123".to_string(),
        our_slot: "slot_a".to_string(),
        their_slot: "slot_b".to_string(),
        deposit_sent: true,
    };
    let state_bytes = serde_json::to_vec(&state).unwrap();
    storage
        .contacts()
        .update_confirmation_state(&id, &state_bytes)
        .unwrap();

    let loaded_bytes = storage
        .contacts()
        .load_confirmation_state(&id)
        .unwrap()
        .unwrap();
    let loaded: ConfirmationState = serde_json::from_slice(&loaded_bytes).unwrap();
    assert_eq!(loaded.our_token, [0xAA; 32]);
    assert_eq!(loaded.gate_hash, "gate123");
    assert!(loaded.deposit_sent);
}

// @internal
#[test]
fn confirmation_state_none_for_new_contact() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let contact = make_test_contact();
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();

    let loaded = storage.contacts().load_confirmation_state(&id).unwrap();
    assert!(loaded.is_none());
}

// ── Task 4: Confirmation escrow key derivation ──

use vauchi_core::exchange::confirmation_escrow::ConfirmationEscrowKeys;
use vauchi_core::exchange::escrow::EscrowRole;

// @internal
#[test]
fn confirmation_escrow_keys_derive_produces_different_slots() {
    let shared_secret = [42u8; 32];
    let keys = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    assert_ne!(keys.our_slot, keys.their_slot);
    assert!(!keys.gate_hash.is_empty());
}

// @internal
#[test]
fn confirmation_escrow_keys_roles_are_symmetric() {
    let shared_secret = [42u8; 32];
    let init = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    let resp = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Responder);
    assert_eq!(init.gate_hash, resp.gate_hash);
    assert_eq!(init.our_slot, resp.their_slot);
    assert_eq!(init.their_slot, resp.our_slot);
}

// @internal
#[test]
fn confirmation_escrow_keys_differ_from_card_escrow() {
    use vauchi_core::exchange::escrow::EscrowKeys;
    let shared_secret = [42u8; 32];
    let card = EscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    let confirm = ConfirmationEscrowKeys::derive(&shared_secret, EscrowRole::Initiator);
    assert_ne!(card.gate_hash, confirm.gate_hash);
}

// ── Task 10: Passive timer ──

use vauchi_core::contact::VisibilityRules;

fn make_contact_with_timestamp(ts: u64) -> Contact {
    Contact::from_sync_data(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        ts,
        false,
        VisibilityRules::new(),
    )
}

// @internal
#[test]
fn reciprocity_pending_within_7_days_stays_pending() {
    let now = 1_700_000_000;
    let mut contact = make_contact_with_timestamp(now - 3600); // 1 hour ago
    contact.set_reciprocity(Reciprocity::Pending);
    assert_eq!(contact.reciprocity(now), Reciprocity::Pending);
}

// @internal
#[test]
fn reciprocity_pending_expires_after_7_days() {
    let now = 1_700_000_000;
    let mut contact = make_contact_with_timestamp(now - 8 * 86400); // 8 days ago
    contact.set_reciprocity(Reciprocity::Pending);
    assert_eq!(contact.reciprocity(now), Reciprocity::Unreciprocated);
}

// @internal
#[test]
fn unreciprocated_upgrades_to_confirmed_on_late_arrival() {
    let mut contact = make_test_contact();
    contact.set_reciprocity(Reciprocity::Unreciprocated);
    // Simulate receiving a valid ReciprocityConfirm after window
    contact.set_reciprocity(Reciprocity::Confirmed);
    assert_eq!(contact.reciprocity(0), Reciprocity::Confirmed);
}

// @internal
#[test]
fn confirmed_not_affected_by_timer() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut contact = make_contact_with_timestamp(now - 30 * 86400); // 30 days ago
    contact.set_reciprocity(Reciprocity::Confirmed);
    assert_eq!(contact.reciprocity(0), Reciprocity::Confirmed);
}

// ── Task 5: Token derivation in key agreement ──

use vauchi_core::exchange::{ExchangeEvent, ExchangeSession, MockProximityVerifier};
use vauchi_core::identity::Identity;

// @internal
#[test]
fn key_agreement_derives_confirmation_tokens() {
    let identity_a = Identity::create("Alice", 0);
    let card_a = ContactCard::new("Alice");
    let mut session_a = ExchangeSession::new_qr(
        identity_a,
        card_a,
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    let identity_b = Identity::create("Bob", 0);
    let card_b = ContactCard::new("Bob");
    let mut session_b = ExchangeSession::new_qr(
        identity_b,
        card_b,
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );

    session_a.apply(ExchangeEvent::StartQR).unwrap();
    session_b.apply(ExchangeEvent::StartQR).unwrap();

    // A scans B's QR
    let qr_b = session_b.qr().unwrap().clone();
    session_a.apply(ExchangeEvent::ProcessQR(qr_b)).unwrap();
    session_a.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    assert!(session_a.our_confirmation_token().is_none());

    session_a.apply(ExchangeEvent::PerformKeyAgreement).unwrap();

    let our_token = session_a
        .our_confirmation_token()
        .expect("our_confirmation_token should be set after key agreement");
    let their_token = session_a
        .expected_their_token()
        .expect("expected_their_token should be set after key agreement");

    // Tokens must be different (asymmetric — bound to different identity keys)
    assert_ne!(our_token, their_token);

    let (gate, our_slot, their_slot) = session_a
        .confirmation_escrow()
        .expect("confirmation escrow should be set after key agreement");
    assert!(!gate.is_empty());
    assert_ne!(our_slot, their_slot);
}
