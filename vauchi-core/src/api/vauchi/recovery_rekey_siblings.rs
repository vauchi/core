// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A rekey one device took, carried to this identity's other devices
//! (vauchi/private#796, D25).
//!
//! Only one of the contact's devices fetches the offer, the continuation
//! or the exchange that moves a recovered contact to its new identity;
//! the others would keep the old key and shared key and could not open
//! what the new identity sends them. The device that accepted journals
//! the result, and a sibling moves or folds the contact the same way. It
//! does not answer the offer again, and its own sessions with the new
//! identity start with the device-registry handshake, which the move
//! leaves to run again.

use super::ProfileEngine;
use crate::api::error::{VauchiError, VauchiResult};
use crate::crypto::SymmetricKey;
use crate::sync::device_sync::{ContactRekeySyncData, RekeySettledSyncData, SyncItem};

impl ProfileEngine {
    /// Journals for the siblings that the contact `old_id` is now `new_id`.
    ///
    /// # Errors
    /// Storage errors; a missing or non-exchanged `new_id` journals nothing.
    #[cfg(feature = "network-http")]
    pub(super) fn record_contact_rekeyed(&self, old_id: &str, new_id: &str) -> VauchiResult<()> {
        if let Some(item) = crate::api::sync::contact_rekeyed_item(&self.storage, old_id, new_id)? {
            self.record_sync_item(item);
        }
        Ok(())
    }

    /// Applies a sibling's rekey: folds the old contact into the new
    /// identity when this device holds it already, else moves the old
    /// contact to the new key with the new shared key. A contact this
    /// device does not hold stays absent.
    ///
    /// # Errors
    /// [`VauchiError::InvalidState`] when the item names the old key as the
    /// new one or the old contact is not an exchanged one; storage errors.
    pub(super) fn apply_contact_rekeyed(
        &self,
        rekey: &ContactRekeySyncData,
        timestamp: u64,
    ) -> VauchiResult<()> {
        let old_id = rekey.old_contact_id.as_str();
        let new_pk = *rekey.new_public_key.as_bytes();
        let new_id = hex::encode(new_pk);
        if new_id == old_id {
            return Err(VauchiError::InvalidState(
                "a rekey must name a new key".into(),
            ));
        }
        let contacts = self.storage.contacts();
        if contacts.load_contact(old_id)?.is_none() {
            return Ok(());
        }
        if contacts.load_contact(&new_id)?.is_some() {
            self.storage.fold_contact_into(old_id, &new_id)?;
            return Ok(());
        }
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            self.storage.move_contact_id(old_id, &new_id)?;
            let mut moved = contacts
                .load_contact(&new_id)?
                .ok_or_else(|| VauchiError::NotFound(new_id.clone()))?;
            moved
                .accept_recovery(
                    new_pk,
                    SymmetricKey::from_bytes(rekey.shared_key),
                    timestamp,
                )
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            contacts.save_contact(&moved)?;
            self.storage
                .ratchets()
                .delete_ratchet_states_for_contact(&new_id)?;
            Ok(())
        })
    }

    /// Journals for the recovered identity's other devices that the
    /// restored contact `contact_id` no longer waits, with its shared key
    /// now (D26).
    ///
    /// # Errors
    /// Storage errors; a contact without a shared key journals nothing.
    pub(super) fn record_rekey_settled(&self, contact_id: &str) -> VauchiResult<()> {
        let Some(contact) = self.storage.contacts().load_contact(contact_id)? else {
            return Ok(());
        };
        let Some(shared_key) = contact.shared_key() else {
            return Ok(());
        };
        self.record_sync_item(SyncItem::RekeySettled {
            settled: RekeySettledSyncData {
                contact_id: contact_id.to_string(),
                shared_key: *shared_key.as_bytes(),
            },
            timestamp: self.clock.unix_seconds(),
        });
        Ok(())
    }

    /// Applies a sibling's settled rekey: a contact this device still has
    /// waiting takes the shared key and stops waiting; a new key ends this
    /// device's old sessions with it, and the handshake starts new ones.
    ///
    /// # Errors
    /// Storage errors.
    pub(super) fn apply_rekey_settled(&self, settled: &RekeySettledSyncData) -> VauchiResult<()> {
        let contact_id = settled.contact_id.as_str();
        let recovery = self.storage.recovery();
        if !recovery.rekey_pending()?.iter().any(|id| id == contact_id) {
            return Ok(());
        }
        let Some(mut contact) = self.storage.contacts().load_contact(contact_id)? else {
            return Ok(());
        };
        let key_changed = contact
            .shared_key()
            .is_none_or(|held| held.as_bytes() != &settled.shared_key);
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            if key_changed {
                contact
                    .adopt_exchange_shared_key(
                        SymmetricKey::from_bytes(settled.shared_key),
                        self.clock.unix_seconds(),
                    )
                    .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
                self.storage.contacts().save_contact(&contact)?;
                self.storage
                    .ratchets()
                    .delete_ratchet_states_for_contact(contact_id)?;
            }
            recovery.clear_rekey_pending(contact_id)?;
            Ok(())
        })
    }
}
