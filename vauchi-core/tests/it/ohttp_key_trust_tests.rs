// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one rule that decides whether a fetched OHTTP gateway key may be used
//! (#288). Real Ed25519 keys from fixed seeds; no mocks (ADR-002).

use ed25519_dalek::{Signer, SigningKey};
use proptest::prelude::*;
use vauchi_core::network::ohttp_key_trust::{HeldOhttpKey, OhttpKeyRejection, accept_signed_key};
use vauchi_protocol::ohttp_key::{
    IntermediateCert, SignedKeyConfig, WINDOW_SECONDS, key_id_for_window, window_of,
};

const NOW: u64 = 1_759_622_400; // 2025-10-05T00:00:00Z, window start

fn anchor() -> SigningKey {
    SigningKey::from_bytes(&[0xa1; 32])
}

fn intermediate() -> SigningKey {
    SigningKey::from_bytes(&[0xb2; 32])
}

fn certify(
    anchor: &SigningKey,
    intermediate: &SigningKey,
    not_before: u64,
    not_after: u64,
) -> IntermediateCert {
    let public_key = intermediate.verifying_key().to_bytes();
    let message = IntermediateCert::signing_message(&public_key, not_before, not_after);
    IntermediateCert {
        public_key,
        not_before,
        not_after,
        anchor_signature: anchor.sign(&message).to_bytes(),
    }
}

fn key_config(window: u64, body: u8) -> Vec<u8> {
    let mut config = vec![key_id_for_window(window), 0x00, 0x20];
    config.extend_from_slice(&[body; 38]);
    config
}

fn record_with(
    window: u64,
    config: Vec<u8>,
    cert: IntermediateCert,
    signer: &SigningKey,
) -> SignedKeyConfig {
    let signature = signer
        .sign(&SignedKeyConfig::signing_message(window, &config))
        .to_bytes();
    SignedKeyConfig {
        window,
        key_config: config,
        signature,
        intermediate: cert,
    }
}

fn valid_cert() -> IntermediateCert {
    certify(
        &anchor(),
        &intermediate(),
        NOW - 30 * 86_400,
        NOW + 60 * 86_400,
    )
}

fn valid_record(window: u64) -> SignedKeyConfig {
    record_with(
        window,
        key_config(window, 0x5a),
        valid_cert(),
        &intermediate(),
    )
}

fn anchor_key() -> [u8; 32] {
    anchor().verifying_key().to_bytes()
}

fn current() -> u64 {
    window_of(NOW)
}

// @scenario: release_privacy_multidevice_certification.feature:Neither relay can decrypt or identify application users
#[test]
fn a_correctly_signed_key_for_the_current_window_is_accepted() {
    let record = valid_record(current());

    let held = accept_signed_key(&record, &anchor_key(), NOW, None).unwrap();

    assert_eq!(
        held,
        HeldOhttpKey {
            window: current(),
            key_config: record.key_config.clone()
        }
    );
}

// @internal
#[test]
fn an_intermediate_not_signed_by_the_anchor_is_rejected() {
    let rogue_anchor = SigningKey::from_bytes(&[0xee; 32]);
    let cert = certify(&rogue_anchor, &intermediate(), NOW - 86_400, NOW + 86_400);
    let record = record_with(current(), key_config(current(), 1), cert, &intermediate());

    assert_eq!(
        accept_signed_key(&record, &anchor_key(), NOW, None),
        Err(OhttpKeyRejection::IntermediateSignature)
    );
}

// @internal
#[test]
fn an_intermediate_outside_its_validity_is_rejected() {
    let early = certify(&anchor(), &intermediate(), NOW + 1, NOW + 86_400);
    let late = certify(&anchor(), &intermediate(), NOW - 86_400, NOW - 1);

    assert_eq!(
        accept_signed_key(
            &record_with(current(), key_config(current(), 1), early, &intermediate()),
            &anchor_key(),
            NOW,
            None
        ),
        Err(OhttpKeyRejection::IntermediateNotYetValid)
    );
    assert_eq!(
        accept_signed_key(
            &record_with(current(), key_config(current(), 1), late, &intermediate()),
            &anchor_key(),
            NOW,
            None
        ),
        Err(OhttpKeyRejection::IntermediateExpired)
    );
}

/// The outer relay's substitution: a key it holds the private half of,
/// with a signature it cannot make.
// @scenario: release_privacy_multidevice_certification.feature:Neither relay can decrypt or identify application users
#[test]
fn a_substituted_key_is_rejected() {
    let mut record = valid_record(current());
    record.key_config = key_config(current(), 0x99);

    assert_eq!(
        accept_signed_key(&record, &anchor_key(), NOW, None),
        Err(OhttpKeyRejection::KeySignature)
    );
}

// @internal
#[test]
fn a_signature_from_a_key_other_than_the_certified_intermediate_is_rejected() {
    let other = SigningKey::from_bytes(&[0xcc; 32]);
    let record = record_with(current(), key_config(current(), 1), valid_cert(), &other);

    assert_eq!(
        accept_signed_key(&record, &anchor_key(), NOW, None),
        Err(OhttpKeyRejection::KeySignature)
    );
}

// @internal
#[test]
fn a_window_outside_current_plus_or_minus_one_is_rejected() {
    for window in [current() - 2, current() + 2] {
        assert_eq!(
            accept_signed_key(&valid_record(window), &anchor_key(), NOW, None),
            Err(OhttpKeyRejection::WindowOutOfRange {
                window,
                current: current()
            })
        );
    }
    for window in [current() - 1, current() + 1] {
        let record = valid_record(window);
        assert_eq!(
            accept_signed_key(&record, &anchor_key(), NOW, None),
            Ok(HeldOhttpKey {
                window,
                key_config: record.key_config.clone()
            }),
            "window {window} is within the grace"
        );
    }
}

// @internal
#[test]
fn a_key_id_that_does_not_match_the_window_is_rejected() {
    let mut config = key_config(current(), 1);
    config[0] = key_id_for_window(current()).wrapping_add(1);
    let record = record_with(current(), config, valid_cert(), &intermediate());

    assert_eq!(
        accept_signed_key(&record, &anchor_key(), NOW, None),
        Err(OhttpKeyRejection::KeyIdMismatch {
            expected: key_id_for_window(current()),
            found: key_id_for_window(current()).wrapping_add(1)
        })
    );
}

/// Equivocation: a second, validly signed key for a window this client
/// already holds — the signal of a per-client key (README §Update 2026-09-07).
// @scenario: release_privacy_multidevice_certification.feature:Neither relay can decrypt or identify application users
#[test]
fn a_different_key_for_a_window_already_held_is_rejected() {
    let held = HeldOhttpKey {
        window: current(),
        key_config: key_config(current(), 0x01),
    };
    let second = record_with(
        current(),
        key_config(current(), 0x02),
        valid_cert(),
        &intermediate(),
    );

    assert_eq!(
        accept_signed_key(&second, &anchor_key(), NOW, Some(&held)),
        Err(OhttpKeyRejection::Equivocation { window: current() })
    );
}

// @internal
#[test]
fn the_same_key_for_a_held_window_is_accepted_again() {
    let record = valid_record(current());
    let held = HeldOhttpKey {
        window: current(),
        key_config: record.key_config.clone(),
    };

    assert_eq!(
        accept_signed_key(&record, &anchor_key(), NOW, Some(&held)),
        Ok(held.clone())
    );
}

// @internal
#[test]
fn a_window_older_than_the_held_one_is_rejected() {
    let held = HeldOhttpKey {
        window: current(),
        key_config: key_config(current(), 1),
    };

    assert_eq!(
        accept_signed_key(
            &valid_record(current() - 1),
            &anchor_key(),
            NOW,
            Some(&held)
        ),
        Err(OhttpKeyRejection::OlderThanHeld {
            window: current() - 1,
            held: current()
        })
    );
}

/// Rejection messages carry error types and windows only, never key bytes
/// (DC-05).
// @internal
#[test]
fn rejection_messages_carry_no_key_material() {
    let message = OhttpKeyRejection::Equivocation { window: 7 }.to_string();

    assert_eq!(
        message,
        "a different OHTTP key was offered for held window 7"
    );
}

proptest! {
    /// Whatever the record says, only the anchor's chain installs a key.
    // @internal
    #[test]
    fn no_record_signed_outside_the_anchor_chain_is_accepted(
        rogue_seed in any::<[u8; 32]>().prop_filter("not the anchor", |s| *s != [0xa1; 32]),
        body in any::<u8>(),
        offset in 0u64..WINDOW_SECONDS,
    ) {
        let rogue = SigningKey::from_bytes(&rogue_seed);
        let cert = certify(&rogue, &rogue, NOW - 86_400, NOW + 86_400);
        let record = record_with(current(), key_config(current(), body), cert, &rogue);

        prop_assert_eq!(
            accept_signed_key(&record, &anchor_key(), NOW + offset, None),
            Err(OhttpKeyRejection::IntermediateSignature)
        );
    }
}

// The validity window is inclusive at both ends.
// @internal
#[test]
fn an_intermediate_is_valid_on_its_first_and_last_second() {
    let starts_now = certify(&anchor(), &intermediate(), NOW, NOW + 86_400);
    let ends_now = certify(&anchor(), &intermediate(), NOW - 86_400, NOW);

    for cert in [starts_now, ends_now] {
        let record = record_with(current(), key_config(current(), 1), cert, &intermediate());
        assert!(accept_signed_key(&record, &anchor_key(), NOW, None).is_ok());
    }
}
