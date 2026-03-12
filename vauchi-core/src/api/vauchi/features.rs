// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Feature operations: aha moments, demo contact, tor, multi-relay, hide/unhide,
//! block/unblock, consent, recovery readiness, and visibility re-propagation.

use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::network::Transport;

use super::super::consent::{ConsentManager, ConsentRecord, ConsentType};
use super::super::contact_manager::ContactManager;
use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::{RecoveryReadiness, Vauchi};

impl<T: Transport> Vauchi<T> {
    // === Aha Moments Operations ===

    /// Tries to trigger an aha moment of the given type.
    ///
    /// Returns the moment if it should be shown (not yet seen).
    /// Automatically persists the "seen" state.
    pub fn try_trigger_aha_moment(
        &self,
        moment_type: crate::aha_moments::AhaMomentType,
    ) -> VauchiResult<Option<crate::aha_moments::AhaMoment>> {
        let mut tracker = self.storage.load_or_create_aha_tracker()?;
        let moment = tracker.try_trigger(moment_type);
        if moment.is_some() {
            self.storage.save_aha_tracker(&tracker)?;
        }
        Ok(moment)
    }

    /// Tries to trigger an aha moment with context.
    ///
    /// Context is used for personalized messages (e.g., contact name).
    pub fn try_trigger_aha_moment_with_context(
        &self,
        moment_type: crate::aha_moments::AhaMomentType,
        context: String,
    ) -> VauchiResult<Option<crate::aha_moments::AhaMoment>> {
        let mut tracker = self.storage.load_or_create_aha_tracker()?;
        let moment = tracker.try_trigger_with_context(moment_type, context);
        if moment.is_some() {
            self.storage.save_aha_tracker(&tracker)?;
        }
        Ok(moment)
    }

    /// Checks if an aha moment has been seen.
    pub fn has_seen_aha_moment(
        &self,
        moment_type: crate::aha_moments::AhaMomentType,
    ) -> VauchiResult<bool> {
        let tracker = self.storage.load_or_create_aha_tracker()?;
        Ok(tracker.has_seen(moment_type))
    }

    /// Gets the number of aha moments seen.
    pub fn aha_moments_seen_count(&self) -> VauchiResult<usize> {
        let tracker = self.storage.load_or_create_aha_tracker()?;
        Ok(tracker.seen_count())
    }

    /// Resets all aha moments (for testing or demo replay).
    pub fn reset_aha_moments(&self) -> VauchiResult<()> {
        let mut tracker = self.storage.load_or_create_aha_tracker()?;
        tracker.reset();
        self.storage.save_aha_tracker(&tracker)?;
        Ok(())
    }

    // === Demo Contact Operations ===

    /// Gets the current demo contact state.
    pub fn demo_contact_state(&self) -> VauchiResult<crate::demo_contact::DemoContactState> {
        Ok(self.storage.load_or_create_demo_contact_state()?)
    }

    /// Checks if the demo contact is active.
    pub fn is_demo_contact_active(&self) -> VauchiResult<bool> {
        Ok(self.storage.is_demo_contact_active()?)
    }

    /// Gets the current demo contact card (if active).
    pub fn demo_contact_card(&self) -> VauchiResult<Option<crate::demo_contact::DemoContactCard>> {
        let state = self.storage.load_or_create_demo_contact_state()?;
        if !state.is_active {
            return Ok(None);
        }
        match state.current_tip() {
            Some(tip) => Ok(Some(crate::demo_contact::generate_demo_contact_card(&tip))),
            None => Ok(None),
        }
    }

    /// Advances the demo contact to the next tip.
    ///
    /// Returns the new tip if successful.
    pub fn advance_demo_contact(&self) -> VauchiResult<Option<crate::demo_contact::DemoTip>> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        if !state.is_active {
            return Ok(None);
        }
        let tip = state.advance_to_next_tip();
        self.storage.save_demo_contact_state(&state)?;
        Ok(tip)
    }

    /// Dismisses the demo contact (user-initiated).
    pub fn dismiss_demo_contact(&self) -> VauchiResult<()> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        state.dismiss();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    /// Auto-removes the demo contact (after first real exchange).
    pub fn auto_remove_demo_contact(&self) -> VauchiResult<()> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        state.auto_remove();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    /// Restores the demo contact from settings.
    pub fn restore_demo_contact(&self) -> VauchiResult<()> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        state.restore();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    /// Initializes the demo contact for a new user.
    ///
    /// Should be called after identity creation if user has no contacts.
    pub fn initialize_demo_contact(&self) -> VauchiResult<()> {
        // Only initialize if user has no real contacts
        if self.contact_count()? > 0 {
            return Ok(());
        }

        let state = crate::demo_contact::DemoContactState::new_active();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    // === Tor Configuration ===

    /// Returns the current Tor configuration.
    pub fn tor_config(&self) -> &crate::tor_config::TorConfig {
        &self.config.tor
    }

    /// Returns the current Tor status.
    ///
    /// Without the `tor` feature enabled, this always returns `Disabled`.
    pub fn tor_status(&self) -> crate::tor_config::TorStatus {
        crate::tor_config::TorStatus::Disabled
    }

    /// Enables Tor with the current configuration.
    ///
    /// Persists the enabled state to storage.
    /// Note: Actual Tor bootstrapping requires the `tor` feature.
    pub fn enable_tor(&mut self) -> VauchiResult<()> {
        self.config.tor.enabled = true;
        self.storage.save_tor_config(&self.config.tor)?;
        self.events.dispatch(VauchiEvent::TorStatusChanged {
            status: crate::tor_config::TorStatus::Disabled,
        });
        Ok(())
    }

    /// Disables Tor.
    ///
    /// Persists the disabled state to storage.
    pub fn disable_tor(&mut self) -> VauchiResult<()> {
        self.config.tor.enabled = false;
        self.storage.save_tor_config(&self.config.tor)?;
        self.events.dispatch(VauchiEvent::TorStatusChanged {
            status: crate::tor_config::TorStatus::Disabled,
        });
        Ok(())
    }

    /// Configures Tor bridge addresses.
    ///
    /// Bridges are used when direct Tor connections are blocked.
    pub fn configure_tor_bridges(&mut self, bridges: Vec<String>) -> VauchiResult<()> {
        self.config.tor.bridges = bridges;
        self.storage.save_tor_config(&self.config.tor)?;
        Ok(())
    }

    /// Toggles the prefer-onion setting.
    ///
    /// Returns the new prefer_onion state.
    pub fn toggle_prefer_onion(&mut self) -> VauchiResult<bool> {
        self.config.tor.prefer_onion = !self.config.tor.prefer_onion;
        self.storage.save_tor_config(&self.config.tor)?;
        Ok(self.config.tor.prefer_onion)
    }

    /// Clears all Tor bridge addresses.
    ///
    /// Returns the number of bridges that were cleared.
    pub fn clear_tor_bridges(&mut self) -> VauchiResult<usize> {
        let count = self.config.tor.bridges.len();
        self.config.tor.bridges.clear();
        self.storage.save_tor_config(&self.config.tor)?;
        Ok(count)
    }

    /// Requests a new Tor circuit rotation.
    ///
    /// Without the `tor` feature, this is a no-op that returns Ok.
    pub fn request_new_tor_circuit(&self) -> VauchiResult<()> {
        // Actual circuit rotation requires the `tor` feature with arti
        Ok(())
    }

    /// Loads the persisted Tor configuration from storage and applies it.
    pub fn load_tor_config(&mut self) -> VauchiResult<()> {
        if let Some(config) = self.storage.load_tor_config()? {
            self.config.tor = config;
        }
        Ok(())
    }

    // === Multi-Relay Configuration ===

    /// Returns the current multi-relay configuration, if any.
    pub fn relay_list(&self) -> Option<&crate::network::MultiRelayConfig> {
        self.config.relay_list.as_ref()
    }

    /// Sets the multi-relay configuration.
    pub fn set_relay_list(&mut self, config: crate::network::MultiRelayConfig) -> VauchiResult<()> {
        self.config.relay_list = Some(config);
        Ok(())
    }

    /// Clears the multi-relay configuration (reverts to single relay).
    pub fn clear_relay_list(&mut self) {
        self.config.relay_list = None;
    }

    // === Hide/Unhide Contacts ===

    /// Hides a contact from the main contact list.
    ///
    /// Hidden contacts provide plausible deniability - they only appear
    /// via secret access (gesture, PIN, or special settings navigation).
    /// Updates from hidden contacts are still received but notifications
    /// are suppressed.
    pub fn hide_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.hide();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactHidden {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Unhides a contact, making it visible in the main contact list again.
    pub fn unhide_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.unhide();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactUnhidden {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Lists all hidden contacts.
    pub fn list_hidden_contacts(&self) -> VauchiResult<Vec<Contact>> {
        let contacts = self.storage.list_contacts()?;
        Ok(contacts.into_iter().filter(|c| c.is_hidden()).collect())
    }

    // === Block/Unblock Contacts ===

    /// Blocks a contact.
    ///
    /// Blocked contacts will not receive card updates and their incoming
    /// updates will be rejected.
    pub fn block_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.block();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactBlocked {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Unblocks a contact.
    pub fn unblock_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.unblock();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactUnblocked {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Lists all blocked contacts.
    pub fn list_blocked_contacts(&self) -> VauchiResult<Vec<Contact>> {
        let contacts = self.storage.list_contacts()?;
        Ok(contacts.into_iter().filter(|c| c.is_blocked()).collect())
    }

    // === Consent Management ===

    /// Grants consent for a specific type.
    pub fn grant_consent(&self, consent_type: ConsentType) -> VauchiResult<()> {
        let manager = ConsentManager::new(&self.storage);
        manager.grant(consent_type).map_err(VauchiError::from)
    }

    /// Revokes consent for a specific type.
    pub fn revoke_consent(&self, consent_type: ConsentType) -> VauchiResult<()> {
        let manager = ConsentManager::new(&self.storage);
        manager.revoke(consent_type).map_err(VauchiError::from)
    }

    /// Checks whether consent is currently granted for a type.
    pub fn check_consent(&self, consent_type: &ConsentType) -> VauchiResult<bool> {
        let manager = ConsentManager::new(&self.storage);
        manager.check(consent_type).map_err(VauchiError::from)
    }

    /// Exports all consent records.
    pub fn export_consent_log(&self) -> VauchiResult<Vec<ConsentRecord>> {
        let manager = ConsentManager::new(&self.storage);
        manager.export_consent_log().map_err(VauchiError::from)
    }

    /// Returns the aggregated consent status for a specific consent type.
    ///
    /// Combines the boolean grant status with the latest consent record's
    /// timestamp and policy version. This replaces inline status assembly
    /// in TUI and iOS (ADR-021 Tier 1).
    pub fn get_consent_status(
        &self,
        consent_type: ConsentType,
    ) -> VauchiResult<super::super::consent::ConsentStatus> {
        let manager = ConsentManager::new(&self.storage);
        let granted = manager.check(&consent_type).map_err(VauchiError::from)?;

        // Find the latest record for this consent type from the versioned log
        let records = manager
            .export_consent_log_with_version()
            .map_err(VauchiError::from)?;

        let latest = records
            .iter()
            .filter(|r| r.consent_type == consent_type)
            .max_by_key(|r| r.timestamp);

        Ok(super::super::consent::ConsentStatus {
            granted,
            last_changed_at: latest.map(|r| r.timestamp),
            policy_version: latest.and_then(|r| r.policy_version.clone()),
        })
    }

    // === Recovery Readiness ===

    /// Returns the recovery readiness assessment.
    ///
    /// Counts contacts marked as recovery-trusted and compares against the
    /// configured recovery threshold. Replaces inline readiness computation
    /// in CLI contacts.rs and recovery.rs (ADR-021 Tier 1).
    pub fn get_recovery_readiness(&self) -> VauchiResult<RecoveryReadiness> {
        let contacts = self.storage.list_contacts()?;
        let trusted_count = contacts.iter().filter(|c| c.is_recovery_trusted()).count();
        let threshold = self.config.recovery.threshold;

        let is_ready = trusted_count >= threshold as usize;
        let shortfall = (threshold as usize).saturating_sub(trusted_count);

        Ok(RecoveryReadiness {
            trusted_count,
            threshold,
            is_ready,
            shortfall,
        })
    }

    // === Visibility Re-Propagation ===

    /// Sets a field as visible to everyone for a contact, and re-propagates the card.
    pub fn set_field_public_and_repropagate(
        &self,
        contact_id: &str,
        field: &str,
    ) -> VauchiResult<()> {
        let cm = ContactManager::new(&self.storage, self.events.clone());
        cm.set_field_public(contact_id, field)?;
        self.events.dispatch(VauchiEvent::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field: field.to_string(),
        });
        self.repropagate_to_contact(contact_id)
    }

    /// Sets a field as private for a contact, and re-propagates the card.
    pub fn set_field_private_and_repropagate(
        &self,
        contact_id: &str,
        field: &str,
    ) -> VauchiResult<()> {
        let cm = ContactManager::new(&self.storage, self.events.clone());
        cm.set_field_private(contact_id, field)?;
        self.events.dispatch(VauchiEvent::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field: field.to_string(),
        });
        self.repropagate_to_contact(contact_id)
    }

    /// Sets a field as restricted to specific contacts, and re-propagates the card.
    pub fn set_field_restricted_and_repropagate(
        &self,
        contact_id: &str,
        field: &str,
        allowed: Vec<String>,
    ) -> VauchiResult<()> {
        let cm = ContactManager::new(&self.storage, self.events.clone());
        cm.set_field_restricted(contact_id, field, allowed)?;
        self.events.dispatch(VauchiEvent::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field: field.to_string(),
        });
        self.repropagate_to_contact(contact_id)
    }

    /// Adds a contact to a label and re-propagates the card to that contact.
    ///
    /// The contact receives an updated card reflecting their new label membership.
    pub fn add_contact_to_group_and_repropagate(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> VauchiResult<()> {
        self.storage.add_contact_to_group(label_id, contact_id)?;
        self.repropagate_to_contact(contact_id)
    }

    /// Removes a contact from a label and re-propagates the card to that contact.
    ///
    /// The contact receives an updated card with fields they can no longer see removed.
    pub fn remove_contact_from_group_and_repropagate(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> VauchiResult<()> {
        self.storage
            .remove_contact_from_group(label_id, contact_id)?;
        self.repropagate_to_contact(contact_id)
    }

    /// Sets field visibility for a label and re-propagates to all contacts in that label.
    ///
    /// All contacts in the label receive updated cards reflecting the visibility change.
    pub fn set_group_field_visibility_and_repropagate(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        self.storage
            .set_group_field_visibility(label_id, field_id, is_visible)?;

        // Re-propagate to all contacts in this label
        let label = self.storage.load_group(label_id)?;
        for contact_id in label.contacts() {
            self.repropagate_to_contact(contact_id)?;
        }
        Ok(())
    }

    /// Sets a per-contact visibility override and re-propagates to that contact.
    ///
    /// The contact receives an updated card reflecting the override.
    pub fn set_contact_visibility_override_and_repropagate(
        &self,
        contact_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        self.storage
            .save_contact_override(contact_id, field_id, is_visible)?;
        self.repropagate_to_contact(contact_id)
    }

    /// Re-propagates the current card state to a single contact.
    ///
    /// Sends a "full card" delta so the contact receives the card as filtered
    /// by their current visibility rules. Skips if the contact has no ratchet.
    fn repropagate_to_contact(&self, contact_id: &str) -> VauchiResult<()> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let own_card = self
            .storage
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let mut contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;

        // Skip contacts without ratchet (not yet synced)
        let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact_id)? {
            Some(r) => r,
            None => return Ok(()),
        };

        // Compute a "full card" delta from an empty card
        let empty_card = ContactCard::new(own_card.display_name());
        let delta = CardDelta::compute(&empty_card, &own_card);
        if delta.is_empty() {
            return Ok(());
        }

        // Filter delta using effective visibility (labels + overrides + defaults)
        let contact_id_owned = contact_id.to_string();
        let mut delta = delta.filter_with(|field_id| {
            self.get_effective_field_visibility(&contact_id_owned, field_id)
                .unwrap_or(false)
        });
        if delta.is_empty() {
            return Ok(());
        }

        // Sign delta with our identity, bound to recipient
        delta.sign(identity, contact.public_key());

        // Serialize delta
        let delta_bytes =
            serde_json::to_vec(&delta).map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Wrap with CEK if contact has one, otherwise legacy
        let payload_bytes = if contact.cek().is_some() {
            let new_cek = ContentEncryptionKey::generate();
            let cek_ciphertext = new_cek
                .encrypt(&delta_bytes)
                .map_err(|e| VauchiError::Crypto(format!("CEK encrypt: {:?}", e)))?;

            let wrapped = CekWrappedPayload {
                cek: new_cek.to_bytes(),
                cek_ciphertext,
                signature: delta.signature,
                nonce: delta.nonce,
            };

            contact.set_cek(new_cek);
            self.storage.save_contact(&contact)?;
            VersionedPayload::encode_cek(&wrapped)
        } else {
            delta_bytes
        };

        // Encrypt with ratchet
        let ratchet_msg = ratchet
            .encrypt(&payload_bytes)
            .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
        let encrypted = serde_json::to_vec(&ratchet_msg)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Save updated ratchet state
        self.storage
            .save_ratchet_state(contact_id, &ratchet, is_initiator)?;

        // Queue for delivery
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let update = PendingUpdate {
            id: uuid::Uuid::new_v4().to_string(),
            contact_id: contact_id.to_string(),
            update_type: "card_delta".to_string(),
            payload: encrypted,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: None,
        };
        self.storage.queue_update(&update)?;

        Ok(())
    }
}
