// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Whoever holds the lost device holds the old key, so a voucher it signs
//! cannot count towards the recovery of that key; otherwise a thief needs
//! one dishonest helper instead of the threshold. Found by
//! `recovery_acceptance.spthy` (vauchi/private#797).

use vauchi_core::recovery::{
    RECOVERY_PROOF_VERSION, RecoveryError, RecoveryProof, RecoverySettings, RecoveryVoucher,
    VerificationResult,
};
use vauchi_core::{Contact, ContactCard, SigningKeyPair, SymmetricKey};

const NOW: u64 = 1_700_000_000;

/// A proof as the relay could deliver it, with no `add_voucher` checks.
#[derive(serde::Serialize)]
struct WireProof {
    version: u8,
    old_pk: [u8; 32],
    new_pk: [u8; 32],
    threshold: u32,
    vouchers: Vec<RecoveryVoucher>,
    created_at: u64,
    expires_at: u64,
}

struct Theft {
    lost: SigningKeyPair,
    thief_new: SigningKeyPair,
    dishonest_helper: SigningKeyPair,
}

impl Theft {
    fn new() -> Self {
        Self {
            lost: SigningKeyPair::generate(),
            thief_new: SigningKeyPair::generate(),
            dishonest_helper: SigningKeyPair::generate(),
        }
    }

    fn old_pk(&self) -> [u8; 32] {
        *self.lost.public_key().as_bytes()
    }

    fn new_pk(&self) -> [u8; 32] {
        *self.thief_new.public_key().as_bytes()
    }

    fn voucher_by(&self, signer: &SigningKeyPair) -> RecoveryVoucher {
        RecoveryVoucher::create(self.old_pk(), self.new_pk(), signer, None, NOW)
    }

    fn proof_from_relay(&self) -> RecoveryProof {
        let wire = WireProof {
            version: RECOVERY_PROOF_VERSION,
            old_pk: self.old_pk(),
            new_pk: self.new_pk(),
            threshold: 2,
            vouchers: vec![
                self.voucher_by(&self.lost),
                self.voucher_by(&self.dishonest_helper),
            ],
            created_at: NOW,
            expires_at: NOW + 1,
        };
        RecoveryProof::from_bytes(&postcard::to_allocvec(&wire).unwrap()).unwrap()
    }

    /// John: the lost identity and the dishonest helper are both contacts.
    fn johns_contacts(&self) -> Vec<Contact> {
        [(&self.lost, "Alice"), (&self.dishonest_helper, "Mallory")]
            .into_iter()
            .map(|(keypair, name)| {
                Contact::from_exchange(
                    *keypair.public_key().as_bytes(),
                    ContactCard::new(name),
                    SymmetricKey::generate(),
                    0,
                )
            })
            .collect()
    }
}

// @internal
#[test]
fn a_voucher_signed_by_the_lost_key_is_not_added() {
    let theft = Theft::new();
    let mut proof = RecoveryProof::new(theft.old_pk(), theft.new_pk(), 2, NOW);

    let result = proof.add_voucher(theft.voucher_by(&theft.lost));

    assert!(matches!(result, Err(RecoveryError::SelfVouching)));
    assert_eq!(proof.voucher_count(), 0);
}

// @internal
#[test]
fn a_proof_whose_voucher_is_signed_by_the_lost_key_is_invalid() {
    let theft = Theft::new();

    let result = theft.proof_from_relay().validate();

    assert!(matches!(result, Err(RecoveryError::SelfVouching)));
}

// @internal
#[test]
fn the_lost_key_does_not_count_as_a_mutual_voucher() {
    let theft = Theft::new();
    let settings = RecoverySettings::new(2, 2).unwrap();

    let result = theft
        .proof_from_relay()
        .verify_for_contact(&theft.johns_contacts(), &settings);

    match result {
        VerificationResult::MediumConfidence {
            mutual_vouchers, ..
        } => assert_eq!(mutual_vouchers, vec!["Mallory".to_string()]),
        other => panic!("one honest-looking voucher is not enough, got {other:?}"),
    }
}
