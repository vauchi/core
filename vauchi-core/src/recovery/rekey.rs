// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rekey after a social recovery (vauchi/private#796, steps 4 to 6),
//! as `recovery_acceptance.spthy` models it.
//!
//! The new identity signs a fresh exchange key for each restored contact
//! and sends it with its recovery proof ([`RekeyOffer`]). A contact that
//! accepted the proof answers with its own exchange key, signing with its
//! identity key and naming the offer's key ([`RekeyAnswer`]). The owner
//! checks the answer against the contact's key from the backup. Both
//! derive the new shared key from the exchange and the two identities
//! ([`rekey_shared_key`]), so the old shared key, which a thief holding the
//! old device also has, plays no part in it.
//!
//! Wire forms:
//! - offer: `MAGIC_OFFER || new_pk(32) || exchange_key(32) || sig(64) ||
//!   proof_len(u32 BE) || proof`, the signature over
//!   `OFFER_DOMAIN || new_pk || exchange_key`;
//! - answer: `MAGIC_ANSWER || contact_pk(32) || exchange_key(32) ||
//!   offer_key(32) || sig(64)`, the signature over
//!   `ANSWER_DOMAIN || contact_pk || exchange_key || offer_key`.

use crate::crypto::kdf::HKDF;
use crate::crypto::{PublicKey, Signature, SymmetricKey};
use crate::exchange::X3DHKeyPair;
use crate::identity::Identity;

const OFFER_DOMAIN: &[u8] = b"vauchi-recovery-rekey-offer";
const ANSWER_DOMAIN: &[u8] = b"vauchi-recovery-rekey-answer";
/// HKDF info for the shared key a rekey derives, its own domain (ADR-007).
const REKEY_SHARED_KEY_INFO: &[u8] = b"Vauchi_Recovery_Rekey_Shared_Key_v1";
const MAGIC_OFFER: &[u8; 4] = b"VRK1";
const MAGIC_ANSWER: &[u8; 4] = b"VRK2";

/// The largest recovery proof an offer carries; a proof holds at most a
/// few vouchers of some hundred bytes each.
pub const MAX_PROOF_BYTES: usize = 16 * 1024;

const KEY: usize = 32;
const SIGNATURE: usize = 64;
const OFFER_FIXED: usize = MAGIC_OFFER.len() + KEY + KEY + SIGNATURE + 4;
const ANSWER_LEN: usize = MAGIC_ANSWER.len() + KEY + KEY + KEY + SIGNATURE;

const CONTINUATION_DOMAIN: &[u8] = b"vauchi-recovery-continue-v1";
const CONTINUATION_LEN: usize = KEY + KEY + SIGNATURE;

/// The recovered identity's word, signed with its old key, that `new_pk`
/// continues it (D20). Sent over the ratchet an in-person exchange set up,
/// so it comes from the person met; the contact folds its old contact for
/// `old_pk` into the one the meeting made. Wire form: `old_pk(32) ||
/// new_pk(32) || sig(64)`, the signature over `CONTINUATION_DOMAIN ||
/// old_pk || new_pk`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryContinuation {
    old_pk: [u8; KEY],
    new_pk: [u8; KEY],
    signature: [u8; SIGNATURE],
}

impl RecoveryContinuation {
    /// Signs, with the old identity's key pair, that `new_pk` continues it.
    pub fn create(old_keys: &crate::crypto::SigningKeyPair, new_pk: &[u8; KEY]) -> Self {
        let old_pk = *old_keys.public_key().as_bytes();
        let signature = *old_keys
            .sign(&continuation_message(&old_pk, new_pk))
            .as_bytes();
        Self {
            old_pk,
            new_pk: *new_pk,
            signature,
        }
    }

    pub fn old_pk(&self) -> &[u8; KEY] {
        &self.old_pk
    }

    pub fn new_pk(&self) -> &[u8; KEY] {
        &self.new_pk
    }

    /// Whether the old key signed it.
    pub fn verify(&self) -> bool {
        PublicKey::from_bytes(self.old_pk).verify(
            &continuation_message(&self.old_pk, &self.new_pk),
            &Signature::from_bytes(self.signature),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        [&self.old_pk[..], &self.new_pk[..], &self.signature[..]].concat()
    }

    /// # Errors
    /// [`RekeyError::Malformed`] for any other length.
    pub fn decode(bytes: &[u8]) -> Result<Self, RekeyError> {
        if bytes.len() != CONTINUATION_LEN {
            return Err(RekeyError::Malformed);
        }
        let (old_pk, rest) = take::<KEY>(bytes)?;
        let (new_pk, rest) = take::<KEY>(rest)?;
        let (signature, _) = take::<SIGNATURE>(rest)?;
        Ok(Self {
            old_pk,
            new_pk,
            signature,
        })
    }
}

fn continuation_message(old_pk: &[u8; KEY], new_pk: &[u8; KEY]) -> Vec<u8> {
    [CONTINUATION_DOMAIN, old_pk, new_pk].concat()
}

/// Whether `bytes` is meant as a rekey answer; the receive path routes it
/// to the recovering phone instead of to the contact router.
pub fn is_rekey_answer(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC_ANSWER)
}

/// Whether `bytes` is meant as a rekey offer; the receive path routes it
/// here instead of to the contact router.
pub fn is_rekey_offer(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC_OFFER)
}

/// Why a rekey message could not be built or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RekeyError {
    /// The bytes are not a rekey message of this kind, or a part is out of
    /// bounds.
    #[error("Malformed rekey message")]
    Malformed,
    /// The exchange keys give no usable shared secret.
    #[error("The rekey exchange key is unusable")]
    WeakExchangeKey,
}

/// The new identity's signed exchange key for one restored contact, with
/// the recovery proof that names that identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyOffer {
    new_pk: [u8; KEY],
    exchange_key: [u8; KEY],
    signature: [u8; SIGNATURE],
    proof: Vec<u8>,
}

impl RekeyOffer {
    /// Signs `exchange_key` with `identity`, the recovering new identity.
    ///
    /// # Errors
    /// [`RekeyError::Malformed`] for an empty proof or one over
    /// [`MAX_PROOF_BYTES`].
    pub fn create(
        identity: &Identity,
        exchange_key: [u8; KEY],
        proof: Vec<u8>,
    ) -> Result<Self, RekeyError> {
        if proof.is_empty() || proof.len() > MAX_PROOF_BYTES {
            return Err(RekeyError::Malformed);
        }
        let new_pk = *identity.signing_public_key();
        let signature = *identity
            .sign(&offer_message(&new_pk, &exchange_key))
            .as_bytes();
        Ok(Self {
            new_pk,
            exchange_key,
            signature,
            proof,
        })
    }

    pub fn new_pk(&self) -> &[u8; KEY] {
        &self.new_pk
    }

    pub fn exchange_key(&self) -> &[u8; KEY] {
        &self.exchange_key
    }

    pub fn proof(&self) -> &[u8] {
        &self.proof
    }

    /// Whether the offer is signed by the identity it names. The caller
    /// still checks that its accepted proof names that identity.
    pub fn verify(&self) -> bool {
        PublicKey::from_bytes(self.new_pk).verify(
            &offer_message(&self.new_pk, &self.exchange_key),
            &Signature::from_bytes(self.signature),
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(OFFER_FIXED.saturating_add(self.proof.len()));
        bytes.extend_from_slice(MAGIC_OFFER);
        bytes.extend_from_slice(&self.new_pk);
        bytes.extend_from_slice(&self.exchange_key);
        bytes.extend_from_slice(&self.signature);
        bytes.extend_from_slice(&(self.proof.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.proof);
        bytes
    }

    /// Reads an offer; it is not yet verified.
    ///
    /// # Errors
    /// [`RekeyError::Malformed`] for other bytes, a proof out of bounds or
    /// a length that disagrees with the bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, RekeyError> {
        let rest = bytes
            .strip_prefix(MAGIC_OFFER.as_slice())
            .ok_or(RekeyError::Malformed)?;
        let (new_pk, rest) = take::<KEY>(rest)?;
        let (exchange_key, rest) = take::<KEY>(rest)?;
        let (signature, rest) = take::<SIGNATURE>(rest)?;
        let (length, proof) = take::<4>(rest)?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > MAX_PROOF_BYTES || proof.len() != length {
            return Err(RekeyError::Malformed);
        }
        Ok(Self {
            new_pk,
            exchange_key,
            signature,
            proof: proof.to_vec(),
        })
    }
}

/// A contact's signed answer to an offer: its own exchange key, naming the
/// offer's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyAnswer {
    contact_pk: [u8; KEY],
    exchange_key: [u8; KEY],
    offer_key: [u8; KEY],
    signature: [u8; SIGNATURE],
}

impl RekeyAnswer {
    /// Answers the offer whose exchange key is `offer_key`, signing with
    /// the contact's `identity`.
    pub fn create(identity: &Identity, exchange_key: [u8; KEY], offer_key: [u8; KEY]) -> Self {
        let contact_pk = *identity.signing_public_key();
        let signature = *identity
            .sign(&answer_message(&contact_pk, &exchange_key, &offer_key))
            .as_bytes();
        Self {
            contact_pk,
            exchange_key,
            offer_key,
            signature,
        }
    }

    pub fn contact_pk(&self) -> &[u8; KEY] {
        &self.contact_pk
    }

    pub fn exchange_key(&self) -> &[u8; KEY] {
        &self.exchange_key
    }

    /// Whether this answers the offer with `offer_key`, signed by the
    /// contact whose key the backup holds, `expected_contact_pk`.
    pub fn verify(&self, expected_contact_pk: &[u8; KEY], offer_key: &[u8; KEY]) -> bool {
        self.contact_pk == *expected_contact_pk
            && self.offer_key == *offer_key
            && PublicKey::from_bytes(self.contact_pk).verify(
                &answer_message(&self.contact_pk, &self.exchange_key, &self.offer_key),
                &Signature::from_bytes(self.signature),
            )
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(ANSWER_LEN);
        bytes.extend_from_slice(MAGIC_ANSWER);
        bytes.extend_from_slice(&self.contact_pk);
        bytes.extend_from_slice(&self.exchange_key);
        bytes.extend_from_slice(&self.offer_key);
        bytes.extend_from_slice(&self.signature);
        bytes
    }

    /// Reads an answer; it is not yet verified.
    ///
    /// # Errors
    /// [`RekeyError::Malformed`] for other bytes or any other length.
    pub fn decode(bytes: &[u8]) -> Result<Self, RekeyError> {
        if bytes.len() != ANSWER_LEN {
            return Err(RekeyError::Malformed);
        }
        let rest = bytes
            .strip_prefix(MAGIC_ANSWER.as_slice())
            .ok_or(RekeyError::Malformed)?;
        let (contact_pk, rest) = take::<KEY>(rest)?;
        let (exchange_key, rest) = take::<KEY>(rest)?;
        let (offer_key, rest) = take::<KEY>(rest)?;
        let (signature, _) = take::<SIGNATURE>(rest)?;
        Ok(Self {
            contact_pk,
            exchange_key,
            offer_key,
            signature,
        })
    }
}

/// The shared key both sides derive: HKDF of the exchange's Diffie-Hellman
/// secret, salted with the new identity's key and the contact's, so it
/// binds the two identities the offer and answer name.
///
/// # Errors
/// [`RekeyError::WeakExchangeKey`] when `their_exchange_key` gives no
/// usable secret (a low-order point).
pub fn rekey_shared_key(
    own_exchange: &X3DHKeyPair,
    their_exchange_key: &[u8; KEY],
    new_pk: &[u8; KEY],
    contact_pk: &[u8; KEY],
) -> Result<SymmetricKey, RekeyError> {
    let secret = own_exchange
        .diffie_hellman(their_exchange_key)
        .map_err(|_| RekeyError::WeakExchangeKey)?;
    let mut identities = [0u8; KEY * 2];
    identities[..KEY].copy_from_slice(new_pk);
    identities[KEY..].copy_from_slice(contact_pk);
    let key = HKDF::derive_key(Some(&identities), &*secret, REKEY_SHARED_KEY_INFO);
    Ok(SymmetricKey::from_bytes(*key))
}

fn offer_message(new_pk: &[u8; KEY], exchange_key: &[u8; KEY]) -> Vec<u8> {
    [OFFER_DOMAIN, new_pk, exchange_key].concat()
}

fn answer_message(
    contact_pk: &[u8; KEY],
    exchange_key: &[u8; KEY],
    offer_key: &[u8; KEY],
) -> Vec<u8> {
    [ANSWER_DOMAIN, contact_pk, exchange_key, offer_key].concat()
}

fn take<const N: usize>(bytes: &[u8]) -> Result<([u8; N], &[u8]), RekeyError> {
    let (head, rest) = bytes
        .split_first_chunk::<N>()
        .ok_or(RekeyError::Malformed)?;
    Ok((*head, rest))
}
