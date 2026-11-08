// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Blocking a contact (ADR-056): silent in both directions, out of every
//! group and the emergency trusted list, with at most one final update
//! emptying my card on their device.

use crate::contact::{BlockReport, Contact};
use crate::rng::SecureRngExt;

use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::ProfileEngine;

impl ProfileEngine {
    /// Blocks a contact.
    ///
    /// Blocked contacts will not receive card updates and their incoming
    /// updates will be rejected.
    pub fn block_contact(&self, id: &str) -> VauchiResult<()> {
        self.block_contact_and_report(id, false).map(drop)
    }

    /// Blocks a contact the way ADR-056 decides it: silently, out of every
    /// group (unblocking does not put them back), off the emergency trusted
    /// list, and with at most one final update emptying my card on their
    /// device when `wipe_their_copy` is confirmed. The wipe is sealed before
    /// the flag is set, since nothing is sealed for a blocked contact; a
    /// contact without a session cannot be wiped, and the report says so.
    pub fn block_contact_and_report(
        &self,
        id: &str,
        wipe_their_copy: bool,
    ) -> VauchiResult<BlockReport> {
        self.storage
            .contacts()
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        let wipe_queued = wipe_their_copy
            && !self.in_duress_mode()
            && self.queue_final_empty_card_for_contact(id).is_ok();

        let groups = self.get_groups_for_contact(id)?;
        for group in &groups {
            self.remove_contact_from_group(group.id(), id)?;
        }

        let mut contact = self
            .storage
            .contacts()
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.block();
        self.storage.contacts().save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactBlocked {
            contact_id: id.to_string(),
        });
        self.record_sync_item(crate::sync::SyncItem::ContactBlocked {
            contact_id: id.to_string(),
            timestamp: self.now_timestamp(),
        });
        let removed_from_emergency_contacts = self.remove_from_emergency_contacts(id)?;
        Ok(BlockReport {
            wipe_queued,
            groups_left: groups.len(),
            removed_from_emergency_contacts,
        })
    }

    /// Unblocks a contact.
    pub fn unblock_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .contacts()
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.unblock();
        self.storage.contacts().save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactUnblocked {
            contact_id: id.to_string(),
        });
        self.record_sync_item(crate::sync::SyncItem::ContactUnblocked {
            contact_id: id.to_string(),
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    /// Lists all blocked contacts.
    pub fn list_blocked_contacts(&self) -> VauchiResult<Vec<Contact>> {
        if self.auth_mode == super::AuthMode::Duress {
            return Ok(Vec::new());
        }
        let contacts = self.storage.contacts().list_contacts()?;
        Ok(contacts.into_iter().filter(|c| c.is_blocked()).collect())
    }

    /// One last update emptying my card on a contact's device, for a block
    /// whose wipe the user confirmed (ADR-056 point 4). Not filtered by
    /// visibility: a removal reveals nothing, and it must reach fields a
    /// past grant delivered. Nothing to remove is a success: their copy is
    /// already empty.
    pub(crate) fn queue_final_empty_card_for_contact(&self, contact_id: &str) -> VauchiResult<()> {
        use crate::card::ContactCard;
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::CardDelta;

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let mut contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", contact_id)))?;
        if contact.is_blocked() {
            return Err(VauchiError::ContactBlocked(contact_id.to_string()));
        }
        contact
            .kind()
            .exchanged_data()
            .ok_or_else(|| VauchiError::InvalidState("contact not exchanged".into()))?;

        let our_card = self
            .storage
            .contacts()
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(identity.display_name()));
        let empty_card = ContactCard::new(identity.display_name());
        let now = self.clock.unix_seconds();
        let delta = CardDelta::compute(&our_card, &empty_card, now);
        if delta.is_empty() {
            return Ok(());
        }

        let relay_url = contact.relay_url().map(String::from);
        self.seal_and_persist_card_delta(identity, &mut contact, delta, |prepared| {
            for payload in prepared {
                self.storage.pending().queue_update(&PendingUpdate {
                    id: self.rng.uuid_v4(),
                    contact_id: contact_id.to_string(),
                    update_type: "card_delta".to_string(),
                    payload: payload.encrypted.clone(),
                    created_at: now,
                    retry_count: 0,
                    status: UpdateStatus::Pending,
                    target_relay_url: relay_url.clone(),
                    target_device_id: (payload.peer_device_id != [0u8; 32])
                        .then_some(payload.peer_device_id),
                })?;
            }
            Ok(())
        })?;
        Ok(())
    }
}
