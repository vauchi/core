// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Card propagation, CEK migration, device lookup, content updates, and sync item application.

use crate::contact_card::{ContactCard, ContactField};

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    // === Card Propagation Operations ===

    /// Propagates own card update to all contacts.
    ///
    /// For each contact with an established ratchet:
    /// 1. Computes delta between old and new card
    /// 2. Signs delta with our identity
    /// 3. If contact has CEK: wraps in `CekWrappedPayload` (version 0x02), rotates CEK
    /// 4. If contact has no CEK: uses legacy format (raw JSON bytes)
    /// 5. Encrypts with contact's ratchet
    /// 6. Queues for delivery via relay
    ///
    /// Returns the number of contacts queued for update.
    pub fn propagate_card_update(
        &self,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> VauchiResult<usize> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let contacts = self.storage.list_contacts()?;
        let mut queued = 0;

        for mut contact in contacts {
            // Skip blocked contacts
            if contact.is_blocked() {
                continue;
            }

            // Skip contacts without ratchet (not yet synced)
            let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact.id())? {
                Some(r) => r,
                None => continue,
            };

            // Compute delta
            let delta = CardDelta::compute(old_card, new_card);
            if delta.is_empty() {
                continue;
            }

            // Filter delta based on visibility rules for this contact
            let mut delta = delta.filter_for_contact(contact.id(), contact.visibility_rules());
            if delta.is_empty() {
                continue;
            }

            // Sign delta with our identity, bound to recipient
            delta.sign(identity, contact.public_key());

            // Serialize delta
            let delta_bytes = serde_json::to_vec(&delta)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Wrap with CEK if contact has one (version 0x02), otherwise legacy
            let payload_bytes = if contact.cek().is_some() {
                // Rotate CEK
                let new_cek = ContentEncryptionKey::generate();

                // Encrypt delta with new CEK
                let cek_ciphertext = new_cek
                    .encrypt(&delta_bytes)
                    .map_err(|e| VauchiError::Crypto(format!("CEK encrypt: {:?}", e)))?;

                // Build wrapped payload
                let wrapped = CekWrappedPayload {
                    cek: new_cek.to_bytes(),
                    cek_ciphertext,
                    signature: delta.signature,
                    nonce: delta.nonce,
                };

                // Update contact with rotated CEK and re-save
                // (re-encrypts card at rest with new CEK)
                contact.set_cek(new_cek);
                self.storage.save_contact(&contact)?;

                // Version-tagged encoding
                VersionedPayload::encode_cek(&wrapped)
            } else {
                // Legacy format: raw delta JSON bytes
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
                .save_ratchet_state(contact.id(), &ratchet, is_initiator)?;

            // Queue for delivery
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let update = PendingUpdate {
                id: uuid::Uuid::new_v4().to_string(),
                contact_id: contact.id().to_string(),
                update_type: "card_delta".to_string(),
                payload: encrypted,
                created_at: now,
                retry_count: 0,
                status: UpdateStatus::Pending,
                target_relay_url: contact.relay_url().map(String::from),
            };
            self.storage.queue_update(&update)?;
            queued += 1;
        }

        Ok(queued)
    }

    /// Processes an encrypted card update from a contact.
    ///
    /// 1. Checks revoked_senders tombstone — rejects updates from revoked senders
    /// 2. Decrypts the update using the contact's ratchet
    /// 3. Detects payload version:
    ///    - Version 0x02 (CEK-wrapped): extracts CEK, decrypts delta, saves CEK
    ///    - Version 0x01 or raw JSON (legacy): parses delta directly
    /// 4. Verifies the signature using the contact's public key
    /// 5. Applies the delta to the contact's card
    ///
    /// Returns a list of changed field labels.
    pub fn process_card_update(
        &self,
        sender_id: &str,
        encrypted: &[u8],
    ) -> VauchiResult<Vec<String>> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::crypto::ratchet::RatchetMessage;
        use crate::network::anonymous::resolve_sender_id;
        use crate::sync::delta::{CardDelta, PAYLOAD_VERSION_CEK, VersionedPayload};

        // Resolve anonymous sender ID to real contact ID.
        // Old-format messages with real identity fingerprints pass through
        // unchanged via the fallback path in resolve_sender_id.
        let contacts = self.storage.list_contacts().unwrap_or_default();
        let resolved =
            resolve_sender_id(&contacts, sender_id).unwrap_or_else(|| sender_id.to_string());
        let contact_id = resolved.as_str();

        // Check revoked_senders tombstone
        if self.storage.is_sender_revoked(contact_id)? {
            return Err(VauchiError::InvalidState(
                "update from revoked sender".to_string(),
            ));
        }

        // Reject updates from blocked contacts
        if let Some(contact) = self.storage.load_contact(contact_id)?
            && contact.is_blocked()
        {
            return Err(VauchiError::ContactBlocked(contact_id.to_string()));
        }

        // Load contact
        let mut contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", contact_id)))?;

        // Load and decrypt with ratchet
        let (mut ratchet, is_initiator) = self
            .storage
            .load_ratchet_state(contact_id)?
            .ok_or_else(|| VauchiError::NotFound("ratchet state".into()))?;

        let ratchet_msg: RatchetMessage = serde_json::from_slice(encrypted)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;
        let plaintext = ratchet
            .decrypt(&ratchet_msg)
            .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;

        // Detect payload version and extract delta bytes + optional CEK
        let (delta_bytes, new_cek) = if !plaintext.is_empty() && plaintext[0] == PAYLOAD_VERSION_CEK
        {
            // Version 0x02: CEK-wrapped payload
            match VersionedPayload::decode(&plaintext) {
                Ok(VersionedPayload::CekWrapped(wrapped)) => {
                    let cek = ContentEncryptionKey::from_bytes(wrapped.cek);
                    let decrypted = cek
                        .decrypt(&wrapped.cek_ciphertext)
                        .map_err(|e| VauchiError::Crypto(format!("CEK decrypt: {:?}", e)))?;
                    (decrypted, Some(cek))
                }
                Ok(VersionedPayload::Legacy(data)) => (data, None),
                Err(e) => {
                    return Err(VauchiError::Serialization(format!("payload decode: {}", e)));
                }
            }
        } else {
            // Legacy: raw JSON bytes (no version tag, or version 0x01)
            match VersionedPayload::decode(&plaintext) {
                Ok(VersionedPayload::Legacy(data)) => (data, None),
                _ => {
                    // Fall back to treating entire plaintext as legacy delta JSON
                    (plaintext, None)
                }
            }
        };

        // Parse delta
        let delta: CardDelta = serde_json::from_slice(&delta_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Verify signature with contact's (sender) and our (recipient) public keys
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        if !delta.verify(contact.public_key(), identity.signing_public_key()) {
            return Err(VauchiError::SignatureInvalid);
        }

        // Check for replay attack
        {
            let mut detector = self
                .replay_detector
                .lock()
                .map_err(|_| VauchiError::InvalidState("replay detector poisoned".into()))?;
            if !detector.check_replay(contact_id, &delta.nonce, delta.timestamp) {
                return Err(VauchiError::ReplayDetected);
            }
        }

        // Reject stale/downgraded delta versions (#42)
        let last_version = self.storage.last_delta_version(contact_id).unwrap_or(0);
        if delta.version > 0 && delta.version < last_version {
            return Err(VauchiError::InvalidState(format!(
                "stale delta version {} (last applied: {})",
                delta.version, last_version
            )));
        }

        // Get changed fields before applying
        let changed = delta.changed_fields();

        // Apply delta to contact's card
        let mut new_card = contact.card().clone();
        delta
            .apply(&mut new_card)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        // Update contact card and CEK atomically
        contact.update_card(new_card);
        if let Some(cek) = new_cek {
            contact.set_cek(cek);
        }

        // All DB writes in a single transaction: ratchet state, replay nonce, contact card.
        // If any write fails, all are rolled back to prevent inconsistent state.
        self.storage.begin_transaction()?;
        let result = (|| -> VauchiResult<()> {
            self.storage
                .save_ratchet_state(contact_id, &ratchet, is_initiator)?;
            self.storage
                .save_replay_nonce(contact_id, &delta.nonce, delta.timestamp)?;
            self.storage.save_contact(&contact)?;
            // Track delta version for downgrade detection (#42)
            if delta.version > 0 {
                self.storage
                    .record_delta_version(contact_id, delta.version)?;
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.storage.commit()?;
                Ok(changed)
            }
            Err(e) => {
                self.storage.rollback();
                Err(e)
            }
        }
    }

    // === CEK Migration ===

    /// Migrates legacy contacts to CEK-protected format.
    ///
    /// For each contact that has an established ratchet but no CEK:
    /// 1. Generates a new CEK
    /// 2. Saves the CEK locally
    /// 3. Queues a migration update (empty delta carrying the CEK) for relay delivery
    ///
    /// Returns the number of contacts migrated.
    pub fn migrate_contacts_to_cek(&self) -> VauchiResult<usize> {
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

        let contacts = self.storage.list_contacts()?;
        let mut migrated = 0;

        for mut contact in contacts {
            // Skip contacts that already have a CEK
            if contact.cek().is_some() {
                continue;
            }

            // Skip contacts without ratchet (can't send updates)
            let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact.id())? {
                Some(r) => r,
                None => continue,
            };

            // Generate a new CEK for this contact
            let cek = ContentEncryptionKey::generate();

            // Create a no-op delta (empty changes — just carries the CEK)
            let mut delta = CardDelta::compute(&own_card, &own_card);
            // Force a nonce so the delta is processable even with no changes
            delta.sign(identity, contact.public_key());

            // Serialize and CEK-encrypt the delta
            let delta_bytes = serde_json::to_vec(&delta)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;
            let cek_ciphertext = cek
                .encrypt(&delta_bytes)
                .map_err(|e| VauchiError::Crypto(format!("CEK encrypt: {:?}", e)))?;

            let wrapped = CekWrappedPayload {
                cek: cek.to_bytes(),
                cek_ciphertext,
                signature: delta.signature,
                nonce: delta.nonce,
            };
            let payload_bytes = VersionedPayload::encode_cek(&wrapped);

            // Ratchet-encrypt
            let ratchet_msg = ratchet
                .encrypt(&payload_bytes)
                .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Save updated ratchet state
            self.storage
                .save_ratchet_state(contact.id(), &ratchet, is_initiator)?;

            // Set CEK on contact and re-save (re-encrypts card at rest with CEK)
            contact.set_cek(cek);
            self.storage.save_contact(&contact)?;

            // Queue for delivery
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let update = PendingUpdate {
                id: uuid::Uuid::new_v4().to_string(),
                contact_id: contact.id().to_string(),
                update_type: "cek_migration".to_string(),
                payload: encrypted,
                created_at: now,
                retry_count: 0,
                status: UpdateStatus::Pending,
                target_relay_url: contact.relay_url().map(String::from),
            };
            self.storage.queue_update(&update)?;
            migrated += 1;
        }

        Ok(migrated)
    }

    // === Device Lookup Operations ===

    /// Finds an active device by hex ID prefix.
    ///
    /// Loads the device registry from storage and searches active devices
    /// whose hex-encoded device ID starts with the given prefix.
    /// Returns `None` if no registry exists or no device matches.
    pub fn find_device_by_prefix(
        &self,
        hex_prefix: &str,
    ) -> VauchiResult<Option<crate::identity::RegisteredDevice>> {
        let registry = self.storage.load_device_registry()?;
        match registry {
            Some(reg) => Ok(reg.find_device_by_prefix(hex_prefix).cloned()),
            None => Ok(None),
        }
    }

    // === Content Update Operations ===

    /// Checks if content updates are available.
    ///
    /// Compares stored/cached manifest versions against available versions.
    /// Returns the status of available updates without applying them.
    ///
    /// This is the synchronous version intended for CLI, desktop, and TUI.
    /// For async mobile usage, see `vauchi-platform`'s `MobileContentUpdater`.
    pub fn check_content_updates(&self) -> crate::content::UpdateStatus {
        let content_config = crate::content::ContentConfig {
            storage_path: self.config.storage_path.join("content_cache"),
            ..Default::default()
        };

        match crate::content::ContentManager::new(content_config) {
            Ok(manager) => manager.check_for_updates_sync(),
            Err(_) => crate::content::UpdateStatus::CheckFailed(
                "Failed to initialize content manager".to_string(),
            ),
        }
    }

    // === Device Sync Item Application ===

    /// Applies a list of sync items received from another device.
    ///
    /// Processes each item sequentially, applying the corresponding
    /// storage mutation (add/remove contact, update card, change
    /// visibility, manage labels, update trust, schedule deletion).
    ///
    /// Returns the number of items successfully applied. Items that
    /// fail are skipped (logged but non-fatal) so partial application
    /// is possible.
    pub fn apply_sync_items(
        &self,
        items: Vec<crate::sync::device_sync::SyncItem>,
    ) -> VauchiResult<usize> {
        use crate::sync::device_sync::SyncItem;

        let mut applied = 0;

        for item in items {
            let result = match item {
                SyncItem::ContactAdded { contact_data, .. } => match contact_data.to_contact() {
                    Ok(contact) => self.storage.save_contact(&contact).map_err(|e| e.into()),
                    Err(e) => Err(VauchiError::InvalidState(e.to_string())),
                },
                SyncItem::ContactRemoved { ref contact_id, .. } => self
                    .storage
                    .delete_contact(contact_id)
                    .map(|_| ())
                    .map_err(|e| e.into()),
                SyncItem::CardUpdated {
                    ref field_label,
                    ref new_value,
                    ..
                } => {
                    // Load own card, update the field by label, save
                    match self.storage.load_own_card()? {
                        Some(mut card) => {
                            // Find field by label and update its value
                            let field_id = card
                                .fields()
                                .iter()
                                .find(|f| f.label() == field_label)
                                .map(|f| f.id().to_string());

                            if let Some(id) = field_id {
                                let _ = card.update_field_value(&id, new_value);
                            } else {
                                // Field not found — add as new
                                let field = ContactField::new(
                                    crate::contact_card::FieldType::Custom,
                                    field_label,
                                    new_value,
                                );
                                let _ = card.add_field(field);
                            }
                            self.storage.save_own_card(&card).map_err(|e| e.into())
                        }
                        None => Err(VauchiError::IdentityNotInitialized),
                    }
                }
                SyncItem::VisibilityChanged {
                    ref contact_id,
                    ref field_label,
                    is_visible,
                    ..
                } => self
                    .storage
                    .save_contact_override(contact_id, field_label, is_visible)
                    .map_err(|e| e.into()),
                SyncItem::LabelChange {
                    ref label_id,
                    ref label_name,
                    ref contacts,
                    ref visible_fields,
                    is_deleted,
                    ..
                } => {
                    if is_deleted {
                        self.storage.delete_group(label_id).map_err(|e| e.into())
                    } else {
                        // Create or update label
                        match self.storage.load_group(label_id) {
                            Ok(_existing) => {
                                // Update existing: rename, re-assign contacts and fields
                                let _ = self.storage.rename_group(label_id, label_name);
                                // Re-apply contacts (simplified: just ensure they're assigned)
                                for cid in contacts {
                                    let _ = self.storage.add_contact_to_group(label_id, cid);
                                }
                                // Re-apply field visibility
                                for fid in visible_fields {
                                    let _ = self
                                        .storage
                                        .set_group_field_visibility(label_id, fid, true);
                                }
                                Ok(())
                            }
                            Err(_) => {
                                // Create new label
                                let _ = self.storage.create_group(label_name);
                                Ok(())
                            }
                        }
                    }
                }
                SyncItem::ContactTrustChanged {
                    ref contact_id,
                    recovery_trusted,
                    ..
                } => {
                    match self.storage.load_contact(contact_id)? {
                        Some(mut contact) => {
                            contact.set_recovery_trusted(recovery_trusted);
                            self.storage.save_contact(&contact).map_err(|e| e.into())
                        }
                        None => Ok(()), // Contact not found, skip
                    }
                }
                SyncItem::DeletionScheduled {
                    scheduled_at,
                    execute_at,
                    ..
                } => {
                    let state = crate::storage::DeletionState::Scheduled {
                        scheduled_at,
                        execute_at,
                    };
                    self.storage
                        .save_deletion_state(&state)
                        .map_err(|e| e.into())
                }
                SyncItem::DeletionCancelled { .. } => self
                    .storage
                    .save_deletion_state(&crate::storage::DeletionState::None)
                    .map_err(|e| e.into()),
            };

            if result.is_ok() {
                applied += 1;
            }
        }

        Ok(applied)
    }
}
