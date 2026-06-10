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
use crate::crypto::{DoubleRatchetState, SymmetricKey};

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
