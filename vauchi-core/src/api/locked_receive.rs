// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a locked run does with a contact's blob (ADR-033 amendment
//! 2026-11-07 and addendum 2026-11-09, vauchi/private#1093 slice 6). It
//! opens the blob on a session it already holds, then saves the advanced
//! ratchet and seals the opened update into the inbox in one transaction,
//! so the ratchet never moves past an update that was not kept. Signature,
//! replay and version checks need the contact, under the storage key; the
//! merge at the unlock runs them.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::RatchetMessage;
use crate::storage::{LockedReceiveStorage, StorageError};

/// A fetch page is capped under the OHTTP forward limit, so a larger blob
/// is not one the relay delivered.
const MAX_BLOB_LEN: usize = 128 * 1024;

/// What became of one blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockedReceipt {
    /// In the inbox, the ratchet saved with it: the blob can be
    /// acknowledged.
    Stored,
    /// Not opened on any session held, or not a ratchet message: it stays
    /// on the relay for the unlock.
    LeftForUnlock,
}

/// An update a locked run opened, as the inbox keeps it.
#[derive(Serialize, Deserialize)]
pub struct LockedInboxItem {
    contact_id: String,
    peer_device_id: [u8; 32],
    plaintext: Vec<u8>,
    received_at: u64,
}

impl Drop for LockedInboxItem {
    fn drop(&mut self) {
        self.plaintext.zeroize();
    }
}

impl LockedInboxItem {
    /// Reads an item from the bytes the inbox opened.
    pub fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        serde_json::from_slice(bytes).map_err(|e| StorageError::Serialization(e.to_string()))
    }

    fn encode(&self) -> Result<Zeroizing<Vec<u8>>, StorageError> {
        serde_json::to_vec(self)
            .map(Zeroizing::new)
            .map_err(|e| StorageError::Serialization(e.to_string()))
    }

    /// The contact whose session opened it.
    pub fn contact_id(&self) -> &str {
        &self.contact_id
    }

    /// The contact's device whose session opened it; all zeros for the
    /// shared session.
    pub fn peer_device_id(&self) -> &[u8; 32] {
        &self.peer_device_id
    }

    /// The opened update, not yet verified.
    pub fn plaintext(&self) -> &[u8] {
        &self.plaintext
    }

    /// Unix seconds when the locked run received it.
    pub fn received_at(&self) -> u64 {
        self.received_at
    }
}

/// Opens `ciphertext`, routed to `contact_id`, on the first session with
/// that contact that opens it, and keeps it for the unlock.
///
/// A failed trial is not saved, so it advances no session. A session still
/// under the storage key is skipped like one that does not open.
pub fn store_locked_update(
    storage: &LockedReceiveStorage,
    contact_id: &str,
    ciphertext: &[u8],
    received_at: u64,
) -> Result<LockedReceipt, StorageError> {
    if ciphertext.len() > MAX_BLOB_LEN {
        return Ok(LockedReceipt::LeftForUnlock);
    }
    let Ok(message) = serde_json::from_slice::<RatchetMessage>(ciphertext) else {
        return Ok(LockedReceipt::LeftForUnlock);
    };
    for peer_device_id in storage.ratchets().peer_devices(contact_id)? {
        let session = match storage
            .ratchets()
            .load_ratchet_state_for_device(contact_id, &peer_device_id)
        {
            Ok(Some(session)) => session,
            Ok(None) | Err(StorageError::StorageKeyUnavailable) => continue,
            Err(e) => return Err(e),
        };
        let (mut ratchet, is_initiator) = session;
        let Ok(plaintext) = ratchet.decrypt(&message) else {
            continue;
        };
        let item = LockedInboxItem {
            contact_id: contact_id.to_string(),
            peer_device_id,
            plaintext,
            received_at,
        }
        .encode()?;
        storage.in_transaction(|storage| {
            storage.ratchets().save_ratchet_state_for_device(
                contact_id,
                &peer_device_id,
                &ratchet,
                is_initiator,
            )?;
            storage.receive_inbox().put(&item)
        })?;
        return Ok(LockedReceipt::Stored);
    }
    Ok(LockedReceipt::LeftForUnlock)
}
