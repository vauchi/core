// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mutation-coverage tests for `contact/mod.rs`.
//!
//! Kills missed mutants in Contact getters, setters, reciprocity timer,
//! proposal trust, archive/soft-delete, and CEK methods.

use vauchi_core::contact::{Contact, ImportSource};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::exchange::{ConfirmationChannel, Reciprocity, TrustMetrics, VerifierEventLog};
use vauchi_core::{ExchangeTransport, ProximityConfidence};

fn exchanged_contact(name: &str) -> Contact {
    Contact::from_exchange(
        [0xAA; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

fn imported_contact(name: &str) -> Contact {
    Contact::from_import(
        format!("contact-{name}"),
        ContactCard::new(name),
        ImportSource::Manual,
        None,
        0,
    )
}

// ============================================================
// is_imported
// ============================================================

// @internal
#[test]
fn exchanged_contact_is_not_imported() {
    let c = exchanged_contact("Alice");
    assert!(!c.is_imported(), "exchanged contact must return false");
}

// @internal
#[test]
fn imported_contact_is_imported() {
    let c = imported_contact("Bob");
    assert!(c.is_imported(), "imported contact must return true");
}

// ============================================================
// card_updated_at / set_card_updated_at
// ============================================================

// @internal
#[test]
fn card_updated_at_initially_none() {
    let c = exchanged_contact("Alice");
    assert_eq!(c.card_updated_at(), None);
}

// @internal
#[test]
fn set_card_updated_at_stores_value() {
    let mut c = exchanged_contact("Alice");
    c.set_card_updated_at(Some(1_700_000_000));
    assert_eq!(c.card_updated_at(), Some(1_700_000_000));
}

// @internal
#[test]
fn set_card_updated_at_clears_value() {
    let mut c = exchanged_contact("Alice");
    c.set_card_updated_at(Some(1_700_000_000));
    c.set_card_updated_at(None);
    assert_eq!(c.card_updated_at(), None);
}

// ============================================================
// proximity_confidence
// ============================================================

// @internal
#[test]
fn proximity_confidence_default_is_unknown() {
    let c = exchanged_contact("Alice");
    assert_eq!(*c.proximity_confidence(), ProximityConfidence::Unknown);
}

// @internal
#[test]
fn proximity_confidence_reflects_set_value() {
    let mut c = exchanged_contact("Alice");
    c.set_proximity_confidence(ProximityConfidence::High);
    assert_eq!(*c.proximity_confidence(), ProximityConfidence::High);
}

// ============================================================
// trust_metrics
// ============================================================

// @internal
#[test]
fn trust_metrics_initially_none() {
    let c = exchanged_contact("Alice");
    assert!(c.trust_metrics().is_none());
}

// @internal
#[test]
fn trust_metrics_returns_some_after_set() {
    let mut c = exchanged_contact("Alice");
    let metrics = TrustMetrics::new(
        ExchangeTransport::Qr,
        ProximityConfidence::Unknown,
        None,
        VerifierEventLog::new(),
        1_700_000_000,
    );
    c.set_trust_metrics(Some(metrics));
    assert!(c.trust_metrics().is_some());
    assert_eq!(c.trust_metrics().unwrap().transport, ExchangeTransport::Qr);
}

// ============================================================
// reciprocity — 7-day timer boundary tests
// ============================================================

// @internal
#[test]
fn reciprocity_pending_within_7_days_stays_pending() {
    let mut c = exchanged_contact("Alice");
    c.set_reciprocity(Reciprocity::Pending);

    // The exchange_timestamp is set to now by from_exchange,
    // so the 7-day window has NOT elapsed.
    assert_eq!(c.reciprocity(0), Reciprocity::Pending);
}

// @internal
#[test]
fn reciprocity_confirmed_stays_confirmed() {
    let mut c = exchanged_contact("Alice");
    c.set_reciprocity(Reciprocity::Confirmed);
    assert_eq!(c.reciprocity(0), Reciprocity::Confirmed);
}

// @internal
#[test]
fn reciprocity_none_returns_unknown() {
    // A freshly exchanged contact has reciprocity = None (legacy)
    let c = exchanged_contact("Alice");
    assert_eq!(c.reciprocity(0), Reciprocity::Unknown);
}

// @internal
#[test]
fn reciprocity_imported_returns_unknown() {
    let c = imported_contact("Bob");
    assert_eq!(c.reciprocity(0), Reciprocity::Unknown);
}

// ============================================================
// confirmation_channel / set_confirmation_channel
// ============================================================

// @internal
#[test]
fn confirmation_channel_initially_none() {
    let c = exchanged_contact("Alice");
    assert_eq!(c.confirmation_channel(), None);
}

// @internal
#[test]
fn set_confirmation_channel_stores_value() {
    let mut c = exchanged_contact("Alice");
    c.set_confirmation_channel(ConfirmationChannel::Audio);
    assert_eq!(c.confirmation_channel(), Some(ConfirmationChannel::Audio));
}

// ============================================================
// set_reciprocity
// ============================================================

// @internal
#[test]
fn set_reciprocity_stores_value() {
    let mut c = exchanged_contact("Alice");
    c.set_reciprocity(Reciprocity::Unreciprocated);
    assert_eq!(c.reciprocity(0), Reciprocity::Unreciprocated);
}

// ============================================================
// mark_fingerprint_unverified
// ============================================================

// @internal
#[test]
fn mark_fingerprint_unverified_clears_flag() {
    let mut c = exchanged_contact("Alice");
    c.mark_fingerprint_verified().unwrap();
    assert!(c.is_fingerprint_verified());

    c.mark_fingerprint_unverified().unwrap();
    assert!(
        !c.is_fingerprint_verified(),
        "should be false after mark_fingerprint_unverified"
    );
}

// @internal
#[test]
fn mark_fingerprint_unverified_fails_on_imported() {
    let mut c = imported_contact("Bob");
    c.mark_fingerprint_unverified()
        .expect_err("should fail on imported contact");
}

// ============================================================
// accept_recovery_with_card
// ============================================================

// @internal
#[test]
fn accept_recovery_with_card_updates_key_and_card() {
    let mut c = exchanged_contact("Alice");
    let old_id = c.id().to_string();

    let new_pk = [0xBB; 32];
    let new_key = SymmetricKey::generate();
    let new_card = ContactCard::new("Alice Updated");

    c.accept_recovery_with_card(new_pk, new_key, new_card, 0)
        .unwrap();

    assert_ne!(c.id(), old_id, "ID should change after recovery");
    assert_eq!(c.display_name(), "Alice Updated");
    assert!(c.has_recovered());
    assert!(!c.is_fingerprint_verified());
}

// @internal
#[test]
fn accept_recovery_with_card_fails_on_imported() {
    let mut c = imported_contact("Bob");
    let result = c.accept_recovery_with_card(
        [0xCC; 32],
        SymmetricKey::generate(),
        ContactCard::new("Bob"),
        0,
    );
    result.expect_err("should fail on imported contact");
}

// ============================================================
// trust_for_proposals / untrust_for_proposals
// ============================================================

// @internal
#[test]
fn trust_for_proposals_sets_flag() {
    let mut c = exchanged_contact("Alice");
    assert!(!c.is_proposal_trusted());

    c.trust_for_proposals().unwrap();
    assert!(c.is_proposal_trusted());
}

// @internal
#[test]
fn untrust_for_proposals_clears_flag() {
    let mut c = exchanged_contact("Alice");
    c.trust_for_proposals().unwrap();
    assert!(c.is_proposal_trusted());

    c.untrust_for_proposals().unwrap();
    assert!(!c.is_proposal_trusted());
}

// @internal
#[test]
fn trust_for_proposals_fails_on_imported() {
    let mut c = imported_contact("Bob");
    c.trust_for_proposals()
        .expect_err("should fail on imported contact");
}

// @internal
#[test]
fn untrust_for_proposals_fails_on_imported() {
    let mut c = imported_contact("Bob");
    c.untrust_for_proposals()
        .expect_err("should fail on imported contact");
}

// ============================================================
// Soft-delete: deleted_at, is_soft_deleted, soft_delete, undo_soft_delete
// ============================================================

// @internal
#[test]
fn deleted_at_returns_exact_timestamp() {
    let mut c = exchanged_contact("Alice");
    assert_eq!(c.deleted_at(), None);

    c.soft_delete(42);
    assert_eq!(c.deleted_at(), Some(42), "must return exact timestamp");
}

// @internal
#[test]
fn is_soft_deleted_true_after_delete() {
    let mut c = exchanged_contact("Alice");
    c.soft_delete(100);
    assert!(c.is_soft_deleted());
}

// @internal
#[test]
fn is_soft_deleted_false_initially() {
    let c = exchanged_contact("Alice");
    assert!(!c.is_soft_deleted());
}

// @internal
#[test]
fn soft_delete_then_undo_clears() {
    let mut c = exchanged_contact("Alice");
    c.soft_delete(100);
    assert!(c.is_soft_deleted());

    c.undo_soft_delete();
    assert!(!c.is_soft_deleted());
    assert_eq!(c.deleted_at(), None);
}

// ============================================================
// ============================================================

// @internal
#[test]
fn is_archived_false_initially() {
    let c = exchanged_contact("Alice");
    assert!(!c.is_archived());
}

// @internal
#[test]
fn archive_sets_flag_and_exact_timestamp() {
    let mut c = exchanged_contact("Alice");
    c.archive(999);
    assert!(c.is_archived());
    assert_eq!(c.archived_at(), Some(999), "must return exact timestamp");
}

// @internal
#[test]
fn archived_at_none_initially() {
    let c = exchanged_contact("Alice");
    assert_eq!(c.archived_at(), None);
}

// @internal
#[test]
fn unarchive_clears_both() {
    let mut c = exchanged_contact("Alice");
    c.archive(999);
    assert!(c.is_archived());
    assert_eq!(c.archived_at(), Some(999));

    c.unarchive();
    assert!(!c.is_archived());
    assert_eq!(c.archived_at(), None);
}

// ============================================================
// CEK: clear_cek
// ============================================================

// @internal
#[test]
fn clear_cek_removes_cek() {
    let mut c = exchanged_contact("Alice");
    let cek = ContentEncryptionKey::generate();
    c.set_cek(cek);
    assert!(c.cek().is_some(), "CEK should be set");

    c.clear_cek();
    assert!(c.cek().is_none(), "CEK should be cleared");
}
