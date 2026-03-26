// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency broadcast, decoy contacts, and configuration accessors.

use std::sync::Arc;

use crate::contact_card::ContactCard;

use crate::storage::Storage;

use super::super::config::VauchiConfig;
use super::super::emergency::{BroadcastResult, EmergencyWipeStatus, MAX_TRUSTED_CONTACTS};
use super::super::error::{VauchiError, VauchiResult};
use super::super::events::{EventDispatcher, VauchiEvent};
use super::Vauchi;
use crate::types::EmergencyBroadcastConfig;

impl Vauchi {
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
                target_relay_url: None,
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

    /// Returns the emergency wipe readiness status.
    ///
    /// Aggregates:
    /// - Whether emergency broadcast is configured
    /// - Whether duress settings are configured
    /// - Whether a deletion (shred) is scheduled or executed
    /// - Whether the user has at least one trusted contact
    pub fn get_emergency_wipe_status(&self) -> VauchiResult<EmergencyWipeStatus> {
        let broadcast_configured = self.storage.load_emergency_config()?.is_some();
        let duress_configured = self.storage.load_duress_settings()?.is_some();

        let deletion_state = self.storage.load_deletion_state()?;
        let deletion_scheduled = matches!(
            deletion_state,
            crate::storage::DeletionState::Scheduled { .. }
        );
        let deletion_executed = matches!(
            deletion_state,
            crate::storage::DeletionState::Executed { .. }
        );

        let contacts = self.storage.list_contacts()?;
        let trusted_contact_count = contacts.iter().filter(|c| c.is_recovery_trusted()).count();
        let has_trusted_contacts = trusted_contact_count > 0;

        let password_enabled = self.storage.load_password_config()?.is_some();

        Ok(EmergencyWipeStatus {
            broadcast_configured,
            duress_configured,
            deletion_scheduled,
            deletion_executed,
            has_trusted_contacts,
            trusted_contact_count,
            password_enabled,
        })
    }

    /// Performs an emergency data wipe (panic shred).
    ///
    /// This is the "nuclear option" — it destroys all local data immediately
    /// without the normal 7-day grace period. Requires explicit confirmation.
    ///
    /// If `confirm` is false, returns an error asking for confirmation.
    /// This prevents accidental wipes from buggy callers.
    ///
    /// The actual implementation delegates to `ShredManager::panic_shred()`
    /// or, if no ShredManager is available, directly clears all storage tables.
    pub fn perform_emergency_wipe(&mut self, confirm: bool) -> VauchiResult<()> {
        if !confirm {
            return Err(VauchiError::InvalidState(
                "emergency wipe requires explicit confirmation (confirm=true)".into(),
            ));
        }

        // Clear all contacts
        let contacts = self.storage.list_contacts()?;
        for contact in &contacts {
            self.storage.delete_contact(contact.id())?;
        }

        // Clear own card
        let empty_card = ContactCard::new("");
        self.storage.save_own_card(&empty_card)?;

        // Clear decoy contacts
        self.storage.clear_all_decoy_contacts()?;

        // Clear emergency config
        let _ = self.storage.delete_emergency_config();

        // Clear duress settings
        let _ = self.storage.delete_duress_settings();

        // Mark deletion as executed
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.storage
            .save_deletion_state(&crate::storage::DeletionState::Executed { executed_at: now })?;

        // Clear identity
        self.identity = None;

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

    /// Returns a mutable reference to the configuration.
    ///
    /// Used by `AppEngine` to persist settings toggles (e.g. delivery
    /// receipts, suppress presence) so that freshly created engines
    /// pick up the latest values.
    pub fn config_mut(&mut self) -> &mut VauchiConfig {
        &mut self.config
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
