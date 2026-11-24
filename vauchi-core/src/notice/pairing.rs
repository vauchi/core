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
use crate::storage::{NoticeStateKind, Storage, StorageError};

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

/// Opens and saves the directions `contact_id` lacks with each of `peers`,
/// and returns how many devices it opened. A saved direction is never
/// opened again: that would restart its chain under keys it already used.
pub fn ensure_directions(
    storage: &Storage,
    contact_id: &str,
    shared_key: &[u8; 32],
    own_device: &X25519KeyPair,
    peers: &[PeerDevice],
    now: u64,
) -> Result<usize, StorageError> {
    let notices = storage.notices();
    let mut opened = 0;
    for peer in peers {
        let has = |kind| -> Result<bool, StorageError> {
            Ok(notices.load(contact_id, kind, &peer.device_id)?.is_some())
        };
        let (has_sender, has_receiver) = (
            has(NoticeStateKind::Sender)?,
            has(NoticeStateKind::Receiver)?,
        );
        if has_sender && has_receiver {
            continue;
        }
        // Opening is deterministic from the shared key, so a direction a
        // crash left unsaved is opened again exactly as it would have been.
        let (sender, receiver) =
            open_directions(shared_key, own_device, &peer.exchange_public, now);
        if !has_sender {
            let sender = sender.save().map_err(saved_error)?;
            notices.save(
                contact_id,
                NoticeStateKind::Sender,
                &peer.device_id,
                &sender,
            )?;
        }
        if !has_receiver {
            let receiver = receiver.save().map_err(saved_error)?;
            notices.save(
                contact_id,
                NoticeStateKind::Receiver,
                &peer.device_id,
                &receiver,
            )?;
        }
        opened += 1;
    }
    Ok(opened)
}

fn saved_error(error: crate::notice::saved::SavedError) -> StorageError {
    StorageError::Serialization(format!("notice state: {error:?}"))
}
