// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Card propagation, CEK migration, device lookup, content updates, and sync item application.

use crate::contact_card::{ContactCard, ContactField};
use crate::rng::SecureRngExt;

use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::Vauchi;

type PreparedDevicePayload = ([u8; 32], Vec<u8>, crate::crypto::DoubleRatchetState, bool);

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
            let queue_result = self.storage.with_savepoint(|| -> VauchiResult<()> {
                let encrypted =
                    self.prepare_card_updates_for_contact(contact.id(), old_card, new_card)?;
                let now = self.clock.unix_seconds();
                for (_, payload) in encrypted {
                    let update = PendingUpdate {
                        id: self.rng.uuid_v4(),
                        contact_id: contact.id().to_string(),
                        update_type: "card_delta".to_string(),
                        payload,
                        created_at: now,
                        retry_count: 0,
                        status: UpdateStatus::Pending,
                        target_relay_url: contact.relay_url().map(String::from),
                    };
                    self.storage.pending().queue_update(&update)?;
                }
                Ok(())
            });
            match queue_result {
                Ok(()) => queued += 1,
                // Expected skips: blocked, no ratchet, empty delta, not exchanged
                Err(VauchiError::ContactBlocked(_))
                | Err(VauchiError::NotFound(_))
                | Err(VauchiError::InvalidState(_)) => continue,
                Err(error) => return Err(error),
            }
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

        self.storage.with_savepoint(|| -> VauchiResult<()> {
            let encrypted =
                self.prepare_card_updates_for_contact(contact_id, &empty_card, &our_card)?;
            let relay_url = self
                .storage
                .contacts()
                .load_contact(contact_id)?
                .and_then(|contact| contact.relay_url().map(String::from));
            let now = self.clock.unix_seconds();
            for (_, payload) in encrypted {
                let update = PendingUpdate {
                    id: self.rng.uuid_v4(),
                    contact_id: contact_id.to_string(),
                    update_type: "card_delta".to_string(),
                    payload,
                    created_at: now,
                    retry_count: 0,
                    status: UpdateStatus::Pending,
                    target_relay_url: relay_url.clone(),
                };
                self.storage.pending().queue_update(&update)?;
            }
            Ok(())
        })
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
        if self
            .storage
            .device()
            .load_contact_active_devices(contact_id)?
            .len()
            > 1
        {
            return Err(VauchiError::InvalidState(
                "contact has multiple active devices; use fan-out preparation".into(),
            ));
        }
        self.prepare_card_updates_for_contact(contact_id, old_card, new_card)?
            .into_iter()
            .next()
            .map(|(_, payload)| payload)
            .ok_or_else(|| VauchiError::NotFound("active peer device".into()))
    }

    /// Prepares one independently-ratcheted copy for every active peer device.
    pub fn prepare_card_updates_for_contact(
        &self,
        contact_id: &str,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> VauchiResult<Vec<([u8; 32], Vec<u8>)>> {
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
        // Archived contacts receive nothing until unarchived (which queues a
        // catch-up) — enforced here so direct per-contact callers cannot
        // bypass the `list_contacts` archived filter (Decision 3,
        // 2026-07-05-ungrouped-contacts-default-open).
        if contact.is_archived() {
            return Err(VauchiError::InvalidState(format!(
                "contact archived: {contact_id}"
            )));
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

        // Filter to the fields this contact may currently see, via the
        // group-aware resolver (ADR-054 D3) — the same chokepoint
        // repropagate_to_contact uses. Replaces the Layer-A-only
        // filter_for_contact, which ignored group membership and leaked
        // ungranted fields to grouped contacts
        // (2026-06-08-sync-card-update-not-group-filtered, G4). unwrap_or(false)
        // fails closed on storage error (privacy > completeness).
        let mut delta = delta.filter_with(|fid| {
            self.get_effective_field_visibility(contact_id, fid)
                .unwrap_or(false)
        });
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

        let prepared =
            self.encrypt_payload_for_contact_devices(identity, &contact, &payload_bytes)?;

        self.storage.with_savepoint(|| -> VauchiResult<()> {
            contact.set_cek(new_cek);
            self.storage.contacts().save_contact(&contact)?;
            for (peer_device_id, _, ratchet, is_initiator) in &prepared {
                self.storage.ratchets().save_ratchet_state_for_device(
                    contact_id,
                    peer_device_id,
                    ratchet,
                    *is_initiator,
                )?;
            }
            self.storage
                .contacts()
                .record_sent_delta_version(contact_id, next_version)?;
            Ok(())
        })?;

        Ok(prepared
            .into_iter()
            .map(|(device_id, encrypted, _, _)| (device_id, encrypted))
            .collect())
    }

    /// Encrypts one payload independently for every active peer device.
    pub(crate) fn encrypt_payload_for_contact_devices(
        &self,
        identity: &crate::identity::Identity,
        contact: &crate::contact::Contact,
        payload: &[u8],
    ) -> VauchiResult<Vec<PreparedDevicePayload>> {
        let contact_id = contact.id();
        let ex = contact
            .kind()
            .exchanged_data()
            .ok_or_else(|| VauchiError::InvalidState("contact not exchanged".into()))?;
        let peer_devices = self
            .storage
            .device()
            .load_contact_active_devices(contact_id)?;
        let targets: Vec<Option<crate::identity::BroadcastDevice>> = if peer_devices.is_empty() {
            vec![None]
        } else {
            peer_devices.into_iter().map(Some).collect()
        };
        let mut prepared = Vec::with_capacity(targets.len());
        for target in targets {
            let peer_device_id = target
                .as_ref()
                .map(|device| device.device_id)
                .unwrap_or([0; 32]);
            let existing = self
                .storage
                .ratchets()
                .load_ratchet_state_for_device(contact_id, &peer_device_id)?;
            let (mut ratchet, is_initiator) = match (existing, target.as_ref()) {
                (Some(session), _) => session,
                (None, Some(device)) => {
                    crate::exchange::ratchet_bootstrap::bootstrap_device_pair_ratchet(
                        &ex.shared_key,
                        identity.signing_public_key(),
                        identity.device_id(),
                        identity.device_info().exchange_keypair(),
                        &ex.public_key,
                        &device.device_id,
                        &device.exchange_public_key,
                    )
                    .map_err(|error| {
                        VauchiError::Crypto(format!("device-pair ratchet: {error:?}"))
                    })?
                }
                (None, None) => return Err(VauchiError::NotFound("ratchet state".into())),
            };
            let ratchet_msg = ratchet.encrypt(payload).map_err(|error| match error {
                crate::crypto::ratchet::RatchetError::NoSendingChain => VauchiError::InvalidState(
                    "responder awaiting initiator's first message; deferring send".into(),
                ),
                other => VauchiError::Crypto(format!("{other:?}")),
            })?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|error| VauchiError::Serialization(error.to_string()))?;
            prepared.push((peer_device_id, encrypted, ratchet, is_initiator));
        }
        Ok(prepared)
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

            let prepared = match self.encrypt_payload_for_contact_devices(
                identity,
                &contact,
                &payload_bytes,
            ) {
                Ok(prepared) => prepared,
                Err(VauchiError::NotFound(_)) | Err(VauchiError::InvalidState(_)) => continue,
                Err(error) => return Err(error),
            };

            self.storage.with_savepoint(|| -> VauchiResult<()> {
                contact.set_cek(cek);
                self.storage.contacts().save_contact(&contact)?;
                let now = self.clock.unix_seconds();
                for (device_id, encrypted, ratchet, is_initiator) in prepared {
                    self.storage.ratchets().save_ratchet_state_for_device(
                        contact.id(),
                        &device_id,
                        &ratchet,
                        is_initiator,
                    )?;
                    let update = PendingUpdate {
                        id: self.rng.uuid_v4(),
                        contact_id: contact.id().to_string(),
                        update_type: "cek_migration".to_string(),
                        payload: encrypted,
                        created_at: now,
                        retry_count: 0,
                        status: UpdateStatus::Pending,
                        target_relay_url: contact.relay_url().map(String::from),
                    };
                    self.storage.pending().queue_update(&update)?;
                }
                Ok(())
            })?;
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
            // Map the item to the screen-invalidation event its successful apply
            // should dispatch — computed before the match consumes `item`, then
            // fired at the success seam below so a device-synced change
            // live-refreshes the matching screen (ADR-021/043; Gap A of
            // 2026-06-30-sync-ui-invalidation-sibling-gaps, same class as !1209).
            let event = sync_item_event(&item);
            let result = match item {
                SyncItem::ContactAdded { contact_data, .. } => match contact_data.to_contact() {
                    Ok(contact) => self
                        .storage
                        .contacts()
                        .save_contact(&contact)
                        .map_err(|e| e.into()),
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
                    ref field_id,
                    is_visible,
                    ..
                } => self
                    .storage
                    .labels()
                    .save_contact_override(contact_id, field_id, is_visible)
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
                if let Some(event) = event {
                    self.events.dispatch(event);
                }
            }
        }

        Ok(applied)
    }
}

/// Map an applied [`SyncItem`] to the screen-invalidation [`VauchiEvent`] its
/// successful apply should dispatch.
///
/// Pure: no storage, no dispatch. [`Vauchi::apply_sync_items`] calls this
/// before the per-item match consumes the item, then dispatches the result at
/// the success seam so a device-synced change live-refreshes the matching
/// screen (ADR-021/043; Gap A of `2026-06-30-sync-ui-invalidation-sibling-gaps`,
/// same class as the `!1209` relay-receive fix). `None` = no mapped
/// invalidation event today: deletion-state changes have no `affected_screens`
/// entry, so the settings screen refreshes on next navigation.
fn sync_item_event(item: &crate::sync::device_sync::SyncItem) -> Option<VauchiEvent> {
    use crate::api::events::EventOrigin;
    use crate::sync::device_sync::SyncItem;

    match item {
        SyncItem::ContactAdded { contact_data, .. } => {
            contact_data
                .to_contact()
                .ok()
                .map(|contact| VauchiEvent::ContactAdded {
                    contact_id: contact.id().to_string(),
                    origin: EventOrigin::Synced,
                })
        }
        SyncItem::ContactRemoved { contact_id, .. } => Some(VauchiEvent::ContactRemoved {
            contact_id: contact_id.clone(),
        }),
        SyncItem::CardUpdated { field_label, .. } => Some(VauchiEvent::OwnCardUpdated {
            changed_fields: vec![field_label.clone()],
        }),
        SyncItem::VisibilityChanged {
            contact_id,
            field_id,
            ..
        } => Some(VauchiEvent::VisibilityChanged {
            contact_id: contact_id.clone(),
            field: field_id.clone(),
        }),
        SyncItem::LabelChange { label_id, .. } => Some(VauchiEvent::LabelSyncCompleted {
            label_id: label_id.clone(),
        }),
        SyncItem::ContactTrustChanged { contact_id, .. } => Some(VauchiEvent::ContactUpdated {
            contact_id: contact_id.clone(),
            changed_fields: vec!["recovery_trusted".to_string()],
        }),
        SyncItem::PersonalNoteChanged { contact_id, .. } => Some(VauchiEvent::ContactUpdated {
            contact_id: contact_id.clone(),
            changed_fields: vec!["personal_note".to_string()],
        }),
        SyncItem::ContactFieldNoteChanged { contact_id, .. } => Some(VauchiEvent::ContactUpdated {
            contact_id: contact_id.clone(),
            changed_fields: vec!["field_note".to_string()],
        }),
        SyncItem::ProposalTrustChanged { contact_id, .. } => Some(VauchiEvent::ContactUpdated {
            contact_id: contact_id.clone(),
            changed_fields: vec!["proposal_trusted".to_string()],
        }),
        SyncItem::ImportedContactAdded { contact_data, .. } => {
            contact_data
                .to_contact()
                .ok()
                .map(|contact| VauchiEvent::ContactAdded {
                    contact_id: contact.id().to_string(),
                    origin: EventOrigin::Synced,
                })
        }
        SyncItem::ImportedContactUpdated { contact_data, .. } => contact_data
            .to_contact()
            .ok()
            .map(|contact| VauchiEvent::ContactUpdated {
                contact_id: contact.id().to_string(),
                changed_fields: Vec::new(),
            }),
        SyncItem::ImportedContactRemoved { contact_id, .. } => Some(VauchiEvent::ContactRemoved {
            contact_id: contact_id.clone(),
        }),
        SyncItem::ContactArchived { contact_id, .. } => Some(VauchiEvent::ContactArchived {
            contact_id: contact_id.clone(),
        }),
        SyncItem::ContactUnarchived { contact_id, .. } => Some(VauchiEvent::ContactUnarchived {
            contact_id: contact_id.clone(),
        }),
        // No mapped invalidation event (no affected_screens entry today).
        SyncItem::DeletionScheduled { .. } | SyncItem::DeletionCancelled { .. } => None,
    }
}
