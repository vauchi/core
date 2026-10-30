// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A recovery is one window of 192 hours: within it the person collects
//! vouchers, restores the backup and tells contacts about the new key
//! (owner decision 2026-10-30, private#959, private#926). A helper refuses
//! an older claim, a contact refuses an older proof, and the vouchers of
//! one proof span at most 96 hours (private#919). The windows are read
//! from the signed voucher timestamps, never from the proof's own fields.

use std::time::{Duration, SystemTime};

use vauchi_core::clock::FakeClock;
use vauchi_core::recovery::{RecoveryClaim, RecoveryError, RecoveryProof, RecoveryVoucher};
use vauchi_core::{SigningKeyPair, Vauchi};

const HOUR: u64 = 60 * 60;
const NOW: u64 = 1_800_000_000;

fn keys() -> (SigningKeyPair, SigningKeyPair) {
    (SigningKeyPair::generate(), SigningKeyPair::generate())
}

/// A two-voucher proof whose helpers signed at `first_at` and `second_at`.
fn proof_with_vouchers_at(first_at: u64, second_at: u64) -> RecoveryProof {
    let (old, new) = keys();
    let old_pk = *old.public_key().as_bytes();
    let new_pk = *new.public_key().as_bytes();
    let mut proof = RecoveryProof::new(old_pk, new_pk, 2, first_at);
    for at in [first_at, second_at] {
        let helper = SigningKeyPair::generate();
        proof
            .add_voucher(RecoveryVoucher::create(old_pk, new_pk, &helper, None, at))
            .unwrap();
    }
    proof
}

// @scenario: contact_recovery :: Recovery proof expiration
#[test]
fn a_recovery_proof_older_than_192_hours_is_refused_by_a_contact() {
    let clock = FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(NOW)).shared();
    let mut contact = Vauchi::in_memory_with_clock(clock).unwrap();
    contact.create_identity("Carol").unwrap();

    let stale = proof_with_vouchers_at(NOW - 193 * HOUR, NOW - 193 * HOUR);
    let fresh = proof_with_vouchers_at(NOW - 191 * HOUR, NOW - 100 * HOUR);

    let refused = contact.verify_recovery_proof(&stale.to_bytes().unwrap());
    assert!(
        matches!(&refused, Err(e) if e.to_string().contains("expired")),
        "a proof whose earliest voucher is 193 h old must be refused, got {refused:?}"
    );
    assert!(
        contact
            .verify_recovery_proof(&fresh.to_bytes().unwrap())
            .is_ok(),
        "a proof whose earliest voucher is 191 h old is still verified"
    );
}

// @scenario: contact_recovery :: Voucher timestamp validation
#[test]
fn a_helper_refuses_a_claim_older_than_192_hours() {
    let (old, new) = keys();
    let helper = SigningKeyPair::generate();
    let stale_claim = RecoveryClaim::new(
        *old.public_key().as_bytes(),
        *new.public_key().as_bytes(),
        NOW - 193 * HOUR,
    );
    let fresh_claim = RecoveryClaim::new(
        *old.public_key().as_bytes(),
        *new.public_key().as_bytes(),
        NOW - 191 * HOUR,
    );

    let refused = RecoveryVoucher::create_from_claim(&stale_claim, &helper, None, NOW);
    assert!(
        matches!(refused, Err(RecoveryError::ClaimExpired)),
        "got {refused:?}"
    );
    assert!(RecoveryVoucher::create_from_claim(&fresh_claim, &helper, None, NOW).is_ok());
}

// @scenario: contact_recovery :: Vouchers of one recovery are given within 96 hours
#[test]
fn vouchers_spanning_more_than_96_hours_are_refused() {
    let too_wide = proof_with_vouchers_at(NOW - 100 * HOUR, NOW - 3 * HOUR);
    let within = proof_with_vouchers_at(NOW - 98 * HOUR, NOW - 3 * HOUR);

    assert!(
        matches!(
            too_wide.validate(),
            Err(RecoveryError::VouchersOutsideWindow)
        ),
        "97 h between the first and the last voucher must be refused"
    );
    assert!(
        within.validate().is_ok(),
        "95 h between vouchers is within the window"
    );
}
