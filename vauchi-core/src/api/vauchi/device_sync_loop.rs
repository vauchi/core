// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live device-to-device sync drivers.
//!
//! The device-sync engine (`DeviceSyncOrchestrator`, version vectors,
//! per-device ECDH encryption) was fully built and tested but never
//! driven in the live sync loop: nothing flushed the pending queue, and
//! the receive loop dropped self-token blobs. This module wires both
//! directions into `Vauchi::sync`'s send/receive phases.
//!
//! - **Send** ([`Vauchi::run_device_sync_send`]): after the contact-card
//!   send, flush each linked device's queued [`SyncItem`]s over the same
//!   relay connection (self-token routing, `EncryptedUpdate`).
//! - **Receive** ([`Vauchi::apply_device_sync_blob`]): a self-token blob
//!   is decrypted via per-device ECDH and the resulting items are applied
//!   to storage through the existing `apply_sync_items` path.
//!
//! Confidentiality is `DeviceSyncOrchestrator::encrypt_for_device` (ECDH
//! from the shared master seed + HKDF + XChaCha20-Poly1305) — no Double
//! Ratchet (see `2026-06-06-multi-device-sync-live-wiring` investigation
//! §2). `register_tokens` lives here too: it registers the same self-token
//! the receive partition keys on.

use std::collections::HashSet;

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};
use crate::api::sync::DeviceSyncOrchestrator;
use crate::api::sync_controller::SyncController;
use crate::contact::Contact;
use crate::identity::Identity;
use crate::network::mailbox_token::{
    batch_register_tokens, compute_self_token, current_day_epoch, token_hex,
};
use crate::network::{
    HttpTransportAdapter, MessagePayload, RegisterMailbox, Transport, create_envelope,
};
use crate::sync::device_sync::SyncItem;

impl Vauchi {
    /// Register mailbox tokens on the adapter for fetch routing.
    ///
    /// Computes contact tokens from shared keys and a self-token from the
    /// master seed, registers them via `RegisterMailbox` messages. Tokens
    /// are padded to 256 per batch and shuffled to prevent relay inference.
    pub(super) fn register_tokens(
        &self,
        identity: &Identity,
        contacts: &[Contact],
        adapter: &mut HttpTransportAdapter,
    ) -> VauchiResult<()> {
        // Collect shared keys from exchanged contacts
        let contact_keys: Vec<[u8; 32]> = contacts
            .iter()
            .filter_map(|c| c.shared_key().map(|k| *k.as_bytes()))
            .collect();

        let day = current_day_epoch(self.clock.unix_seconds());
        let master_seed = identity.master_seed();

        // Build padded token batches (256 per batch, shuffled)
        let batches = batch_register_tokens(self.rng.as_ref(), &contact_keys, master_seed, day, 0);

        // Register each batch with the adapter
        for tokens in batches {
            let envelope = create_envelope(
                MessagePayload::RegisterMailbox(RegisterMailbox { tokens }),
                self.clock.unix_seconds(),
            );
            adapter.send(&envelope).map_err(VauchiError::Network)?;
        }

        Ok(())
    }

    /// Self-token hexes for today and ±1 day (clock-drift tolerance).
    ///
    /// These identify inbound device-sync blobs: they were sent to
    /// `compute_self_token(master_seed, day)`, which every same-seed
    /// device can recompute and registered for via [`Self::register_tokens`].
    pub(super) fn self_token_hexes(&self, identity: &Identity) -> HashSet<String> {
        let day = current_day_epoch(self.clock.unix_seconds());
        let seed = identity.master_seed();
        [day.saturating_sub(1), day, day.saturating_add(1)]
            .iter()
            .map(|d| token_hex(&compute_self_token(seed, *d)))
            .collect()
    }

    /// Flush queued device-sync items to every linked device.
    ///
    /// Reuses the already-connected `SyncController` from the send phase.
    /// Best-effort per device: a send failure leaves that device's queue
    /// intact for the next cycle and does not abort the others. Returns the
    /// number of devices a non-empty batch was successfully sent to.
    pub(super) fn run_device_sync_send<T: Transport>(
        &self,
        ctrl: &mut SyncController<'_, T>,
        identity: &Identity,
    ) -> VauchiResult<usize> {
        // Gate: a sync target requires ≥2 registered devices.
        let registry = match self.storage.device().load_device_registry() {
            Ok(Some(r)) if r.device_count() > 1 => r,
            _ => return Ok(0),
        };

        let device_info = identity.create_device_info(self.clock.unix_seconds());
        let current_id = *device_info.device_id();
        let mut orchestrator =
            match DeviceSyncOrchestrator::load(&self.storage, device_info, registry.clone()) {
                Ok(o) => o,
                Err(_) => return Ok(0),
            };

        let master_seed = identity.master_seed();
        let mut sent = 0usize;

        for device_id in orchestrator.devices_with_pending() {
            if device_id == current_id {
                continue;
            }
            // Resolve the target device's exchange key; skip if revoked.
            let target_pub = match registry.find_device(&device_id) {
                Some(d) => d.exchange_public_key,
                None => continue,
            };

            let version = orchestrator.version_vector().get(&current_id);
            match ctrl.send_device_sync(&orchestrator, &device_id, &target_pub, master_seed) {
                Ok(()) => {
                    orchestrator.mark_synced(&device_id, version)?;
                    sent += 1;
                }
                Err(_) => {
                    // best-effort: leave queued, retry next cycle
                }
            }
        }

        Ok(sent)
    }

    /// Decrypt + apply one inbound device-sync blob.
    ///
    /// The envelope's `sender_id` is the shared identity, not the sending
    /// device, so the sender is identified by trying each *other*
    /// registered device's exchange key until AEAD authentication succeeds
    /// (linked-device count is tiny; no wire change needed). On success the
    /// decrypted [`SyncItem`]s pass through LWW conflict resolution and the
    /// shared `apply_sync_items` storage path. Returns the number applied
    /// (0 if no device key decrypts it — not ours / out-of-protocol).
    pub(super) fn apply_device_sync_blob(
        &self,
        identity: &Identity,
        ciphertext: &[u8],
    ) -> VauchiResult<usize> {
        let registry = match self.storage.device().load_device_registry() {
            Ok(Some(r)) if r.device_count() > 1 => r,
            _ => return Ok(0),
        };

        let device_info = identity.create_device_info(self.clock.unix_seconds());
        let current_id = *device_info.device_id();
        let mut orchestrator =
            match DeviceSyncOrchestrator::load(&self.storage, device_info, registry.clone()) {
                Ok(o) => o,
                Err(_) => return Ok(0),
            };

        for device in registry.active_devices() {
            if device.device_id == current_id {
                continue;
            }
            let plaintext =
                match orchestrator.decrypt_from_device(&device.exchange_public_key, ciphertext) {
                    Ok(p) => p,
                    Err(_) => continue, // not from this device — try the next key
                };
            let items: Vec<SyncItem> = match serde_json::from_slice(&plaintext) {
                Ok(i) => i,
                Err(_) => return Ok(0), // decrypted but malformed — drop
            };
            // The decrypting device IS the sender — use its id for the
            // ADR-020 tie-break (no wire field needed).
            let applied = orchestrator
                .process_incoming(items, &device.device_id)
                .map_err(VauchiError::DeviceSync)?;
            return self.apply_sync_items(applied);
        }

        Ok(0)
    }
}

// INLINE_TEST_REQUIRED: exercises the pub(super) device-sync drivers
// (run_device_sync_send / apply_device_sync_blob / self_token_hexes),
// crate-internal by design and unreachable from the tests/ crate without
// widening the public Vauchi surface.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::ContactCard;
    use crate::identity::device::DeviceInfo;
    use crate::sync::device_sync::ContactSyncData;

    const NOW: u64 = 1_000_000;

    /// A `ContactSyncData` for a `ContactAdded` item (mirrors tier2 helper).
    fn contact_sync_data(seed_byte: u8, name: &str) -> ContactSyncData {
        let public_key = [seed_byte; 32];
        let card = ContactCard::new(name);
        let visibility_rules =
            serde_json::to_string(&crate::contact::VisibilityRules::default()).unwrap();
        ContactSyncData {
            id: hex::encode(public_key),
            public_key: crate::identifiers::IdentityKey::from_bytes(public_key),
            display_name: name.to_string(),
            card_json: serde_json::to_string(&card).unwrap(),
            shared_key: [0xBB; 32],
            exchange_timestamp: NOW,
            fingerprint_verified: false,
            visibility_rules_json: visibility_rules,
            recovery_trusted: false,
        }
    }

    /// `self_token_hexes` yields exactly the today/±1-day self-tokens.
    // @internal
    #[test]
    fn self_token_hexes_covers_three_days() {
        let mut b = Vauchi::in_memory().unwrap();
        b.create_identity("Alice").unwrap();
        let identity = b.identity().unwrap();

        let tokens = b.self_token_hexes(identity);
        assert_eq!(tokens.len(), 3, "today + yesterday + tomorrow");

        let day = current_day_epoch(b.clock.unix_seconds());
        let seed = identity.master_seed();
        for d in [day - 1, day, day + 1] {
            let expected = token_hex(&compute_self_token(seed, d));
            assert!(tokens.contains(&expected), "missing self-token for day {d}");
        }
        // Each hex token is 64 chars (32 bytes).
        assert!(tokens.iter().all(|t| t.len() == 64));
    }

    /// End-to-end receive: a `ContactAdded` sealed by a peer device for B's
    /// exchange key is decrypted (sender identified by trial) and applied to
    /// B's storage. This is the core "changes propagate after linking" proof.
    // @internal
    #[test]
    fn apply_device_sync_blob_round_trip_adds_contact() {
        // Receiver B — a real identity (primary device = index 0).
        let mut b = Vauchi::in_memory().unwrap();
        b.create_identity("Alice").unwrap();
        let identity_b = b.identity().unwrap();
        let device_b = identity_b.create_device_info(NOW);

        // Synthetic sender device A (independent keypair — ECDH needs no
        // shared seed, only that B's registry carries A's public key).
        let seed_a = [0x11u8; 32];
        let device_a = DeviceInfo::derive(&seed_a, 0, "DeviceA".into(), NOW);

        // B's registry must hold both devices so device_count > 1 and the
        // sender-trial loop can find A's key.
        let mut registry = identity_b.initial_device_registry();
        registry
            .add_device_unsigned(device_a.to_registered(&seed_a))
            .unwrap();
        b.storage()
            .device()
            .save_device_registry(&registry)
            .unwrap();

        // A seals a ContactAdded item for B's exchange public key.
        let items = vec![SyncItem::ContactAdded {
            contact_data: contact_sync_data(0xAA, "Bob"),
            timestamp: NOW,
        }];
        let payload = serde_json::to_vec(&items).unwrap();
        let orch_a = DeviceSyncOrchestrator::new(b.storage(), device_a, registry.clone());
        let ciphertext = orch_a
            .encrypt_for_device(device_b.exchange_public_key(), &payload)
            .unwrap();

        // B applies the inbound blob.
        assert_eq!(b.contact_count().unwrap(), 0);
        let applied = b.apply_device_sync_blob(identity_b, &ciphertext).unwrap();
        assert_eq!(applied, 1, "one ContactAdded item applied");
        assert_eq!(b.contact_count().unwrap(), 1, "contact persisted on B");
        let bob = b.get_contact(&hex::encode([0xAAu8; 32])).unwrap();
        assert_eq!(bob.unwrap().display_name(), "Bob");
    }

    /// A blob no registered device can decrypt is dropped (returns 0), not
    /// misapplied — the negative path of the sender-trial loop.
    // @internal
    #[test]
    fn apply_device_sync_blob_undecryptable_returns_zero() {
        let mut b = Vauchi::in_memory().unwrap();
        b.create_identity("Alice").unwrap();
        let identity_b = b.identity().unwrap();

        // Two-device registry, but the blob is random bytes from no one.
        let seed_a = [0x22u8; 32];
        let device_a = DeviceInfo::derive(&seed_a, 0, "DeviceA".into(), NOW);
        let mut registry = identity_b.initial_device_registry();
        registry
            .add_device_unsigned(device_a.to_registered(&seed_a))
            .unwrap();
        b.storage()
            .device()
            .save_device_registry(&registry)
            .unwrap();

        let applied = b
            .apply_device_sync_blob(identity_b, b"not-a-valid-ciphertext")
            .unwrap();
        assert_eq!(applied, 0, "undecryptable blob applies nothing");
        assert_eq!(b.contact_count().unwrap(), 0);
    }
}
