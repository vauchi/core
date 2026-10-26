// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sealing one payload for one contact: the CEK-wrapped card delta and
//! the stateless payloads (cover, a resync request) that ride the same
//! ratchet sessions without moving any card state.

use crate::rng::SecureRngExt;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

/// A card delta stamped, signed and CEK-wrapped for one contact
/// (`Vauchi::cek_wrapped_card_delta`).
pub(crate) struct SealedCardDelta {
    pub(crate) payload_bytes: Vec<u8>,
    pub(crate) cek: crate::crypto::cek::ContentEncryptionKey,
    pub(crate) version: u32,
}

impl Vauchi {
    /// Seals `payload` to every active device of `contact` and queues one
    /// `card_delta` per copy, persisting only the ratchet sessions — for
    /// payloads that carry no card state (cover, a resync request), where
    /// no CEK, delta version or baseline moves.
    pub(super) fn queue_sealed_payload(
        &self,
        identity: &crate::identity::Identity,
        contact: &crate::contact::Contact,
        payload: &[u8],
    ) -> VauchiResult<()> {
        use crate::storage::{PendingUpdate, UpdateStatus};

        let contact_id = contact.id();
        let prepared = self.encrypt_payload_for_contact_devices(identity, contact, payload)?;
        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            for copy in &prepared {
                if copy.persist_session {
                    self.storage.ratchets().save_ratchet_state_for_device(
                        contact_id,
                        &copy.peer_device_id,
                        &copy.session,
                        copy.is_initiator,
                    )?;
                }
                self.storage.pending().queue_update(&PendingUpdate {
                    id: self.rng.uuid_v4(),
                    contact_id: contact_id.to_string(),
                    update_type: "card_delta".to_string(),
                    payload: copy.encrypted.clone(),
                    created_at: now,
                    retry_count: 0,
                    status: UpdateStatus::Pending,
                    target_relay_url: None,
                    target_device_id: copy.target_device_id(),
                })?;
            }
            Ok(())
        })
    }

    /// Stamps, signs and CEK-wraps `delta` for `contact`: the plaintext the
    /// contact's ratchet sessions encrypt, with the CEK and version to persist
    /// once it is sent.
    pub(crate) fn cek_wrapped_card_delta(
        &self,
        identity: &crate::identity::Identity,
        contact: &crate::contact::Contact,
        mut delta: crate::sync::delta::CardDelta,
    ) -> VauchiResult<SealedCardDelta> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::sync::delta::{CekWrappedPayload, VersionedPayload};

        // Version tracking for downgrade detection (#42)
        let version = self
            .storage
            .contacts()
            .last_sent_delta_version(contact.id())
            .unwrap_or(0)
            + 1;
        delta.set_version(version);

        // Sign delta with our identity, bound to recipient
        let recipient_pk = *contact
            .public_key()
            .ok_or_else(|| VauchiError::InvalidState("Contact has no public key".into()))?;
        delta.sign(identity, &recipient_pk);

        let delta_bytes =
            serde_json::to_vec(&delta).map_err(|e| VauchiError::Serialization(e.to_string()))?;

        let cek = ContentEncryptionKey::generate();
        let cek_ciphertext = cek
            .encrypt(&delta_bytes)
            .map_err(|e| VauchiError::Crypto(format!("CEK encrypt: {:?}", e)))?;
        let wrapped = CekWrappedPayload {
            cek: cek.to_bytes(),
            cek_ciphertext,
            signature: delta.signature,
            nonce: delta.nonce,
        };
        Ok(SealedCardDelta {
            payload_bytes: VersionedPayload::encode_cek(&wrapped),
            cek,
            version,
        })
    }
}
