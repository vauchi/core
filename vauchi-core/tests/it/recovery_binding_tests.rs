// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A recovery voucher, proof or revocation counts only for the exact
//! old-key → new-key pair it was made for, and recovery bytes from the
//! network are length-checked before use (vauchi/private#522).

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::*;

const OLD: [u8; 32] = [0x01; 32];
const NEW: [u8; 32] = [0x02; 32];
const OTHER: [u8; 32] = [0x03; 32];

fn voucher_for(old: &[u8; 32], new: &[u8; 32]) -> RecoveryVoucher {
    RecoveryVoucher::create(old, new, &SigningKeyPair::generate(), None, 0)
}

/// A one-voucher proof for OLD → NEW whose voucher was swapped in transit.
fn received_proof_with_voucher(voucher: RecoveryVoucher) -> RecoveryProof {
    let mut proof = RecoveryProof::new(&OLD, &NEW, 1, 0);
    proof.add_voucher(voucher_for(&OLD, &NEW)).unwrap();
    let mut json = serde_json::to_value(&proof).unwrap();
    json["vouchers"][0] = serde_json::to_value(&voucher).unwrap();
    serde_json::from_value(json).unwrap()
}

// @scenario: contact_recovery :: Collect multiple vouchers from trusted contacts
// @internal
#[test]
fn a_received_proof_rejects_a_voucher_made_for_another_key_pair() {
    assert!(
        received_proof_with_voucher(voucher_for(&OLD, &NEW))
            .validate()
            .is_ok()
    );

    for (old, new) in [(&OLD, &OTHER), (&OTHER, &NEW)] {
        let result = received_proof_with_voucher(voucher_for(old, new)).validate();
        assert!(
            matches!(result, Err(RecoveryError::MismatchedKeys)),
            "{result:?}"
        );
    }
}

// @scenario: contact_recovery :: Collect multiple vouchers from trusted contacts
// @internal
#[test]
fn recovery_progress_rejects_a_voucher_made_for_another_key_pair() {
    for (old, new) in [(&OLD, &OTHER), (&OTHER, &NEW)] {
        let mut progress = RecoveryProgress::new(RecoveryClaim::new(&OLD, &NEW, 0), 2, 0);

        let result = progress.add_voucher(voucher_for(old, new));

        assert!(
            matches!(result, Err(RecoveryError::MismatchedKeys)),
            "{result:?}"
        );
    }
}

// @internal
#[test]
fn a_revocation_applies_only_to_the_recovery_it_names() {
    let old_keys = SigningKeyPair::generate();
    let old = *old_keys.public_key().as_bytes();
    let revocation = RecoveryRevocation::create(&old, &NEW, &old_keys, 0);

    assert!(revocation.applies_to(&RecoveryProof::new(&old, &NEW, 1, 0)));
    assert!(!revocation.applies_to(&RecoveryProof::new(&old, &OTHER, 1, 0)));
    assert!(!revocation.applies_to(&RecoveryProof::new(&OTHER, &NEW, 1, 0)));
}

// @internal
#[test]
fn a_truncated_claim_is_refused() {
    let bytes = RecoveryClaim::new(&OLD, &NEW, 7).to_bytes();
    assert_eq!(bytes.len(), 73);

    assert!(RecoveryClaim::from_bytes(&bytes).is_ok());
    for len in [0, 1, 72] {
        assert!(
            matches!(
                RecoveryClaim::from_bytes(&bytes[..len]),
                Err(RecoveryError::InvalidFormat)
            ),
            "{len} bytes"
        );
    }
}

// The guardian-token tail is read only for version 2 and only when present.
// @internal
#[test]
fn a_voucher_reads_a_token_tail_only_when_version_two_carries_one() {
    let v1 = voucher_for(&OLD, &NEW).to_bytes();
    assert_eq!((v1[0], v1.len()), (1, 169));

    let mut v2_without_token = v1.clone();
    v2_without_token[0] = 2;
    let parsed = RecoveryVoucher::from_bytes(&v2_without_token).unwrap();
    assert!(parsed.guardian_token().is_none());

    let mut v1_with_trailing_bytes = v1.clone();
    v1_with_trailing_bytes.extend_from_slice(&[0xFF; 8]);
    let parsed = RecoveryVoucher::from_bytes(&v1_with_trailing_bytes).unwrap();
    assert!(parsed.guardian_token().is_none());

    for extra in 1..=3 {
        let mut short_prefix = v2_without_token.clone();
        short_prefix.extend(std::iter::repeat_n(0, extra));
        assert!(
            matches!(
                RecoveryVoucher::from_bytes(&short_prefix),
                Err(RecoveryError::InvalidFormat)
            ),
            "{extra} length-prefix bytes"
        );
    }

    let mut empty_token = v2_without_token;
    empty_token.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        matches!(
            RecoveryVoucher::from_bytes(&empty_token),
            Err(RecoveryError::SerializationError(_))
        ),
        "a complete length prefix hands the declared token to its parser"
    );
}

fn proof_with_vouchers(new: &[u8; 32], count: usize) -> RecoveryProof {
    let mut proof = RecoveryProof::new(&OLD, new, 1, 0);
    for _ in 0..count {
        proof.add_voucher(voucher_for(&OLD, new)).unwrap();
    }
    proof
}

// @internal
#[test]
fn a_conflict_reports_how_many_vouchers_back_each_new_key() {
    let conflict =
        RecoveryConflict::detect(&[proof_with_vouchers(&NEW, 2), proof_with_vouchers(&OTHER, 1)])
            .expect("two new keys for one old key conflict");

    let mut counts: Vec<([u8; 32], usize)> = conflict
        .claims()
        .iter()
        .map(|claim| (*claim.new_pk().as_bytes(), claim.voucher_count()))
        .collect();
    counts.sort();
    assert_eq!(counts, [(NEW, 2), (OTHER, 1)]);
}

// @internal
#[test]
fn a_proof_exposes_the_vouchers_it_collected() {
    let proof = proof_with_vouchers(&NEW, 2);

    assert_eq!(proof.vouchers().len(), 2);
    assert!(
        proof
            .vouchers()
            .iter()
            .all(|v| v.new_pk().as_bytes() == &NEW)
    );
}

// @internal
#[test]
fn a_claim_expires_only_after_its_maximum_age() {
    let claim = RecoveryClaim::new(&OLD, &NEW, 1_000);
    let limit = 1_000 + RecoveryClaim::MAX_AGE_SECS;

    assert_eq!(claim.timestamp(), 1_000);
    assert!(!claim.is_expired(limit));
    assert!(claim.is_expired(limit + 1));
}

// @internal
#[test]
fn a_revocation_keeps_the_time_it_was_made() {
    let old_keys = SigningKeyPair::generate();
    let old = *old_keys.public_key().as_bytes();

    assert_eq!(
        RecoveryRevocation::create(&old, &NEW, &old_keys, 1_234).timestamp(),
        1_234
    );
}

// @internal
#[test]
fn a_proof_without_a_version_is_refused() {
    let mut json = serde_json::to_value(RecoveryProof::new(&OLD, &NEW, 1, 0)).unwrap();
    json.as_object_mut().unwrap().remove("version");

    let err = serde_json::from_value::<RecoveryProof>(json).unwrap_err();

    assert!(
        err.to_string().contains("version"),
        "unexpected error: {err}"
    );
}

// @internal
#[test]
fn a_recovery_reminder_is_due_after_its_days_have_passed() {
    let reminder = RecoveryReminder::new(&OLD, 1_000);
    let due = 1_000 + u64::from(RecoveryReminder::DEFAULT_REMINDER_DAYS) * 86_400;

    assert!(!reminder.is_due(due - 1));
    assert!(reminder.is_due(due));
}

// @internal
#[test]
fn recovery_responses_name_themselves_and_carry_a_reminder_time() {
    let later = RecoveryResponse::RemindMeLater { remind_at: 5 };

    assert_eq!(
        [
            RecoveryResponse::Accept.as_str(),
            RecoveryResponse::Reject.as_str(),
            later.as_str()
        ],
        ["accept", "reject", "remind_me_later"]
    );
    assert_eq!(later.remind_at(), Some(5));
    assert_eq!(RecoveryResponse::Accept.remind_at(), None);
}

// A response arriving from a shell is one of three decisions, and only a
// reminder carries a time (vauchi/private#538).
// @internal
#[test]
fn a_recovery_response_parses_only_from_a_known_decision() {
    assert_eq!(
        RecoveryResponse::parse("accept", None),
        Some(RecoveryResponse::Accept)
    );
    assert_eq!(
        RecoveryResponse::parse("reject", None),
        Some(RecoveryResponse::Reject)
    );
    assert_eq!(
        RecoveryResponse::parse("remind_me_later", Some(5)),
        Some(RecoveryResponse::RemindMeLater { remind_at: 5 })
    );

    for (name, remind_at) in [
        ("accepted", None),
        ("", None),
        ("ACCEPT", None),
        ("accept", Some(5)),
        ("reject", Some(5)),
        ("remind_me_later", None),
    ] {
        assert_eq!(
            RecoveryResponse::parse(name, remind_at),
            None,
            "{name:?} {remind_at:?}"
        );
    }
}
