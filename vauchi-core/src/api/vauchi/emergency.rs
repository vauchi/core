// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency broadcast, decoy contacts, and configuration accessors.

use std::sync::Arc;

use crate::contact_card::ContactCard;
use crate::network::Transport;
use crate::storage::Storage;

use super::super::config::VauchiConfig;
use super::super::emergency::{BroadcastResult, EmergencyBroadcastConfig, MAX_TRUSTED_CONTACTS};
use super::super::error::{VauchiError, VauchiResult};
use super::super::events::{EventDispatcher, VauchiEvent};
use super::Vauchi;

impl<T: Transport> Vauchi<T> {
    // === Emergency Broadcast ===

    /// Configures the emergency broadcast system.
    ///
    /// Sets which contacts receive emergency alerts, the alert message,
    /// and whether to include device location.
    ///
    /// # Constraints
    /// - Maximum 10 trusted contacts
    /// - Contact IDs list must not be empty
    pub fn configure_emergency_broadcast(
        &mut self,
        contact_ids: Vec<String>,
        message: String,
        include_location: bool,
    ) -> VauchiResult<()> {
        if contact_ids.len() > MAX_TRUSTED_CONTACTS {
            return Err(VauchiError::InvalidState(format!(
                "maximum {} trusted contacts allowed, got {}",
                MAX_TRUSTED_CONTACTS,
                contact_ids.len()
            )));
        }

        let config = EmergencyBroadcastConfig {
            trusted_contact_ids: contact_ids,
            message,
            include_location,
        };

        self.storage.save_emergency_config(&config)?;
        Ok(())
    }

    /// Loads the emergency broadcast configuration.
    ///
    /// Returns `None` if no configuration has been set.
    pub fn load_emergency_config(&self) -> VauchiResult<Option<EmergencyBroadcastConfig>> {
        Ok(self.storage.load_emergency_config()?)
    }

    /// Sends an emergency broadcast to all trusted contacts.
    ///
    /// For each trusted contact that has an established ratchet:
    /// 1. Creates an `EmergencyAlert` payload
    /// 2. Serializes and encrypts it as a card update (indistinguishable)
    /// 3. Queues for delivery via relay
    ///
    /// Returns a `BroadcastResult` with sent/total counts.
    pub fn send_emergency_broadcast(&mut self) -> VauchiResult<BroadcastResult> {
        use crate::network::EmergencyAlert;
        use crate::storage::{PendingUpdate, UpdateStatus};

        let config = self.storage.load_emergency_config()?.ok_or_else(|| {
            VauchiError::InvalidState("emergency broadcast not configured".into())
        })?;

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let sender_id = identity.public_id();
        let total = config.trusted_contact_ids.len();
        let mut sent = 0;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for contact_id in &config.trusted_contact_ids {
            // Skip contacts that don't exist locally
            let contact = match self.storage.load_contact(contact_id)? {
                Some(c) => c,
                None => continue,
            };

            // Skip blocked contacts
            if contact.is_blocked() {
                continue;
            }

            // Skip contacts without ratchet (can't encrypt)
            let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact_id)? {
                Some(r) => r,
                None => continue,
            };

            // Create the emergency alert payload
            let alert = EmergencyAlert {
                sender_id: sender_id.clone(),
                message: config.message.clone(),
                timestamp: now,
                location: None, // Location is provided by mobile layer at send time
            };

            // Serialize the alert as JSON (same format as card delta)
            let alert_bytes = serde_json::to_vec(&alert)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Encrypt with ratchet (indistinguishable from card update)
            let ratchet_msg = ratchet
                .encrypt(&alert_bytes)
                .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Save updated ratchet state
            self.storage
                .save_ratchet_state(contact_id, &ratchet, is_initiator)?;

            // Queue for delivery (update_type = "emergency_alert" internally,
            // but on the wire it's just an EncryptedUpdate like any other)
            let update = PendingUpdate {
                id: uuid::Uuid::new_v4().to_string(),
                contact_id: contact_id.to_string(),
                update_type: "card_delta".to_string(), // Indistinguishable
                payload: encrypted,
                created_at: now,
                retry_count: 0,
                status: UpdateStatus::Pending,
            };
            self.storage.queue_update(&update)?;
            sent += 1;
        }

        // Dispatch event
        self.events.dispatch(VauchiEvent::EmergencyBroadcastSent {
            sent_count: sent,
            total,
        });

        Ok(BroadcastResult { sent, total })
    }

    /// Deletes the emergency broadcast configuration.
    pub fn delete_emergency_config(&mut self) -> VauchiResult<()> {
        self.storage.delete_emergency_config()?;
        Ok(())
    }

    // === Decoy Contacts ===

    /// Adds a decoy contact for duress mode.
    pub fn add_decoy_contact(
        &self,
        id: &str,
        display_name: &str,
        card: &ContactCard,
    ) -> VauchiResult<()> {
        self.storage.save_decoy_contact(id, display_name, card)?;
        Ok(())
    }

    /// Removes a decoy contact.
    pub fn remove_decoy_contact(&self, id: &str) -> VauchiResult<()> {
        self.storage.delete_decoy_contact(id)?;
        Ok(())
    }

    /// Lists all decoy contacts as (id, display_name, card) tuples.
    pub fn list_decoy_contacts(&self) -> VauchiResult<Vec<(String, String, ContactCard)>> {
        Ok(self.storage.load_decoy_contacts()?)
    }

    /// Clears all decoy contacts.
    pub fn clear_decoy_contacts(&self) -> VauchiResult<()> {
        self.storage.clear_all_decoy_contacts()?;
        Ok(())
    }

    // === Configuration ===

    /// Returns the current configuration.
    pub fn config(&self) -> &VauchiConfig {
        &self.config
    }

    /// Returns a reference to the storage.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Returns a reference to the event dispatcher.
    pub fn events(&self) -> &Arc<EventDispatcher> {
        &self.events
    }
}
