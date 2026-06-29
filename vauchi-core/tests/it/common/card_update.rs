// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared card-update sealing for tests.
//!
//! Builds the wire bytes a recipient's device receiver
//! (`process_single_card_update`) consumes: sign the delta bound to the
//! recipient, CEK-wrap it (v0x02), ratchet-encrypt, serialize the
//! `RatchetMessage`. This is the test-side counterpart of the shipping seal so
//! tests drive the same seal→open contract devices run — replacing the
//! test-only `Vauchi::prepare_card_update_for_contact` sender that drifted from
//! the production path (2026-06-29-card-update-duplicate-message-paths).

use vauchi_core::Vauchi;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

/// Seal a card update from `sender` to the contact `recipient_id_at_sender`,
/// stamped with delta `version`. The delta is bound to the recipient's stored
/// signing key (as the production sender does). Returns the serialized
/// `RatchetMessage` bytes the recipient feeds to `process_single_card_update`.
pub fn seal_update(
    sender: &Vauchi,
    recipient_id_at_sender: &str,
    old_card: &ContactCard,
    new_card: &ContactCard,
    version: u32,
) -> Vec<u8> {
    let sender_identity = sender.identity().expect("sender has an identity");

    let recipient_pk = *sender
        .storage()
        .contacts()
        .load_contact(recipient_id_at_sender)
        .expect("load recipient contact")
        .expect("recipient contact exists in sender storage")
        .public_key()
        .expect("recipient contact carries a signing key");

    let mut delta = CardDelta::compute(old_card, new_card, 0);
    delta.set_version(version);
    delta.sign(sender_identity, &recipient_pk);

    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek = ContentEncryptionKey::generate();
    let cek_ciphertext = cek.encrypt(&delta_bytes).unwrap();
    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: delta.signature,
        nonce: delta.nonce,
    };
    let payload = VersionedPayload::encode_cek(&wrapped);

    let (mut ratchet, is_init) = sender
        .storage()
        .ratchets()
        .load_ratchet_state(recipient_id_at_sender)
        .expect("load sender ratchet")
        .expect("sender ratchet exists for recipient");
    let ratchet_msg = ratchet.encrypt(&payload).expect("ratchet encrypt");
    sender
        .storage()
        .ratchets()
        .save_ratchet_state(recipient_id_at_sender, &ratchet, is_init)
        .expect("save advanced sender ratchet");

    serde_json::to_vec(&ratchet_msg).unwrap()
}

/// Default-version (`1`) seal, matching the historical
/// `prepare_card_update_for_contact` behavior for tests that don't exercise
/// the #42 version floor.
pub fn seal_update_default(
    sender: &Vauchi,
    recipient_id_at_sender: &str,
    old_card: &ContactCard,
    new_card: &ContactCard,
) -> Vec<u8> {
    seal_update(sender, recipient_id_at_sender, old_card, new_card, 1)
}
