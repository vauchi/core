// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Legacy contact migration to CEK-protected card deltas.
//!
//! Split from `propagation.rs` (VRS04 file-size seam) — a self-contained
//! one-shot upgrade path, distinct from ongoing card propagation.

use crate::rng::SecureRngExt;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
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
}
