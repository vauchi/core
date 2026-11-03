// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The guardian backup delivery carries the guardian's token, and an
//! in-person vouch attaches it (vauchi/private#796, D19): the token is the
//! designator's old key naming this guardian, so a contact can tell a
//! voucher from one of the owner's own guardians without asking anyone.
//! A delivery in the earlier form, without a token, still reads.

use std::collections::HashMap;

use vauchi_core::Vauchi;
use vauchi_core::api::ReceiveOutcome;
use vauchi_core::recovery::RecoveryVoucher;
use vauchi_core::recovery::guardian::GuardianToken;
use vauchi_core::sync::guardian_piece::{GuardianDelivery, PieceAssembly};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::signing_pk;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

/// Alice with three guardians, each in a group of its own, that have
/// received her backup; returns each guardian's newest delivery.
fn guarded() -> (Vauchi, Vec<Recipient>, Vec<GuardianDelivery>) {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let mut guardians: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }
    let deliveries = guardians
        .iter_mut()
        .map(|guardian| {
            let mut assemblies: HashMap<[u8; 16], PieceAssembly> = HashMap::new();
            let mut whole = None;
            for outcome in receive(&alice, guardian) {
                if let ReceiveOutcome::GuardianBackupPiece(piece) = outcome
                    && let Some(bytes) = assemblies
                        .entry(piece.backup_id())
                        .or_default()
                        .add(piece)
                        .unwrap()
                {
                    whole = Some(GuardianDelivery::decode(&bytes).unwrap());
                }
            }
            whole.unwrap()
        })
        .collect();
    (alice, guardians, deliveries)
}

// @internal
#[test]
fn each_delivery_carries_a_token_alices_key_signed_naming_that_guardian() {
    let (alice, guardians, deliveries) = guarded();

    for (guardian, delivery) in guardians.iter().zip(&deliveries) {
        let token = GuardianToken::from_bytes(delivery.guardian_token().unwrap()).unwrap();
        assert!(token.verify());
        assert_eq!(token.designator_pk(), &signing_pk(&alice));
        assert_eq!(token.guardian_pk(), &signing_pk(guardian.vauchi()));
    }
}

// @internal
#[test]
fn an_in_person_vouch_attaches_the_token_the_guardian_holds() {
    let (alice, guardians, _) = guarded();
    let phone = create_vauchi_with_identity("Alice, new phone");
    let claim = phone
        .create_recovery_claim_hex_b64(&hex::encode(signing_pk(&alice)))
        .unwrap();

    let voucher_b64 = guardians[0]
        .vauchi()
        .create_voucher_from_claim_b64(&claim)
        .unwrap();

    use base64::Engine;
    let voucher = RecoveryVoucher::from_bytes(
        &base64::engine::general_purpose::STANDARD
            .decode(&voucher_b64)
            .unwrap(),
    )
    .unwrap();
    let token = voucher.guardian_token().expect("a guardian's voucher");
    assert_eq!(token.designator_pk(), &signing_pk(&alice));
    assert_eq!(token.guardian_pk(), &signing_pk(guardians[0].vauchi()));
    assert!(phone.add_recovery_voucher_b64(&voucher_b64).is_ok());
}

// @internal
#[test]
fn a_contact_who_is_no_guardian_vouches_without_a_token() {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let friend = add_recipient(&alice, &alice_pk, "Friend");
    let phone = create_vauchi_with_identity("Alice, new phone");
    let claim = phone
        .create_recovery_claim_hex_b64(&hex::encode(alice_pk))
        .unwrap();

    let voucher_b64 = friend
        .vauchi()
        .create_voucher_from_claim_b64(&claim)
        .unwrap();

    use base64::Engine;
    let voucher = RecoveryVoucher::from_bytes(
        &base64::engine::general_purpose::STANDARD
            .decode(&voucher_b64)
            .unwrap(),
    )
    .unwrap();
    assert!(voucher.guardian_token().is_none());
}

// @internal
#[test]
fn a_delivery_in_the_earlier_form_still_reads_without_a_token() {
    let mut earlier = Vec::new();
    earlier.extend_from_slice(&1_700_000_000u64.to_be_bytes());
    earlier.extend_from_slice(&3u16.to_be_bytes());
    earlier.extend_from_slice(&[7, 7, 7]);
    earlier.extend_from_slice(b"backup");

    let delivery = GuardianDelivery::decode(&earlier).unwrap();

    assert_eq!(delivery.created_at(), 1_700_000_000);
    assert_eq!(delivery.sealed_share(), [7, 7, 7]);
    assert_eq!(delivery.backup(), b"backup");
    assert_eq!(delivery.guardian_token(), None);
    assert_eq!(delivery.encode(), earlier, "no token, the earlier form");
}

// @internal
#[test]
fn a_delivery_with_a_token_round_trips_and_malformed_tokens_are_refused() {
    let delivery = GuardianDelivery::new(5, vec![1; 124], b"backup".to_vec())
        .unwrap()
        .with_guardian_token(vec![9; 150])
        .unwrap();

    let decoded = GuardianDelivery::decode(&delivery.encode()).unwrap();
    assert_eq!(decoded, delivery);
    assert_eq!(decoded.guardian_token(), Some([9u8; 150].as_slice()));

    let delivery = GuardianDelivery::new(5, vec![1; 124], b"backup".to_vec()).unwrap();
    assert!(delivery.clone().with_guardian_token(Vec::new()).is_err());
    assert!(delivery.with_guardian_token(vec![0; 1025]).is_err());
    let mut lying = GuardianDelivery::new(5, vec![1; 124], b"backup".to_vec())
        .unwrap()
        .with_guardian_token(vec![9; 150])
        .unwrap()
        .encode();
    let token_len_at = 1 + 8 + 2 + 124;
    lying[token_len_at..token_len_at + 2].copy_from_slice(&u16::MAX.to_be_bytes());
    assert!(GuardianDelivery::decode(&lying).is_err());
}
