// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Exchange operations: QR code generation for contact exchange.

use crate::card::ContactCard;
use crate::contact::Contact;
use crate::crypto::ratchet::DoubleRatchetState;
use crate::exchange::{ExchangeEvent, ExchangeSession, ManualConfirmationVerifier};

use super::super::contact_manager::ContactManager;
use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::ProfileEngine;

/// Exchange QR data with expiration info.
#[derive(Debug, Clone)]
pub struct ExchangeQrData {
    /// The QR code data string.
    pub data: String,
    /// Unix timestamp when the QR was generated.
    pub generated_at: u64,
    /// QR expiration time in seconds.
    pub expires_in_secs: u64,
}

impl ExchangeQrData {
    /// Calculate remaining seconds until expiration relative to `now`.
    ///
    /// `now` is an explicit unix-seconds parameter — callers pass
    /// `vauchi.clock().unix_seconds()` in production and a fixed
    /// value in tests. Phase 1 / Task 1.1 / F3 mop-up: retires the
    /// ambient `SystemTime::now` read this method used to do.
    pub fn remaining_secs(&self, now: u64) -> u64 {
        let expires_at = self.generated_at + self.expires_in_secs;
        expires_at.saturating_sub(now)
    }

    /// Check if the QR code has expired relative to `now`.
    pub fn is_expired(&self, now: u64) -> bool {
        self.remaining_secs(now) == 0
    }
}

impl ProfileEngine {
    /// Generates exchange QR data for contact exchange.
    ///
    /// Uses ExchangeSession state machine with ManualConfirmationVerifier.
    /// Clones the identity internally since ExchangeSession requires ownership.
    pub fn generate_exchange_qr(&self) -> VauchiResult<ExchangeQrData> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let our_card = self
            .storage
            .contacts()
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(identity.display_name()));

        // Clone identity via storage serialization (ExchangeSession needs ownership)
        let identity_owned = crate::identity::Identity::from_storage_bytes(
            &identity.to_storage_bytes(),
            self.clock.unix_seconds(),
        )
        .map_err(|e| VauchiError::InvalidState(format!("Failed to clone identity: {:?}", e)))?;

        // Create exchange session for mutual QR exchange
        let verifier = ManualConfirmationVerifier::new();
        let mut session =
            ExchangeSession::new_qr(identity_owned, our_card, verifier, self.clock.clone());

        // Generate QR via state machine
        session
            .apply(ExchangeEvent::StartQR)
            .map_err(|e| VauchiError::InvalidState(format!("Failed to generate QR: {:?}", e)))?;

        let qr = session
            .qr()
            .ok_or_else(|| VauchiError::InvalidState("QR code not generated".into()))?;

        Ok(ExchangeQrData {
            data: qr.to_qr_url(),
            generated_at: qr.timestamp(),
            expires_in_secs: 300, // 5 minutes, matching QR_EXPIRY_SECONDS
        })
    }

    /// Upsert the card of a contact established via an in-person exchange,
    /// without (re)keying the channel. A contact the owner blocked stays
    /// blocked: the re-exchange is rejected before anything is written
    /// (ADR-056, #295), and the caller reports an ordinary failure.
    pub fn save_exchanged_card(&self, contact: &Contact) -> VauchiResult<()> {
        if let Some(event) = self.persist_exchanged_card(contact)? {
            self.events.dispatch(event);
        }
        Ok(())
    }

    fn persist_exchanged_card(&self, contact: &Contact) -> VauchiResult<Option<VauchiEvent>> {
        let blocked = self
            .storage
            .contacts()
            .load_contact(contact.id())?
            .is_some_and(|stored| stored.is_blocked());
        if blocked {
            return Err(VauchiError::ContactBlocked(contact.id().to_string()));
        }
        self.persist_contact_upsert(contact)
    }

    /// Persist a contact established (or re-established) via an in-person
    /// exchange.
    ///
    /// A first exchange stores the contact and keys its channel. A 2nd-to-nth
    /// exchange of the same pair UPSERTS the card — a repeat must never drop
    /// the peer's updated card (the historical BLE/multi-stage `add_contact`
    /// reject-and-return bug) — but keeps the shared key and ratchet the
    /// first exchange established: an exchange never rekeys an existing
    /// contact (private#941, ship checklist SC-06; this replaces the rekey of
    /// the repeat-exchange decision of 2026-06-27). The BLE, multi-stage and
    /// legacy-QR completion paths all route through here, so the policy
    /// lives in core, not per transport in the humble frontends
    /// (ADR-021/043).
    /// Contact, rekey status, ratchet and queued bootstrap writes commit
    /// together, so a failed write leaves the exchange safe to retry.
    pub fn save_exchanged_contact(
        &self,
        contact: &Contact,
        ratchet: &DoubleRatchetState,
        is_initiator: bool,
    ) -> VauchiResult<()> {
        let mut first_contact_name = None;
        let mut contact_event = None;
        self.storage.with_transaction(|| -> VauchiResult<()> {
            if let Some(mut stored) = self.storage.contacts().load_contact(contact.id())? {
                stored.update_card(contact.card().clone(), self.clock.unix_seconds());
                // The one exception to keeping the first exchange's keys: a
                // contact restored from a guardian backup and waiting for its
                // rekey has no relationship on this device, only an old shared
                // key a thief may also hold; meeting it in person gives it one
                // (vauchi/private#796, D20).
                if !self.restored_waiting_for_rekey(contact.id())? {
                    contact_event = self.persist_exchanged_card(&stored)?;
                    return Ok(());
                }
                let shared = contact.shared_key().ok_or_else(|| {
                    VauchiError::InvalidState("An exchanged contact has a shared key".into())
                })?;
                stored
                    .adopt_exchange_shared_key(shared.clone(), self.clock.unix_seconds())
                    .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
                contact_event = self.persist_exchanged_card(&stored)?;
                self.storage.recovery().mark_rekey_met_again(contact.id())?;
            } else {
                contact_event = self.persist_exchanged_card(contact)?;
                first_contact_name = Some(contact.card().display_name().to_owned());
            }
            self.save_exchange_ratchet(contact.id(), ratchet, is_initiator)?;
            // Bootstrap card-update sync to the freshly-exchanged contact. The
            // initiator's first send establishes the responder's Double-Ratchet
            // sending chain — until the responder receives that message its
            // `ratchet.encrypt` errors ("no sending chain (responder must receive
            // first)"), so a responder that edits first can never propagate. Arming
            // the durable marker makes the next sync repropagate to the new contact
            // (a no-op for already-synced contacts, whose baseline is current); the
            // responder's own send is retried by the marker until that first receive
            // bootstraps its chain. The CLI does this via
            // `queue_initial_card_for_contact`; the mobile exchange path omitted it,
            // which silently broke device-to-device sync
            // (2026-06-30 device root-cause; `2026-06-28-sync-delivery-sent-not-received`).
            self.mark_own_card_repropagate()?;
            // A guardian in a recovery meeting hands back as the exchange with
            // the claim's new key completes (vauchi/private#796, D14); one that
            // cannot send yet owes it to the next sync.
            self.hand_back_owed()?;
            Ok(())
        })?;
        if let Some(event) = contact_event {
            self.events.dispatch(event);
        }
        if let Some(name) = first_contact_name {
            ContactManager::new(&self.storage, self.events.clone())
                .trigger_first_contact_moment(&name);
        }
        Ok(())
    }

    /// [`Self::save_exchanged_contact`] for an exchange whose peer has not
    /// yet said it saved us. A contact restored from a guardian backup and
    /// waiting for its rekey gets the new card but keeps its old key and
    /// stays waiting: adopting a key the peer may never store would cut the
    /// only channel left (vauchi/private#1217). Returns whether it did; call
    /// [`Self::save_exchanged_contact`] once the peer's done is read.
    pub fn save_exchanged_contact_before_peer_saved(
        &self,
        contact: &Contact,
        ratchet: &DoubleRatchetState,
        is_initiator: bool,
    ) -> VauchiResult<bool> {
        if self.restored_waiting_for_rekey(contact.id())?
            && let Some(mut stored) = self.storage.contacts().load_contact(contact.id())?
        {
            stored.update_card(contact.card().clone(), self.clock.unix_seconds());
            self.save_exchanged_card(&stored)?;
            return Ok(true);
        }
        self.save_exchanged_contact(contact, ratchet, is_initiator)?;
        Ok(false)
    }

    fn restored_waiting_for_rekey(&self, contact_id: &str) -> VauchiResult<bool> {
        let recovery = self.storage.recovery();
        Ok(recovery.rekey_pending()?.iter().any(|id| id == contact_id)
            && !recovery.rekey_met_again(contact_id)?)
    }
}
