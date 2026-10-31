// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Recovery parsers take exactly their bytes, a proof is one shape, and a
//! timestamp from the future is a refusal, not a longer life
//! (vauchi/private#930, DC-01/DC-02).

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::{
    RECOVERY_PROOF_VERSION, RecoveryClaim, RecoveryError, RecoveryProgress, RecoveryProof,
    RecoverySettings, RecoveryVoucher,
};

const NOW: u64 = 1_700_000_000;

fn keys() -> (SigningKeyPair, SigningKeyPair) {
    (SigningKeyPair::generate(), SigningKeyPair::generate())
}

fn claim_at(old: &SigningKeyPair, new: &SigningKeyPair, timestamp: u64) -> RecoveryClaim {
    RecoveryClaim::new_with_timestamp(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        timestamp,
    )
}

fn proof_with_helpers(threshold: u32, helpers: usize) -> RecoveryProof {
    let (old, new) = keys();
    let mut proof = RecoveryProof::new(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        threshold,
        NOW,
    );
    for _ in 0..helpers {
        let helper = SigningKeyPair::generate();
        let voucher = RecoveryVoucher::create(
            old.public_key().as_bytes(),
            new.public_key().as_bytes(),
            &helper,
            None,
            NOW,
        );
        proof.add_voucher(voucher).unwrap();
    }
    proof
}

// @scenario: contact_recovery :: Recovery parsers take exactly their bytes
#[test]
fn a_recovery_claim_with_trailing_bytes_is_refused() {
    let (old, new) = keys();
    let exact = claim_at(&old, &new, NOW).to_bytes();
    let mut one_more = exact.clone();
    one_more.push(0);

    assert_eq!(exact.len(), 73);
    assert_eq!(
        RecoveryClaim::from_bytes(&exact).unwrap().timestamp(),
        NOW,
        "the exact 73 bytes parse"
    );
    assert!(matches!(
        RecoveryClaim::from_bytes(&one_more),
        Err(RecoveryError::InvalidFormat)
    ));
    assert!(matches!(
        RecoveryClaim::from_bytes(&exact[..72]),
        Err(RecoveryError::InvalidFormat)
    ));
}

// @scenario: contact_recovery :: Recovery parsers take exactly their bytes
#[test]
fn a_recovery_proof_with_trailing_bytes_is_refused() {
    let exact = proof_with_helpers(1, 1).to_bytes().unwrap();
    let mut one_more = exact.clone();
    one_more.push(0);

    assert_eq!(
        RecoveryProof::from_bytes(&exact).unwrap().voucher_count(),
        1
    );
    assert!(
        matches!(
            RecoveryProof::from_bytes(&one_more),
            Err(RecoveryError::InvalidFormat)
        ),
        "a byte after the postcard proof is refused"
    );
}

// @scenario: contact_recovery :: Recovery parsers take exactly their bytes
#[test]
fn a_recovery_proof_with_an_unknown_version_is_refused() {
    let current = proof_with_helpers(1, 1).to_bytes().unwrap();
    let mut older = current.clone();
    older[0] = RECOVERY_PROOF_VERSION - 1;
    let mut newer = current.clone();
    newer[0] = RECOVERY_PROOF_VERSION + 1;

    assert_eq!(
        RecoveryProof::from_bytes(&current).unwrap().version(),
        RECOVERY_PROOF_VERSION
    );
    for (name, bytes) in [("older", older), ("newer", newer)] {
        assert!(
            matches!(
                RecoveryProof::from_bytes(&bytes),
                Err(RecoveryError::InvalidProof(_))
            ),
            "a proof of {name} version must be refused at the parser"
        );
    }
}

// @scenario: contact_recovery :: A recovery proof outside the threshold bounds is refused
#[test]
fn a_recovery_threshold_of_zero_is_refused() {
    let zero = proof_with_helpers(0, 0);
    let ten = proof_with_helpers(RecoverySettings::MAX_RECOVERY_THRESHOLD + 1, 0);
    let one = proof_with_helpers(1, 1);

    assert!(
        matches!(zero.validate(), Err(RecoveryError::ThresholdTooLow)),
        "no vouchers satisfy a threshold of zero, got {:?}",
        zero.validate()
    );
    assert!(matches!(
        ten.validate(),
        Err(RecoveryError::ThresholdTooHigh)
    ));
    assert!(one.validate().is_ok(), "{:?}", one.validate());
}

// @scenario: contact_recovery :: A future-dated recovery claim is refused
#[test]
fn a_future_dated_recovery_claim_is_refused() {
    let (old, new) = keys();
    let helper = SigningKeyPair::generate();
    let skew = RecoveryClaim::MAX_FUTURE_SKEW_SECS;
    let within_skew = claim_at(&old, &new, NOW + skew);
    let beyond_skew = claim_at(&old, &new, NOW + skew + 1);
    let ten_years_ahead = claim_at(&old, &new, NOW + 10 * 365 * 24 * 3600);

    assert!(RecoveryVoucher::create_from_claim(&within_skew, &helper, None, NOW).is_ok());
    assert!(matches!(
        RecoveryVoucher::create_from_claim(&beyond_skew, &helper, None, NOW),
        Err(RecoveryError::TimestampInFuture)
    ));
    assert!(
        matches!(
            RecoveryVoucher::create_from_claim(
                &ten_years_ahead,
                &helper,
                None,
                NOW + 365 * 24 * 3600
            ),
            Err(RecoveryError::TimestampInFuture)
        ),
        "a year later the ten-years-ahead claim is still not vouchable"
    );
}

// @scenario: contact_recovery :: A future-dated recovery claim is refused
#[test]
fn a_proof_whose_voucher_is_from_the_future_is_refused() {
    let (old, new) = keys();
    let helper = SigningKeyPair::generate();
    let skew = RecoveryClaim::MAX_FUTURE_SKEW_SECS;
    let mut proof = RecoveryProof::new(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        1,
        NOW,
    );
    let voucher = RecoveryVoucher::create(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        &helper,
        None,
        NOW + skew + 1,
    );
    proof.add_voucher(voucher).unwrap();

    assert!(matches!(
        proof.validate_at(NOW),
        Err(RecoveryError::TimestampInFuture)
    ));
    assert!(
        proof.validate_at(NOW + 1).is_ok(),
        "a second later it is within skew"
    );
}

// @scenario: contact_recovery :: An old-key voucher is refused while collecting
#[test]
fn an_old_key_voucher_is_refused_while_collecting() {
    let (old, new) = keys();
    let helper = SigningKeyPair::generate();
    let claim = claim_at(&old, &new, NOW);
    let mut progress = RecoveryProgress::new(claim.clone(), 2, NOW);
    let by_old_key = RecoveryVoucher::create(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        &old,
        None,
        NOW,
    );
    let by_new_key = RecoveryVoucher::create(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        &new,
        None,
        NOW,
    );
    let by_helper = RecoveryVoucher::create(
        old.public_key().as_bytes(),
        new.public_key().as_bytes(),
        &helper,
        None,
        NOW,
    );

    assert!(matches!(
        progress.add_voucher(by_old_key),
        Err(RecoveryError::SelfVouching)
    ));
    assert!(matches!(
        progress.add_voucher(by_new_key),
        Err(RecoveryError::SelfVouching)
    ));
    assert_eq!(progress.add_voucher(by_helper).unwrap(), 1);
}
