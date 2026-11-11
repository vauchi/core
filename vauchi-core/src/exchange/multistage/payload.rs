// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The payload a multi-stage session commits to and its peer opens:
//! `[2][identity key:32][signature:64][card json]`.
//!
//! The identity key signs the sender's own session id, ephemeral key, relay
//! URL and card, so a peer cannot claim an identity it does not hold
//! (vauchi/private#1217). The signature covers only the sender's side: a
//! payload is committed before the peer is known. Replaying one in another
//! session is stopped by the keyed confirmation, which only a holder of an
//! ephemeral secret of that session can compute (Tamarin
//! `verification/multistage_exchange.spthy`).

use crate::card::{ContactCard, MAX_CARD_SIZE_BYTES, MAX_FIELDS};
use crate::crypto::signing::{Signature, verify_signature};
use crate::identity::Identity;

pub const PAYLOAD_VERSION: u8 = 2;
const DOMAIN: &[u8] = b"vauchi-multistage-payload-v2";
const HEADER_LEN: usize = 1 + 32 + 64;

/// A peer's opened and verified payload.
#[derive(Debug, Clone)]
pub struct PeerPayload {
    pub identity_pk: [u8; 32],
    pub card: ContactCard,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    #[error("payload is not version {PAYLOAD_VERSION}")]
    Version,
    #[error("payload is shorter than its header")]
    Truncated,
    #[error("payload card exceeds the card size limit")]
    TooLarge,
    #[error("payload signature does not verify for the identity it carries")]
    Signature,
    #[error("payload carries our own identity")]
    OwnIdentity,
    #[error("payload card is invalid")]
    Card,
}

fn signed_message(
    session_id: &[u8; 16],
    ephemeral: &[u8; 32],
    identity_pk: &[u8; 32],
    relay_url: Option<&str>,
    card_json: &[u8],
) -> Vec<u8> {
    let relay = relay_url.unwrap_or("").as_bytes();
    let relay_len = u16::try_from(relay.len()).unwrap_or(u16::MAX);
    let mut out =
        Vec::with_capacity(DOMAIN.len() + 16 + 32 + 32 + 2 + relay.len() + card_json.len());
    out.extend_from_slice(DOMAIN);
    out.extend_from_slice(session_id);
    out.extend_from_slice(ephemeral);
    out.extend_from_slice(identity_pk);
    out.extend_from_slice(&relay_len.to_be_bytes());
    out.extend_from_slice(&relay[..usize::from(relay_len)]);
    out.extend_from_slice(card_json);
    out
}

/// Builds and signs our payload for this session's id, ephemeral and relay.
pub fn build(
    identity: &Identity,
    session_id: &[u8; 16],
    ephemeral: &[u8; 32],
    relay_url: Option<&str>,
    card: &ContactCard,
) -> Vec<u8> {
    let card_json = serde_json::to_vec(card).expect("ContactCard serialization should not fail");
    let identity_pk = *identity.signing_public_key();
    let signature = identity.sign(&signed_message(
        session_id,
        ephemeral,
        &identity_pk,
        relay_url,
        &card_json,
    ));
    let mut out = Vec::with_capacity(HEADER_LEN + card_json.len());
    out.push(PAYLOAD_VERSION);
    out.extend_from_slice(&identity_pk);
    out.extend_from_slice(signature.as_bytes());
    out.extend_from_slice(&card_json);
    out
}

/// Opens the peer's payload and checks it against the peer's session id,
/// ephemeral and relay URL from its INIT: version, signature, that the
/// identity is not ours, and the card's size and fields.
pub fn open(
    bytes: &[u8],
    peer_session_id: &[u8; 16],
    peer_ephemeral: &[u8; 32],
    peer_relay_url: Option<&str>,
    own_identity: &[u8; 32],
) -> Result<PeerPayload, PayloadError> {
    if bytes.first() != Some(&PAYLOAD_VERSION) {
        return Err(PayloadError::Version);
    }
    if bytes.len() < HEADER_LEN {
        return Err(PayloadError::Truncated);
    }
    let card_json = &bytes[HEADER_LEN..];
    if card_json.len() > MAX_CARD_SIZE_BYTES {
        return Err(PayloadError::TooLarge);
    }
    let identity_pk: [u8; 32] = bytes[1..33]
        .try_into()
        .map_err(|_| PayloadError::Truncated)?;
    let signature_bytes: [u8; 64] = bytes[33..HEADER_LEN]
        .try_into()
        .map_err(|_| PayloadError::Truncated)?;
    let message = signed_message(
        peer_session_id,
        peer_ephemeral,
        &identity_pk,
        peer_relay_url,
        card_json,
    );
    if !verify_signature(
        &identity_pk,
        &message,
        &Signature::from_bytes(signature_bytes),
    ) {
        return Err(PayloadError::Signature);
    }
    if identity_pk == *own_identity {
        return Err(PayloadError::OwnIdentity);
    }
    let card: ContactCard = serde_json::from_slice(card_json).map_err(|_| PayloadError::Card)?;
    if card.fields().len() > MAX_FIELDS || card.fields().iter().any(|f| f.validate().is_err()) {
        return Err(PayloadError::Card);
    }
    Ok(PeerPayload { identity_pk, card })
}
