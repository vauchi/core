// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Own-card repropagation retry queue
//! (2026-06-14-own-card-propagation-retry-queue).
//!
//! An own-card edit sets a durable marker; the sync loop runs a group-aware
//! repropagation pass (via `repropagate_to_contact`) and clears the marker only
//! once every contact's update has been queued. A permanent error backs off via
//! `failed_attempts` instead of hot-looping. These tests cover the durable
//! marker; the pass + sync wiring are covered alongside the implementation.

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, OwnCardRepropagateState,
};

/// Alice's `Vauchi` plus an exchanged, ratcheted contact "Bob" (so
/// `repropagate_to_contact` can encrypt and queue).
fn alice_with_ratcheted_bob() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let bob = Identity::create("Bob", 0);
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *bob.signing_public_key(),
        ContactCard::new("Bob"),
        shared.clone(),
        0,
    );
    let bob_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&bob_id, &shared, *their_dh.public_key())
        .unwrap();

    (wb, bob_id)
}

fn pending_count(wb: &Vauchi, contact_id: &str) -> usize {
    wb.storage()
        .pending()
        .get_pending_updates(contact_id)
        .unwrap()
        .len()
}

// @internal
#[test]
fn own_card_repropagate_marker_roundtrips_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let db = dir.path().join("vauchi.db");

    // Fresh DB → the marker defaults to "not owed".
    {
        let config = VauchiConfig::with_storage_path(&db).with_storage_key(key.clone());
        let vauchi = Vauchi::new(config).unwrap();
        let state = vauchi.storage().ux().load_own_card_repropagate().unwrap();
        assert_eq!(state, OwnCardRepropagateState::default());
        assert!(!state.needs_repropagate, "fresh DB owes no repropagation");
        assert_eq!(state.failed_attempts, 0);

        let dirty = OwnCardRepropagateState {
            needs_repropagate: true,
            failed_attempts: 3,
        };
        vauchi
            .storage()
            .ux()
            .save_own_card_repropagate(&dirty)
            .unwrap();
    }

    // Reopen the same DB → the persisted marker round-trips exactly.
    {
        let config = VauchiConfig::with_storage_path(&db).with_storage_key(key);
        let vauchi = Vauchi::new(config).unwrap();
        let state = vauchi.storage().ux().load_own_card_repropagate().unwrap();
        assert!(state.needs_repropagate, "dirty marker survives reopen");
        assert_eq!(state.failed_attempts, 3, "failed_attempts survives reopen");
        assert!(state.should_run(), "owed + under cap → runs");
    }
}

// @internal
#[test]
fn own_card_repropagate_backs_off_at_cap() {
    let at_cap = OwnCardRepropagateState {
        needs_repropagate: true,
        failed_attempts: OwnCardRepropagateState::MAX_FAILED_ATTEMPTS,
    };
    assert!(
        !at_cap.should_run(),
        "at the retry cap the marker backs off (no hot-loop)"
    );

    let under_cap = OwnCardRepropagateState {
        needs_repropagate: true,
        failed_attempts: OwnCardRepropagateState::MAX_FAILED_ATTEMPTS - 1,
    };
    assert!(under_cap.should_run(), "one below the cap still runs");

    assert!(
        !OwnCardRepropagateState::default().should_run(),
        "not owed → does not run"
    );
}

// @scenario: visibility_control :: An own-card edit is repropagated to contacts
#[test]
fn own_card_edit_sets_repropagate_marker() {
    let (wb, _bob) = alice_with_ratcheted_bob();
    assert!(
        !wb.storage()
            .ux()
            .load_own_card_repropagate()
            .unwrap()
            .needs_repropagate,
        "no edit yet → marker not owed"
    );

    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();

    let state = wb.storage().ux().load_own_card_repropagate().unwrap();
    assert!(
        state.needs_repropagate,
        "an own-card edit marks the card for repropagation"
    );
    assert_eq!(
        state.failed_attempts, 0,
        "a fresh edit resets the retry budget"
    );
}

// @scenario: visibility_control :: An own-card edit is repropagated to contacts
#[test]
fn pass_queues_repropagation_then_clears_marker_on_success() {
    let (wb, bob) = alice_with_ratcheted_bob();

    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    assert_eq!(
        pending_count(&wb, &bob),
        0,
        "the edit alone queues nothing — the pass does the propagation"
    );

    wb.run_owed_repropagation().unwrap();

    assert_eq!(
        pending_count(&wb, &bob),
        1,
        "the pass queues exactly one card delta to the ratcheted contact"
    );
    let state = wb.storage().ux().load_own_card_repropagate().unwrap();
    assert!(
        !state.needs_repropagate,
        "the marker clears after a fully successful pass"
    );
    assert_eq!(state.failed_attempts, 0);
}

// @internal
#[test]
fn pass_is_noop_when_nothing_owed() {
    let (wb, bob) = alice_with_ratcheted_bob();
    // No edit → marker not owed.
    wb.run_owed_repropagation().unwrap();
    assert_eq!(
        pending_count(&wb, &bob),
        0,
        "no pass runs when no edit is owed"
    );
}

// @internal
#[test]
fn pass_skips_contact_without_ratchet_and_still_clears() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    // An exchanged contact with NO ratchet established.
    let bob = Identity::create("Bob", 0);
    let contact = Contact::from_exchange(
        *bob.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let bob_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    wb.run_owed_repropagation().unwrap();

    assert_eq!(
        pending_count(&wb, &bob_id),
        0,
        "a contact without a ratchet is skipped (no queue)"
    );
    assert!(
        !wb.storage()
            .ux()
            .load_own_card_repropagate()
            .unwrap()
            .needs_repropagate,
        "skipping a no-ratchet contact is a success, not a failure — marker clears"
    );
}

// @scenario: visibility_control :: An own-card edit is repropagated to contacts
#[test]
fn pass_backed_off_at_cap_does_not_run() {
    let (wb, bob) = alice_with_ratcheted_bob();
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();

    // Simulate a marker that has exhausted its retry budget.
    wb.storage()
        .ux()
        .save_own_card_repropagate(&OwnCardRepropagateState {
            needs_repropagate: true,
            failed_attempts: OwnCardRepropagateState::MAX_FAILED_ATTEMPTS,
        })
        .unwrap();

    wb.run_owed_repropagation().unwrap();
    assert_eq!(
        pending_count(&wb, &bob),
        0,
        "a backed-off marker does not run the pass (no hot-loop)"
    );
}
