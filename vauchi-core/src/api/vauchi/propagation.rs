// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Card propagation, CEK migration, device lookup, content updates, and sync item application.

use crate::contact_card::{ContactCard, ContactField};

use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::Vauchi;

impl Vauchi {
    // === Card Propagation Operations ===

    /// Propagates own card update to all contacts.
    ///
    /// Delegates to `prepare_card_update_for_contact()` for each eligible
    /// contact, then queues the encrypted result for relay delivery.
    /// Single crypto path — no duplication.
    ///
    /// Returns the number of contacts queued for update.
    pub fn propagate_card_update(
        &self,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> VauchiResult<usize> {
        use crate::storage::{PendingUpdate, UpdateStatus};

        let contacts = self.storage.contacts().list_contacts()?;
        let mut queued = 0;

        for contact in contacts {
            let encrypted =
                match self.prepare_card_update_for_contact(contact.id(), old_card, new_card) {
                    Ok(data) => data,
                    // Expected skips: blocked, no ratchet, empty delta, not exchanged
                    Err(VauchiError::ContactBlocked(_))
                    | Err(VauchiError::NotFound(_))
                    | Err(VauchiError::InvalidState(_)) => continue,
                    Err(e) => return Err(e),
                };

            let now = self.clock.unix_seconds();

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
            self.storage.pending().queue_update(&update)?;
            queued += 1;
        }

        Ok(queued)
    }

    /// Queues an initial card update for a newly exchanged contact.
    ///
    /// After a contact exchange (QR or relay), the initiator must send the
    /// first ratchet message to establish the responder's receive chain.
    /// This method encrypts the full own card as a delta (empty → current)
    /// and queues it for delivery on the next `sync()` call.
    pub fn queue_initial_card_for_contact(&self, contact_id: &str) -> VauchiResult<()> {
        use crate::contact_card::ContactCard;
        use crate::storage::{PendingUpdate, UpdateStatus};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let our_card = self
            .storage
            .contacts()
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(identity.display_name()));
        let empty_card = ContactCard::new(identity.display_name());

        let encrypted = self.prepare_card_update_for_contact(contact_id, &empty_card, &our_card)?;

        // Load contact to get relay_url for per-contact relay routing
        let relay_url = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .and_then(|c| c.relay_url().map(String::from));

        let now = self.clock.unix_seconds();

        let update = PendingUpdate {
            id: uuid::Uuid::new_v4().to_string(),
            contact_id: contact_id.to_string(),
            update_type: "card_delta".to_string(),
            payload: encrypted,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
            target_relay_url: relay_url,
        };
        self.storage.pending().queue_update(&update)?;

        Ok(())
    }

    /// Prepares an encrypted card update for a single contact.
    ///
    /// Single crypto path for card propagation (ADR-021). Handles:
    /// delta computation, version tracking, signing, CEK wrapping,
    /// ratchet encryption, and atomic state persistence.
    ///
    /// Used directly by CLI for relay transport, and indirectly by
    /// `propagate_card_update()` for batch queuing.
    ///
    /// Returns the encrypted ciphertext ready for relay delivery.
    /// Returns `Err` if the delta is empty (no changes to send).
    pub fn prepare_card_update_for_contact(
        &self,
        contact_id: &str,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> VauchiResult<Vec<u8>> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

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

        // Compute delta
        let delta = CardDelta::compute(old_card, new_card, self.clock.unix_seconds());
        if delta.is_empty() {
            return Err(VauchiError::InvalidState("empty delta".into()));
        }

        // Only exchanged contacts have public keys for signing
        let ex = contact
            .kind()
            .exchanged_data()
            .ok_or_else(|| VauchiError::InvalidState("contact not exchanged".into()))?;

        // Filter delta based on visibility rules
        let mut delta = delta.filter_for_contact(contact_id, &ex.visibility_rules);
        if delta.is_empty() {
            return Err(VauchiError::InvalidState(
                "empty delta after visibility filter".into(),
            ));
        }

        // Version tracking for downgrade detection (#42)
        let next_version = self
            .storage
            .contacts()
            .last_sent_delta_version(contact_id)
            .unwrap_or(0)
            + 1;
        delta.set_version(next_version);

        // Sign delta with our identity, bound to recipient
        let public_key = ex.public_key;
        delta.sign(identity, &public_key);

        // Serialize delta
        let delta_bytes =
            serde_json::to_vec(&delta).map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Always use CEK format (version 0x02) — process_card_update rejects
        // legacy payloads, so contacts without CEK need one generated.
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
        let payload_bytes = VersionedPayload::encode_cek(&wrapped);

        // Load ratchet and encrypt
        let (mut ratchet, is_initiator) =
            self.storage
                .ratchets()
                .load_ratchet_state(contact_id)?
                .ok_or_else(|| VauchiError::NotFound("ratchet state".into()))?;

        let ratchet_msg = ratchet
            .encrypt(&payload_bytes)
            .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
        let encrypted = serde_json::to_vec(&ratchet_msg)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Save CEK, ratchet state, and sent version atomically
        self.storage.begin_transaction()?;
        let save_result = (|| -> VauchiResult<()> {
            contact.set_cek(new_cek);
            self.storage.contacts().save_contact(&contact)?;
            self.storage
                .ratchets()
                .save_ratchet_state(contact_id, &ratchet, is_initiator)?;
            self.storage
                .contacts()
                .record_sent_delta_version(contact_id, next_version)?;
            Ok(())
        })();
        match save_result {
            Ok(()) => self.storage.commit()?,
            Err(e) => {
                self.storage.rollback();
                return Err(e);
            }
        }

        Ok(encrypted)
    }

    /// Processes an encrypted card update from a contact.
    ///
    /// 1. Checks revoked_senders tombstone — rejects updates from revoked senders
    /// 2. Decrypts the update using the contact's ratchet
    /// 3. Detects payload version:
    ///    - Version 0x02 (CEK-wrapped): extracts CEK, decrypts delta, saves CEK
    ///    - Other versions: rejected (legacy raw JSON is no longer supported)
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
        let contacts = self.storage.contacts().list_contacts().unwrap_or_default();
        let resolved = resolve_sender_id(&contacts, sender_id, self.clock.unix_seconds())
            .unwrap_or_else(|| sender_id.to_string());
        let contact_id = resolved.as_str();

        // Check revoked_senders tombstone
        if self.storage.contacts().is_sender_revoked(contact_id)? {
            return Err(VauchiError::InvalidState(
                "update from revoked sender".to_string(),
            ));
        }

        // Reject updates from blocked contacts
        if let Some(contact) = self.storage.contacts().load_contact(contact_id)?
            && contact.is_blocked()
        {
            return Err(VauchiError::ContactBlocked(contact_id.to_string()));
        }

        // Load contact
        let mut contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", contact_id)))?;

        // Load and decrypt with ratchet
        let (mut ratchet, is_initiator) =
            self.storage
                .ratchets()
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
                Ok(VersionedPayload::ReciprocityConfirm(_confirm)) => {
                    // TODO(reciprocity): handle ReciprocityConfirm in receive path
                    // For now, skip — the confirmer will handle this once wired.
                    return Ok(Vec::new());
                }
                Err(e) => {
                    return Err(VauchiError::Serialization(format!("payload decode: {}", e)));
                }
            }
        } else {
            return Err(VauchiError::Serialization("unknown payload version".into()));
        };

        // Parse delta
        let delta: CardDelta = serde_json::from_slice(&delta_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Verify signature with contact's (sender) and our (recipient) public keys
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let sender_pk = contact.public_key().ok_or(VauchiError::InvalidState(
            "Contact has no public key".into(),
        ))?;
        if !delta.verify(sender_pk, identity.signing_public_key()) {
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
        let last_version = self
            .storage
            .contacts()
            .last_delta_version(contact_id)
            .unwrap_or(0);
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
            .apply(&mut new_card, self.clock.unix_seconds())
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        // Update contact card and CEK atomically
        contact.update_card(new_card, 0);
        if let Some(cek) = new_cek {
            contact.set_cek(cek);
        }

        // All DB writes in a single transaction: ratchet state, replay nonce, contact card.
        // If any write fails, all are rolled back to prevent inconsistent state.
        self.storage.begin_transaction()?;
        let result = (|| -> VauchiResult<()> {
            self.storage
                .ratchets()
                .save_ratchet_state(contact_id, &ratchet, is_initiator)?;
            self.storage
                .replay()
                .save_replay_nonce(contact_id, &delta.nonce, delta.timestamp)?;
            self.storage.contacts().save_contact(&contact)?;
            // Track delta version for downgrade detection (#42)
            if delta.version > 0 {
                self.storage
                    .contacts()
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
            .contacts()
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let contacts = self.storage.contacts().list_contacts()?;
        let mut migrated = 0;

        for mut contact in contacts {
            // Skip contacts that already have a CEK
            if contact.cek().is_some() {
                continue;
            }

            // Skip contacts without ratchet (can't send updates)
            let (mut ratchet, is_initiator) =
                match self.storage.ratchets().load_ratchet_state(contact.id())? {
                    Some(r) => r,
                    None => continue,
                };

            // Generate a new CEK for this contact
            let cek = ContentEncryptionKey::generate();

            // Create a no-op delta (empty changes — just carries the CEK)
            let mut delta = CardDelta::compute(&own_card, &own_card, self.clock.unix_seconds());
            // Force a nonce so the delta is processable even with no changes
            let Some(recipient_pk) = contact.public_key() else {
                continue; // Skip imported contacts
            };
            delta.sign(identity, recipient_pk);

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
                .ratchets()
                .save_ratchet_state(contact.id(), &ratchet, is_initiator)?;

            // Set CEK on contact and re-save (re-encrypts card at rest with CEK)
            contact.set_cek(cek);
            self.storage.contacts().save_contact(&contact)?;

            // Queue for delivery
            let now = self.clock.unix_seconds();

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
            self.storage.pending().queue_update(&update)?;
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
        let registry = self.storage.device().load_device_registry()?;
        match registry {
            Some(reg) => Ok(reg.find_device_by_prefix(hex_prefix).cloned()),
            None => Ok(None),
        }
    }

    // Content update operations moved to vauchi-app.

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
                    Ok(contact) => {
                        let contact_id = contact.id().to_string();
                        let result = self
                            .storage
                            .contacts()
                            .save_contact(&contact)
                            .map_err(|e| e.into());
                        if result.is_ok() {
                            self.events.dispatch(VauchiEvent::ContactAdded {
                                contact_id,
                                origin: crate::api::events::EventOrigin::Synced,
                            });
                        }
                        result
                    }
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
                    match self.storage.contacts().load_own_card()? {
                        Some(mut card) => {
                            // Find field by label and update its value
                            let field_id = card
                                .fields()
                                .iter()
                                .find(|f| f.label() == field_label)
                                .map(|f| f.id().to_string());

                            if let Some(id) = field_id {
                                card.update_field_value(&id, new_value, self.clock.unix_seconds())
                                    .map_err(VauchiError::from)?;
                            } else {
                                // Field not found — add as new
                                let field = ContactField::new(
                                    crate::contact_card::FieldType::Custom,
                                    field_label,
                                    new_value,
                                    self.clock.unix_seconds(),
                                );
                                card.add_field(field).map_err(VauchiError::from)?;
                            }
                            self.storage
                                .contacts()
                                .save_own_card(&card)
                                .map_err(|e| e.into())
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
                    .labels()
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
                        self.storage
                            .labels()
                            .delete_group(label_id)
                            .map_err(|e| e.into())
                    } else {
                        // Create or update label
                        match self.storage.labels().load_group(label_id) {
                            Ok(_existing) => {
                                // Update existing: rename, re-assign contacts and
                                // fields. Each call propagates so divergent state
                                // surfaces instead of being silently dropped.
                                self.storage.labels().rename_group(label_id, label_name)?;
                                for cid in contacts {
                                    self.storage.labels().add_contact_to_group(label_id, cid)?;
                                }
                                for fid in visible_fields {
                                    self.storage
                                        .labels()
                                        .set_group_field_visibility(label_id, fid, true)?;
                                }
                                Ok(())
                            }
                            Err(_) => {
                                // Create new label
                                self.storage
                                    .labels()
                                    .create_group(label_name)
                                    .map(|_| ())
                                    .map_err(|e| e.into())
                            }
                        }
                    }
                }
                SyncItem::ContactTrustChanged {
                    ref contact_id,
                    recovery_trusted,
                    ..
                } => {
                    match self.storage.contacts().load_contact(contact_id)? {
                        Some(mut contact) => {
                            contact
                                .set_recovery_trusted(recovery_trusted)
                                .map_err(VauchiError::from)?;
                            self.storage
                                .contacts()
                                .save_contact(&contact)
                                .map_err(|e| e.into())
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
                        .consent()
                        .save_deletion_state(&state)
                        .map_err(|e| e.into())
                }
                SyncItem::DeletionCancelled { .. } => self
                    .storage
                    .consent()
                    .save_deletion_state(&crate::storage::DeletionState::None)
                    .map_err(|e| e.into()),
                SyncItem::PersonalNoteChanged {
                    ref contact_id,
                    ref note,
                    ..
                } => self
                    .storage
                    .contacts()
                    .save_personal_notes(contact_id, note.as_bytes())
                    .map_err(|e| e.into()),
                SyncItem::ContactFieldNoteChanged {
                    ref contact_id,
                    ref field_id,
                    ref note,
                    ..
                } => self
                    .storage
                    .field_notes()
                    .save_contact_field_note(contact_id, field_id, note.as_bytes())
                    .map_err(|e| e.into()),
                SyncItem::ProposalTrustChanged {
                    ref contact_id,
                    proposal_trusted,
                    ..
                } => match self.storage.contacts().load_contact(contact_id)? {
                    Some(mut contact) => {
                        contact
                            .set_proposal_trusted(proposal_trusted)
                            .map_err(VauchiError::from)?;
                        self.storage
                            .contacts()
                            .save_contact(&contact)
                            .map_err(|e| e.into())
                    }
                    None => Ok(()), // Contact not found, skip
                },
                SyncItem::ImportedContactAdded {
                    ref contact_data, ..
                } => match contact_data.to_contact() {
                    Ok(contact) => self
                        .storage
                        .contacts()
                        .save_contact(&contact)
                        .map_err(|e| e.into()),
                    Err(e) => Err(e.into()),
                },
                SyncItem::ImportedContactUpdated {
                    ref contact_data, ..
                } => match contact_data.to_contact() {
                    Ok(contact) => self
                        .storage
                        .contacts()
                        .save_contact(&contact)
                        .map_err(|e| e.into()),
                    Err(e) => Err(e.into()),
                },
                SyncItem::ImportedContactRemoved { ref contact_id, .. } => self
                    .storage
                    .delete_contact(contact_id)
                    .map(|_| ())
                    .map_err(|e| e.into()),
                SyncItem::ContactArchived {
                    ref contact_id,
                    timestamp,
                    ..
                } => match self.storage.contacts().load_contact(contact_id)? {
                    Some(mut contact) => {
                        contact.archive(timestamp);
                        self.storage
                            .contacts()
                            .save_contact(&contact)
                            .map_err(|e| e.into())
                    }
                    None => Ok(()), // Contact not found, skip
                },
                SyncItem::ContactUnarchived { ref contact_id, .. } => {
                    match self.storage.contacts().load_contact(contact_id)? {
                        Some(mut contact) => {
                            contact.unarchive();
                            self.storage
                                .contacts()
                                .save_contact(&contact)
                                .map_err(|e| e.into())
                        }
                        None => Ok(()), // Contact not found, skip
                    }
                }
            };

            if result.is_ok() {
                applied += 1;
            }
        }

        Ok(applied)
    }
}
