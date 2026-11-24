// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The two directions between one own device and one peer device
//! (vauchi/private#1375, decision A1): each seeded by the contact's shared
//! key under the label of its sender and receiver device, so both sides
//! derive the same seed epoch with nothing exchanged.

use super::direction::{Receiver, Sender};
use super::keys::{Label, seed_epoch};
use crate::crypto::X25519KeyPair;

/// The sender toward `peer_device_public` and the receiver from it.
pub fn open_directions(
    shared_key: &[u8; 32],
    own_device: &X25519KeyPair,
    peer_device_public: &[u8; 32],
    now: u64,
) -> (Sender, Receiver) {
    let own_public = *own_device.public_key();
    let outgoing = Label::new(&own_public, peer_device_public);
    let incoming = Label::new(peer_device_public, &own_public);
    let sender = Sender::new(
        seed_epoch(shared_key, &outgoing),
        outgoing,
        X25519KeyPair::from_bytes(*own_device.secret_bytes()),
        *peer_device_public,
        now,
    );
    let receiver = Receiver::new(
        seed_epoch(shared_key, &incoming),
        incoming,
        X25519KeyPair::from_bytes(*own_device.secret_bytes()),
        *peer_device_public,
        now,
    );
    (sender, receiver)
}

/// A peer device's routing ID and exchange public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerDevice {
    pub device_id: [u8; 32],
    pub exchange_public: [u8; 32],
}
