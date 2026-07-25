// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Card propagation, CEK migration, device lookup, content updates, and sync item application.

use crate::contact_card::{ContactCard, ContactField};
use crate::rng::SecureRngExt;

use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::Vauchi;

/// One per-device ciphertext plus the session that produced it.
pub(crate) struct PreparedDevicePayload {
    pub(crate) peer_device_id: [u8; 32],
    pub(crate) encrypted: Vec<u8>,
    pub(crate) session: crate::crypto::DoubleRatchetState,
    pub(crate) is_initiator: bool,
    /// False only for genesis-born sessions: persisting one would make the
    /// sender's next alert ride a private chain a guarded receiver never
    /// re-seats to, silently dropping every alert after the first
    /// (ADR-064 Amendment 2026-07-24, guarded invariant 2).
    pub(crate) persist_session: bool,
}

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
        contact
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
        let delta = delta.filter_with(|fid| {
            self.get_effective_field_visibility(contact_id, fid)
                .unwrap_or(false)
        });
        if delta.is_empty() {
            return Err(VauchiError::InvalidState(
                "empty delta after visibility filter".into(),
            ));
        }

        let prepared =
            self.seal_and_persist_card_delta(identity, &mut contact, delta, |_| Ok(()))?;

        Ok(prepared
            .into_iter()
            .map(|payload| (payload.peer_device_id, payload.encrypted))
            .collect())
    }

    /// Stamps the next sent-version, signs, CEK-wraps, and per-device
    /// encrypts a visibility-filtered card delta, then persists the CEK,
    /// the advanced per-device ratchets, and the version floor in one
    /// savepoint. `extra_persist` runs inside that same savepoint for
    /// caller-specific writes (queueing pending updates, sent baselines).
    ///
    /// The single version-stamping site for both delta build paths —
    /// this stamping drifting apart between edit propagation (here) and
    /// repropagation (`features.rs`) produced the 2026-07-19
    /// delta-version-floor bug.
    ///
    /// Always emits CEK format (version 0x02): receivers reject legacy raw
    /// deltas, and a freshly-exchanged contact has no CEK yet, so one is
    /// generated per delta (2026-06-29-card-update-duplicate-message-paths).
    pub(crate) fn seal_and_persist_card_delta(
        &self,
        identity: &crate::identity::Identity,
        contact: &mut crate::contact::Contact,
        mut delta: crate::sync::delta::CardDelta,
        extra_persist: impl FnOnce(&[PreparedDevicePayload]) -> VauchiResult<()>,
    ) -> VauchiResult<Vec<PreparedDevicePayload>> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::sync::delta::{CekWrappedPayload, VersionedPayload};

        let contact_id = contact.id().to_string();

        // Version tracking for downgrade detection (#42)
        let next_version = self
            .storage
            .contacts()
            .last_sent_delta_version(&contact_id)
            .unwrap_or(0)
            + 1;
        delta.set_version(next_version);

        // Sign delta with our identity, bound to recipient
        let recipient_pk = *contact
            .public_key()
            .ok_or_else(|| VauchiError::InvalidState("Contact has no public key".into()))?;
        delta.sign(identity, &recipient_pk);

        let delta_bytes =
            serde_json::to_vec(&delta).map_err(|e| VauchiError::Serialization(e.to_string()))?;

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
            self.encrypt_payload_for_contact_devices(identity, contact, &payload_bytes)?;

        self.storage.with_savepoint(|| -> VauchiResult<()> {
            contact.set_cek(new_cek);
            self.storage.contacts().save_contact(contact)?;
            for payload in prepared.iter().filter(|p| p.persist_session) {
                self.storage.ratchets().save_ratchet_state_for_device(
                    &contact_id,
                    &payload.peer_device_id,
                    &payload.session,
                    payload.is_initiator,
                )?;
            }
            self.storage
                .contacts()
                .record_sent_delta_version(&contact_id, next_version)?;
            extra_persist(&prepared)?;
            Ok(())
        })?;

        Ok(prepared)
    }

    /// This identity's signed registry broadcast for genesis announcement.
    ///
    /// Uses the persisted multi-device registry when present, else the
    /// single-device registry derived from this identity — either way the
    /// receiver merges it additively (never destructively) on genesis
    /// receipt (ADR-068 §Decision req 6, plan §REVISION F2/F3).
    fn own_registry_broadcast(
        &self,
        identity: &crate::identity::Identity,
    ) -> VauchiResult<crate::identity::RegistryBroadcast> {
        let registry = match self.storage.device().load_device_registry()? {
            Some(registry) => registry,
            None => identity.initial_device_registry(),
        };
        Ok(crate::identity::RegistryBroadcast::new(
            &registry,
            identity.signing_keypair(),
            self.clock.unix_seconds(),
        ))
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
        // Registry presence alone never opens the per-device path: resolving
        // our device-scoped tokens requires the peer to hold OUR registry,
        // which only a completed bilateral handshake confirms (ADR-064
        // Amendment 2026-07-25 — the refuted B-lite hazard). Anything short
        // of Active rides the legacy [0;32] session the peer is known to
        // resolve.
        let activation_confirmed = self
            .storage
            .registry_activation()
            .load_activation(contact_id)?
            .map(|tracker| {
                tracker.state() == crate::sync::registry_activation::ActivationState::Active
            })
            .unwrap_or(false);
        let targets: Vec<Option<crate::identity::BroadcastDevice>> =
            if peer_devices.is_empty() || !activation_confirmed {
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
                (None, None) => {
                    // No established session and no peer registry — the
                    // secondary-device / first-contact case. A safety alert
                    // bootstraps a genesis session rooted in shared_key
                    // (ADR-068); any other payload has no genesis path and
                    // keeps failing closed.
                    if let Ok(crate::sync::delta::VersionedPayload::Alert(_)) =
                        crate::sync::delta::VersionedPayload::decode(payload)
                    {
                        let broadcast = self.own_registry_broadcast(identity)?;
                        let epoch = crate::network::mailbox_token::current_day_epoch(
                            self.clock.unix_seconds(),
                        );
                        let (message, session) = crate::exchange::genesis::GenesisEnvelope::seal(
                            &ex.shared_key,
                            identity,
                            &ex.public_key,
                            &broadcast,
                            epoch,
                            payload,
                        )
                        .map_err(|error| VauchiError::Crypto(format!("genesis seal: {error}")))?;
                        let encrypted = serde_json::to_vec(&message)
                            .map_err(|error| VauchiError::Serialization(error.to_string()))?;
                        prepared.push(PreparedDevicePayload {
                            peer_device_id: [0; 32],
                            encrypted,
                            session,
                            is_initiator: true,
                            persist_session: false,
                        });
                        continue;
                    }
                    return Err(VauchiError::NotFound("ratchet state".into()));
                }
            };
            let ratchet_msg = ratchet.encrypt(payload).map_err(|error| match error {
                crate::crypto::ratchet::RatchetError::NoSendingChain => VauchiError::InvalidState(
                    "responder awaiting initiator's first message; deferring send".into(),
                ),
                other => VauchiError::Crypto(format!("{other:?}")),
            })?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|error| VauchiError::Serialization(error.to_string()))?;
            prepared.push(PreparedDevicePayload {
                peer_device_id,
                encrypted,
                session: ratchet,
                is_initiator,
                persist_session: true,
            });
        }
        Ok(prepared)
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
                SyncItem::ContactCardUpdated {
                    ref contact_id,
                    ref card_json,
                    timestamp,
                } => match self.storage.contacts().load_contact(contact_id)? {
                    Some(mut contact) => {
                        let card: crate::contact_card::ContactCard =
                            serde_json::from_str(card_json).map_err(|error| {
                                VauchiError::InvalidState(format!(
                                    "invalid device-synced contact card: {error}"
                                ))
                            })?;
                        contact.update_card(card, timestamp);
                        self.storage
                            .contacts()
                            .save_contact(&contact)
                            .map_err(|e| e.into())
                    }
                    // A peer update cannot recreate an owner-removed contact.
                    None => Ok(()),
                },
                SyncItem::DeviceRegistryChanged {
                    ref registry_json, ..
                } => {
                    let registry = crate::identity::DeviceRegistry::from_json(registry_json)
                        .map_err(|error| VauchiError::InvalidState(error.to_string()))?;
                    let identity = self
                        .identity
                        .as_ref()
                        .ok_or(VauchiError::IdentityNotInitialized)?;
                    if registry.device_count() > crate::identity::MAX_DEVICES
                        || registry.find_device(identity.device_id()).is_none()
                        || !registry.verify(&identity.signing_keypair().public_key())
                    {
                        Err(VauchiError::InvalidState(
                            "invalid owner device registry update".to_string(),
                        ))
                    } else {
                        let current_version = self
                            .storage
                            .device()
                            .load_device_registry()?
                            .map(|current| current.version())
                            .unwrap_or(0);
                        if registry.version() > current_version {
                            self.storage.device().save_device_registry(&registry)?;
                        }
                        Ok(())
                    }
                }
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
                                .map_err(VauchiError::from)
                                .and_then(|()| self.mark_own_card_repropagate())
                        }
                        None => Err(VauchiError::IdentityNotInitialized),
                    }
                }
                SyncItem::CardFieldSynced {
                    ref field,
                    ref field_visibility,
                    ..
                } => match self.storage.contacts().load_own_card()? {
                    Some(mut card) => {
                        let mut synced_field = field.clone();
                        if let Some(existing) = card
                            .fields()
                            .iter()
                            .find(|existing| existing.id() == synced_field.id())
                            && synced_field.updated_at() <= existing.updated_at()
                        {
                            // `process_incoming` has already selected this
                            // equal-time write by the ADR-020 device-id
                            // tie-break. Advance its field clock so the
                            // resolved winner also dominates delayed
                            // re-propagation deltas on peer cards, whose V1
                            // field payload has no originating-device stamp.
                            let value = synced_field.value().to_string();
                            synced_field.set_value(&value, existing.updated_at().saturating_add(1));
                        }
                        if let Some(existing_id) = card
                            .fields()
                            .iter()
                            .find(|existing| {
                                existing.label() == synced_field.label()
                                    && existing.id() != synced_field.id()
                            })
                            .map(|existing| existing.id().to_string())
                        {
                            card.remove_field(&existing_id).map_err(VauchiError::from)?;
                        }
                        card.add_field(synced_field.clone())
                            .map_err(VauchiError::from)?;
                        match field_visibility {
                            Some(crate::visibility::FieldVisibility::Everyone) => {
                                card.field_visibility_mut().set_everyone(synced_field.id());
                            }
                            Some(crate::visibility::FieldVisibility::Contacts(contacts)) => {
                                card.field_visibility_mut()
                                    .set_contacts(synced_field.id(), contacts.clone());
                            }
                            Some(crate::visibility::FieldVisibility::Nobody) => {
                                card.field_visibility_mut().set_nobody(synced_field.id());
                            }
                            None => card.field_visibility_mut().remove(synced_field.id()),
                        }
                        self.storage
                            .contacts()
                            .save_own_card(&card)
                            .map_err(VauchiError::from)?;
                        self.mark_own_card_repropagate()
                    }
                    None => Err(VauchiError::IdentityNotInitialized),
                },
                SyncItem::CardFieldRemoved {
                    ref field_label, ..
                } => match self.storage.contacts().load_own_card()? {
                    Some(mut card) => {
                        if let Some(field_id) = card
                            .fields()
                            .iter()
                            .find(|field| field.label() == field_label)
                            .map(|field| field.id().to_string())
                        {
                            card.remove_field(&field_id).map_err(VauchiError::from)?;
                            self.storage.contacts().save_own_card(&card)?;
                        }
                        Ok(())
                    }
                    None => Err(VauchiError::IdentityNotInitialized),
                },
                SyncItem::VisibilityChanged {
                    ref contact_id,
                    ref field_id,
                    is_visible,
                    ..
                } => self
                    .storage
                    .labels()
                    .save_contact_override(contact_id, field_id, is_visible)
                    .map_err(VauchiError::from)
                    .and_then(|()| self.mark_own_card_repropagate()),
                SyncItem::GroupChanged { ref group_data, .. } => self
                    .storage
                    .labels()
                    .save_group(&group_data.to_group())
                    .map_err(|e| e.into()),
                SyncItem::GroupDeleted { ref group_id, .. } => self
                    .storage
                    .labels()
                    .delete_group(group_id)
                    .map_err(|e| e.into()),
                SyncItem::TagChanged { ref tag_data, .. } => self
                    .storage
                    .tags()
                    .save_tag(&tag_data.to_tag())
                    .map_err(|e| e.into()),
                SyncItem::TagDeleted { ref tag_id, .. } => self
                    .storage
                    .tags()
                    .delete_tag(tag_id)
                    .map(|_| ())
                    .map_err(|e| e.into()),
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
                } => {
                    // The wire payload is plaintext; notes are stored
                    // encrypted at rest (ADR-021), so the apply arm must
                    // encrypt with the contact's shared key exactly like
                    // add_personal_note — storing raw bytes would make the
                    // note undecryptable via read_personal_note.
                    let contact = match self.storage.contacts().load_contact(contact_id)? {
                        Some(contact) => contact,
                        None => {
                            // A note cannot recreate an owner-removed contact.
                            continue;
                        }
                    };
                    let shared_key = match contact.shared_key() {
                        Some(key) => key,
                        None => {
                            return Err(VauchiError::InvalidState(format!(
                                "PersonalNoteChanged for contact {contact_id} without shared key"
                            )));
                        }
                    };
                    let encrypted =
                        crate::crypto::encrypt(shared_key, note.as_bytes()).map_err(|error| {
                            VauchiError::Configuration(format!("Encryption failed: {error}"))
                        })?;
                    self.storage
                        .contacts()
                        .save_personal_notes(contact_id, &encrypted)
                        .map_err(|e| e.into())
                }
                SyncItem::PersonalNoteRemoved { ref contact_id, .. } => self
                    .storage
                    .contacts()
                    .delete_personal_notes(contact_id)
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
        SyncItem::ContactCardUpdated { contact_id, .. } => Some(VauchiEvent::ContactUpdated {
            contact_id: contact_id.clone(),
            changed_fields: vec!["card".to_string()],
        }),
        SyncItem::DeviceRegistryChanged { .. } => None,
        SyncItem::CardUpdated { field_label, .. } => Some(VauchiEvent::OwnCardUpdated {
            changed_fields: vec![field_label.clone()],
        }),
        SyncItem::CardFieldSynced { field, .. } => Some(VauchiEvent::OwnCardUpdated {
            changed_fields: vec![field.label().to_string()],
        }),
        SyncItem::CardFieldRemoved { field_label, .. } => Some(VauchiEvent::OwnCardUpdated {
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
        SyncItem::GroupChanged { group_data, .. } => Some(VauchiEvent::LabelSyncCompleted {
            label_id: group_data.id.clone(),
        }),
        SyncItem::GroupDeleted { group_id, .. } => Some(VauchiEvent::LabelSyncCompleted {
            label_id: group_id.clone(),
        }),
        SyncItem::TagChanged { .. } | SyncItem::TagDeleted { .. } => None,
        SyncItem::ContactTrustChanged { contact_id, .. } => Some(VauchiEvent::ContactUpdated {
            contact_id: contact_id.clone(),
            changed_fields: vec!["recovery_trusted".to_string()],
        }),
        SyncItem::PersonalNoteChanged { contact_id, .. }
        | SyncItem::PersonalNoteRemoved { contact_id, .. } => Some(VauchiEvent::ContactUpdated {
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
