// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Role-correct Double Ratchet bootstrap shared by exchange sessions.
//!
//! WHY: feeding the Ed25519 identity key as the DH key, or
//! initializing both peers as initiator, silently produces an
//! undecryptable channel. Every exchange session routes through this
//! one function so both mistakes stay unrepresentable in exactly one
//! place — the role rule was previously duplicated per session type
//! and the same role bug had to be fixed twice (2026-05-25).

use super::key_order;
use crate::crypto::x3dh::X3DHKeyPair;
use crate::crypto::{DoubleRatchetState, HKDF, SymmetricKey};

#[derive(Debug, PartialEq, Eq)]
pub enum RatchetBootstrapError {
    /// The initiator role keys off the peer's exchange ephemeral,
    /// which the caller did not retain.
    MissingPeerEphemeral,
    /// The responder role keys off our own exchange ephemeral,
    /// which the caller did not retain.
    MissingOurEphemeral,
    /// Underlying ratchet initialization failed.
    Init(String),
}

/// Establishes an independent ratchet for one concrete local/peer device pair.
///
/// Both devices derive the same relationship- and topology-bound secret from
/// their static device exchange keys. The lexicographically smaller
/// `(identity_key, device_id)` endpoint is the ratchet initiator. Ratchet state
/// is never copied between linked devices.
#[allow(clippy::too_many_arguments)]
pub fn bootstrap_device_pair_ratchet(
    relationship_key: &SymmetricKey,
    our_identity: &[u8; 32],
    our_device_id: &[u8; 32],
    our_device_keypair: &X3DHKeyPair,
    their_identity: &[u8; 32],
    their_device_id: &[u8; 32],
    their_device_public_key: &[u8; 32],
) -> Result<(DoubleRatchetState, bool), RatchetBootstrapError> {
    let dh = our_device_keypair
        .diffie_hellman(their_device_public_key)
        .map_err(|error| RatchetBootstrapError::Init(error.to_string()))?;
    let ours = (our_identity.as_slice(), our_device_id.as_slice());
    let theirs = (their_identity.as_slice(), their_device_id.as_slice());
    let is_initiator = ours < theirs;
    let (first_identity, first_device, second_identity, second_device) = if is_initiator {
        (our_identity, our_device_id, their_identity, their_device_id)
    } else {
        (their_identity, their_device_id, our_identity, our_device_id)
    };
    let mut ikm = Vec::with_capacity(64);
    ikm.extend_from_slice(&*dh);
    ikm.extend_from_slice(relationship_key.as_bytes());
    let mut info = b"vauchi-device-pair-ratchet-v1".to_vec();
    info.extend_from_slice(first_identity);
    info.extend_from_slice(first_device);
    info.extend_from_slice(second_identity);
    info.extend_from_slice(second_device);
    let pair_key = SymmetricKey::from_bytes(*HKDF::derive_key(None, &ikm, &info));

    let ratchet = if is_initiator {
        DoubleRatchetState::initialize_initiator(&pair_key, *their_device_public_key)
            .map_err(|error| RatchetBootstrapError::Init(error.to_string()))?
    } else {
        DoubleRatchetState::initialize_responder(
            &pair_key,
            X3DHKeyPair::from_bytes(*our_device_keypair.secret_bytes()),
        )
    };
    Ok((ratchet, is_initiator))
}

/// Builds the role-correct Double Ratchet for a completed exchange.
///
/// The role is derived deterministically from the identity keys
/// (smaller = initiator). The initiator keys the ratchet off the
/// peer's exchange ephemeral public key; the responder keys it off
/// our own retained ephemeral keypair — the keypair whose public key
/// the initiator received. Both sides reconcile on the first message
/// (see [`DoubleRatchetState::dh_ratchet`]).
///
/// Pure crypto — persistence stays with the caller. Callers resolve
/// session state into the two `Option`s; an ephemeral is only
/// required for the role that uses it, so a caller may pass `None`
/// for state it never retained.
///
/// Returns the initialized ratchet and the `is_initiator` flag for
/// `Storage::save_ratchet_state`.
pub fn bootstrap_exchange_ratchet(
    shared: &SymmetricKey,
    our_identity: &[u8; 32],
    their_identity: &[u8; 32],
    peer_ephemeral: Option<[u8; 32]>,
    our_ephemeral: Option<X3DHKeyPair>,
) -> Result<(DoubleRatchetState, bool), RatchetBootstrapError> {
    let is_initiator = key_order::is_initiator(our_identity, their_identity);
    let ratchet = if is_initiator {
        let peer = peer_ephemeral.ok_or(RatchetBootstrapError::MissingPeerEphemeral)?;
        DoubleRatchetState::initialize_initiator(shared, peer)
            .map_err(|e| RatchetBootstrapError::Init(e.to_string()))?
    } else {
        let ours = our_ephemeral.ok_or(RatchetBootstrapError::MissingOurEphemeral)?;
        DoubleRatchetState::initialize_responder(shared, ours)
    };
    Ok((ratchet, is_initiator))
}
