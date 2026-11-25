// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Authenticated application bodies inside cohort content records (#1375).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::content::MAX_BODY_BYTES;
use crate::card::{CURRENT_CARD_SCHEMA_VERSION, ContactCard, ContactField};
use crate::crypto::{PublicKey, Signature, SigningKeyPair};
use crate::identity::Identity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardError {
    BodySize,
    Format,
    Authentication,
    Malformed,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCard {
    format: u8,
    revision: u64,
    device: String,
    card: Zeroizing<String>,
    identity_signature: String,
    device_signature: String,
}

pub struct VerifiedCard {
    pub revision: u64,
    pub card: ContactCard,
}

pub(crate) fn shared_card(card: &ContactCard) -> ContactCard {
    let mut card = card.clone();
    *card.field_visibility_mut() = Default::default();
    *card.fields_mut() = card
        .fields()
        .iter()
        .map(ContactField::strip_private)
        .collect();
    card
}

impl SignedCard {
    /// Both signatures bind the exact card bytes, owner, device and revision.
    /// A recipient verifies the device key against the held active registry.
    pub fn sign(
        card: &ContactCard,
        owner: &Identity,
        device: &SigningKeyPair,
        revision: u64,
    ) -> Result<Zeroizing<Vec<u8>>, CardError> {
        if revision == 0 {
            return Err(CardError::Format);
        }
        let card = Zeroizing::new(
            serde_json::to_string(&shared_card(card)).map_err(|_| CardError::Malformed)?,
        );
        let message = signing_data(
            owner.signing_public_key(),
            owner.device_id(),
            revision,
            card.as_bytes(),
        );
        let identity_signature = owner.sign(&message);
        let device_signature = device.sign(&device_signing_data(
            &message,
            identity_signature.as_bytes(),
        ));
        let publication = Self {
            format: 1,
            revision,
            device: hex::encode(owner.device_id()),
            card,
            identity_signature: hex::encode(identity_signature.as_bytes()),
            device_signature: hex::encode(device_signature.as_bytes()),
        };
        let bytes =
            Zeroizing::new(serde_json::to_vec(&publication).map_err(|_| CardError::Malformed)?);
        if bytes.len() > MAX_BODY_BYTES {
            return Err(CardError::BodySize);
        }
        Ok(bytes)
    }

    /// Decode bounds are checked before parsing; no card reaches a caller until
    /// both the pinned owner key and the active device key have verified it.
    pub fn verify(
        bytes: &[u8],
        owner: &[u8; 32],
        device: &[u8; 32],
        device_key: &[u8; 32],
    ) -> Result<VerifiedCard, CardError> {
        if bytes.len() > MAX_BODY_BYTES {
            return Err(CardError::BodySize);
        }
        let publication: Self = serde_json::from_slice(bytes).map_err(|_| CardError::Malformed)?;
        if publication.format != 1 || publication.revision == 0 {
            return Err(CardError::Format);
        }
        if publication.device != hex::encode(device) {
            return Err(CardError::Authentication);
        }
        let identity_signature = signature(&publication.identity_signature)?;
        let device_signature = signature(&publication.device_signature)?;
        let message = signing_data(
            owner,
            device,
            publication.revision,
            publication.card.as_bytes(),
        );
        if !PublicKey::from_bytes(*owner).verify(&message, &identity_signature)
            || !PublicKey::from_bytes(*device_key).verify(
                &device_signing_data(&message, identity_signature.as_bytes()),
                &device_signature,
            )
        {
            return Err(CardError::Authentication);
        }
        let card: ContactCard =
            serde_json::from_str(&publication.card).map_err(|_| CardError::Malformed)?;
        if card.schema_version() != CURRENT_CARD_SCHEMA_VERSION
            || !card.field_visibility().is_empty()
            || card
                .fields()
                .iter()
                .any(|field| field.note().is_some() || field.validate().is_err())
            || card.validate_size().is_err()
            || !valid_card_fields(&card)
        {
            return Err(CardError::Malformed);
        }
        Ok(VerifiedCard {
            revision: publication.revision,
            card,
        })
    }
}

fn valid_card_fields(card: &ContactCard) -> bool {
    let mut ids = std::collections::BTreeSet::new();
    !card.display_name().trim().is_empty()
        && card.display_name().chars().count() <= crate::card::MAX_DISPLAY_NAME_LENGTH
        && card
            .bio()
            .is_none_or(|bio| bio.chars().count() <= crate::card::MAX_BIO_LENGTH)
        && card.fields().len() <= crate::card::MAX_FIELDS
        && card.fields().iter().all(|field| {
            !field.id().is_empty()
                && ids.insert(field.id())
                && field.label().chars().count() <= crate::card::MAX_LABEL_LENGTH
        })
}

fn signature(text: &str) -> Result<Signature, CardError> {
    if text.len() != 128 {
        return Err(CardError::Authentication);
    }
    let mut bytes = [0u8; 64];
    hex::decode_to_slice(text, &mut bytes).map_err(|_| CardError::Authentication)?;
    Ok(Signature::from_bytes(bytes))
}

fn signing_data(owner: &[u8; 32], device: &[u8; 32], revision: u64, card: &[u8]) -> Vec<u8> {
    let mut message = b"Vauchi_Notice_Card_v1".to_vec();
    message.extend_from_slice(owner);
    message.extend_from_slice(device);
    message.extend_from_slice(&revision.to_be_bytes());
    message.extend_from_slice(&Sha256::digest(card));
    message
}

fn device_signing_data(message: &[u8], identity_signature: &[u8; 64]) -> Vec<u8> {
    let mut data = b"Vauchi_Notice_Card_Device_v1".to_vec();
    data.extend_from_slice(message);
    data.extend_from_slice(identity_signature);
    data
}
