// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local-only tests for the recovery API on `Vauchi`.
//!
//! Covers the parts of `vauchi-core/src/api/vauchi/recovery.rs` that
//! don't require a real relay: claim creation, voucher accumulation
//! into local progress, progress accessors, and pre-upload validation.
//! The relay-touching helpers (`upload_guardian_entries`,
//! `vouch_for_claim`, `upload_recovery_proof`'s success path) need a
//! mock-transport story that doesn't yet exist — they are exercised
//! only at the identity gate here.

use vauchi_core::Vauchi;
use vauchi_core::VauchiError;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::recovery::{
    RecoveryClaim, RecoveryProof, RecoverySettings, RecoveryVoucher, VerificationResult,
};

fn vauchi_with_identity(name: &str) -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

/// Build a real signed voucher from a fresh helper identity.
///
/// `claim` is the recovering user's claim; `helper` is the guardian
/// who is signing the voucher.
fn build_voucher(claim: &RecoveryClaim, helper: &Identity) -> RecoveryVoucher {
    RecoveryVoucher::create_from_claim(claim, helper.signing_keypair(), None, 0)
        .expect("fresh claim is not expired and helper != recovering identity")
}

// ============================================================
// create_recovery_claim
// ============================================================

// @internal
#[test]
fn create_recovery_claim_returns_claim_binding_old_and_new_pk() {
    let wb = vauchi_with_identity("Alice");
    let old_pk = [7u8; 32];

    let claim = wb.create_recovery_claim(&old_pk).unwrap();

    assert_eq!(
        claim.old_pk(),
        &old_pk,
        "claim's old_pk must equal argument"
    );
    assert_eq!(
        claim.new_pk(),
        wb.identity().unwrap().signing_public_key(),
        "claim's new_pk must equal current identity"
    );
    assert!(
        !claim.is_expired(1_700_000_000),
        "freshly minted claim must not be expired"
    );
}

// @internal
#[test]
fn create_recovery_claim_persists_progress_with_default_threshold() {
    let wb = vauchi_with_identity("Alice");
    let _ = wb.create_recovery_claim(&[1u8; 32]).unwrap();

    let progress = wb
        .get_recovery_progress()
        .unwrap()
        .expect("progress must be persisted after create");

    assert_eq!(progress.voucher_count(), 0, "no vouchers yet");
    // Default RecoverySettings::recovery_threshold == 3.
    assert_eq!(
        progress.threshold,
        RecoverySettings::default().recovery_threshold()
    );
    assert!(!progress.is_complete(), "no vouchers → not complete");
}

// @internal
#[test]
fn create_recovery_claim_honors_custom_threshold() {
    let wb = vauchi_with_identity("Alice");
    let custom = RecoverySettings::new(5, 2).unwrap();
    wb.storage()
        .recovery()
        .save_recovery_settings(&custom)
        .unwrap();

    let _ = wb.create_recovery_claim(&[2u8; 32]).unwrap();

    let progress = wb.get_recovery_progress().unwrap().unwrap();
    assert_eq!(
        progress.threshold, 5,
        "must use saved threshold, not default"
    );
}

// @internal
#[test]
fn create_recovery_claim_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();
    let result = wb.create_recovery_claim(&[0u8; 32]);

    assert!(matches!(result, Err(VauchiError::IdentityNotInitialized)));
}

// @internal
#[test]
fn create_recovery_claim_overwrites_previous_progress() {
    let wb = vauchi_with_identity("Alice");
    let first = wb.create_recovery_claim(&[1u8; 32]).unwrap();
    let second = wb.create_recovery_claim(&[2u8; 32]).unwrap();

    assert_ne!(first.old_pk(), second.old_pk());
    let progress = wb.get_recovery_progress().unwrap().unwrap();
    assert_eq!(
        progress.claim.old_pk(),
        &[2u8; 32],
        "second create must replace first claim in progress"
    );
}

// ============================================================
// get_recovery_progress
// ============================================================

// @internal
#[test]
fn get_recovery_progress_is_none_for_fresh_vauchi() {
    let wb = vauchi_with_identity("Alice");
    assert!(wb.get_recovery_progress().unwrap().is_none());
}

// ============================================================
// add_recovery_voucher
// ============================================================

// @internal
#[test]
fn add_recovery_voucher_appends_to_existing_progress() {
    let wb = vauchi_with_identity("Alice");
    let claim = wb.create_recovery_claim(&[3u8; 32]).unwrap();

    let helper = Identity::create("Bob", 0);
    let voucher = build_voucher(&claim, &helper);
    let voucher_bytes = voucher.to_bytes();

    let updated = wb.add_recovery_voucher(&voucher_bytes).unwrap();

    assert_eq!(updated.voucher_count(), 1, "must accept signed voucher");

    // still there.
    let reloaded = wb.get_recovery_progress().unwrap().unwrap();
    assert_eq!(reloaded.voucher_count(), 1);
}

// @internal
#[test]
fn add_recovery_voucher_errors_when_no_progress_in_flight() {
    let wb = vauchi_with_identity("Alice");

    let claim = RecoveryClaim::new(&[4u8; 32], wb.identity().unwrap().signing_public_key(), 0);
    let helper = Identity::create("Bob", 0);
    let voucher = build_voucher(&claim, &helper);

    let err = wb.add_recovery_voucher(&voucher.to_bytes()).unwrap_err();

    match err {
        VauchiError::InvalidState(msg) => {
            assert!(
                msg.contains("No recovery in progress"),
                "expected progress-missing error, got: {msg}"
            );
        }
        other => panic!("expected InvalidState, got {other:?}"),
    }
}

// @internal
#[test]
fn add_recovery_voucher_rejects_malformed_bytes() {
    let wb = vauchi_with_identity("Alice");
    let _ = wb.create_recovery_claim(&[5u8; 32]).unwrap();

    let err = wb.add_recovery_voucher(&[0xFF; 8]).unwrap_err();

    assert!(matches!(err, VauchiError::Serialization(_)));
}

// @internal
#[test]
fn add_recovery_voucher_accumulates_across_helpers() {
    let wb = vauchi_with_identity("Alice");
    let claim = wb.create_recovery_claim(&[6u8; 32]).unwrap();

    for name in ["Bob", "Carol", "Dave"] {
        let helper = Identity::create(name, 0);
        let voucher = build_voucher(&claim, &helper);
        wb.add_recovery_voucher(&voucher.to_bytes()).unwrap();
    }

    let progress = wb.get_recovery_progress().unwrap().unwrap();
    assert_eq!(progress.voucher_count(), 3);
    assert!(
        progress.is_complete(),
        "default threshold = 3 vouchers must mark progress complete"
    );
}

// ============================================================
// save_recovery_response_action
// ============================================================

// @internal
#[test]
fn save_recovery_response_action_persists_to_storage() {
    let wb = vauchi_with_identity("Alice");

    wb.save_recovery_response_action("claim-id-1", "contact-id-1", "accept", None)
        .unwrap();

    let row = wb
        .storage()
        .recovery()
        .get_recovery_response("claim-id-1")
        .unwrap()
        .expect("response must exist after save");

    let (contact_id, response, remind_at) = row;
    assert_eq!(contact_id, "contact-id-1");
    assert_eq!(response, "accept");
    assert!(remind_at.is_none(), "no remind_at was passed");
}

// @internal
#[test]
fn save_recovery_response_action_records_remind_at() {
    let wb = vauchi_with_identity("Alice");
    let remind = 1_700_000_000u64;

    wb.save_recovery_response_action("claim-2", "contact-2", "remind_me_later", Some(remind))
        .unwrap();

    let (contact_id, response, remind_at) = wb
        .storage()
        .recovery()
        .get_recovery_response("claim-2")
        .unwrap()
        .expect("response must exist after save");

    assert_eq!(contact_id, "contact-2");
    assert_eq!(response, "remind_me_later");
    assert_eq!(remind_at, Some(remind));
}

// ============================================================
// Identity gates on relay-touching methods
// ============================================================

// @internal
#[test]
fn upload_guardian_entries_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();
    let result = wb.upload_guardian_entries();
    assert!(matches!(result, Err(VauchiError::IdentityNotInitialized)));
}

// @internal
#[test]
fn vouch_for_claim_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();
    let claim = RecoveryClaim::new(&[1u8; 32], &[2u8; 32], 0);
    let result = wb.vouch_for_claim(&claim, "any-contact");
    assert!(matches!(result, Err(VauchiError::IdentityNotInitialized)));
}

// ============================================================
// verify_recovery_proof (PAE G3 push-down — was inline in
// vauchi-platform's VerifyRecoveryProof dispatch arm)
// ============================================================

/// Proof for recovering `old_pk` into `new_pk`, vouched by `helpers`.
fn build_proof(old_pk: [u8; 32], new_pk: &[u8; 32], helpers: &[&Identity]) -> RecoveryProof {
    let claim = RecoveryClaim::new(&old_pk, new_pk, 0);
    let mut proof = RecoveryProof::new(old_pk, *new_pk, helpers.len() as u32, 0);
    for helper in helpers {
        proof.add_voucher(build_voucher(&claim, helper)).unwrap();
    }
    proof
}

fn add_exchanged_contact(wb: &Vauchi, identity: &Identity, name: &str) {
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    wb.add_contact(contact).unwrap();
}

// @internal
#[test]
fn verify_recovery_proof_high_confidence_when_known_vouchers_meet_threshold() {
    let wb = vauchi_with_identity("Recoverer");
    let new_pk = *wb.identity().unwrap().signing_public_key();
    let bob = Identity::create("Bob", 0);
    let carol = Identity::create("Carol", 0);
    add_exchanged_contact(&wb, &bob, "Bob");
    add_exchanged_contact(&wb, &carol, "Carol");

    let proof = build_proof([7u8; 32], &new_pk, &[&bob, &carol]);
    let (parsed, result) = wb
        .verify_recovery_proof(&proof.to_bytes().unwrap())
        .unwrap();

    assert_eq!(parsed.old_pk().as_bytes(), &[7u8; 32]);
    assert_eq!(parsed.new_pk().as_bytes(), &new_pk);
    assert_eq!(parsed.voucher_count(), 2);
    match result {
        VerificationResult::HighConfidence {
            mutual_vouchers,
            total_vouchers,
        } => {
            assert_eq!(total_vouchers, 2);
            let mut names = mutual_vouchers.clone();
            names.sort();
            assert_eq!(names, vec!["Bob".to_string(), "Carol".to_string()]);
        }
        other => panic!("expected HighConfidence, got {other:?}"),
    }
}

// @internal
#[test]
fn verify_recovery_proof_medium_confidence_with_one_known_voucher() {
    let wb = vauchi_with_identity("Recoverer");
    let new_pk = *wb.identity().unwrap().signing_public_key();
    let bob = Identity::create("Bob", 0);
    let stranger = Identity::create("Mallory", 0);
    add_exchanged_contact(&wb, &bob, "Bob");

    let proof = build_proof([7u8; 32], &new_pk, &[&bob, &stranger]);
    let (_, result) = wb
        .verify_recovery_proof(&proof.to_bytes().unwrap())
        .unwrap();

    match result {
        VerificationResult::MediumConfidence {
            mutual_vouchers,
            required,
            total_vouchers,
        } => {
            assert_eq!(mutual_vouchers, vec!["Bob".to_string()]);
            assert_eq!(required, 2, "default verification threshold");
            assert_eq!(total_vouchers, 2);
        }
        other => panic!("expected MediumConfidence, got {other:?}"),
    }
}

// @internal
#[test]
fn verify_recovery_proof_low_confidence_with_no_known_vouchers() {
    let wb = vauchi_with_identity("Recoverer");
    let new_pk = *wb.identity().unwrap().signing_public_key();
    let stranger = Identity::create("Mallory", 0);

    let proof = build_proof([7u8; 32], &new_pk, &[&stranger]);
    let (_, result) = wb
        .verify_recovery_proof(&proof.to_bytes().unwrap())
        .unwrap();

    match result {
        VerificationResult::LowConfidence { total_vouchers } => {
            assert_eq!(total_vouchers, 1);
        }
        other => panic!("expected LowConfidence, got {other:?}"),
    }
}

// @internal
#[test]
fn verify_recovery_proof_rejects_garbage_bytes() {
    let wb = vauchi_with_identity("Recoverer");
    let result = wb.verify_recovery_proof(&[0xFF, 0x00, 0x13, 0x37]);
    assert!(matches!(result, Err(VauchiError::Serialization(_))));
}

// @internal
#[test]
fn verify_recovery_proof_rejects_proof_below_its_own_threshold() {
    let wb = vauchi_with_identity("Recoverer");
    let new_pk = *wb.identity().unwrap().signing_public_key();
    let bob = Identity::create("Bob", 0);

    let claim = RecoveryClaim::new(&[7u8; 32], &new_pk, 0);
    let mut proof = RecoveryProof::new([7u8; 32], new_pk, 3, 0);
    proof.add_voucher(build_voucher(&claim, &bob)).unwrap();

    let result = wb.verify_recovery_proof(&proof.to_bytes().unwrap());
    assert!(matches!(result, Err(VauchiError::InvalidState(_))));
}
