// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::api::{EventDispatcher, SyncConfig};
use crate::card::{ContactCard, ContactField, FieldType};
use crate::network::{MockTransport, RelayClient, RelayClientConfig};
use std::sync::Arc;

#[test]
fn own_device_order_actual_send_chunks_survive_reordering_retry_and_reload() {
    let pair = Pair::new();
    let contact = Contact::from_exchange(
        [0x44; 32],
        ContactCard::new("Original"),
        SymmetricKey::generate(),
        NOW,
    );
    let id = contact.id().to_string();
    pair.receiver
        .storage()
        .contacts()
        .save_contact(&contact)
        .unwrap();
    let mut sender = pair.sender();
    for index in 0..4 {
        let mut card = ContactCard::new(&format!("Version {index}"));
        for field in 0..20 {
            card.add_field(ContactField::new(
                FieldType::Custom,
                &format!("field{field}"),
                &"x".repeat(800),
                NOW,
            ))
            .unwrap();
        }
        sender
            .record_local_change(SyncItem::ContactCardUpdated {
                contact_id: id.clone(),
                card_json: serde_json::to_string(&card).unwrap(),
                timestamp: NOW,
            })
            .unwrap();
    }
    drop(sender);
    let sender = pair.sender();
    let relay = RelayClient::new(
        MockTransport::new(),
        RelayClientConfig::default(),
        "sender".into(),
    );
    let mut phase = SendPhase::new(
        relay,
        &pair.storage,
        SyncConfig::default(),
        Arc::new(EventDispatcher::new()),
    );
    phase.connect(&crate::rng::OsSecureRng::new()).unwrap();
    let receiver_device = pair.receiver.identity().unwrap().create_device_info(NOW);
    phase
        .send_device_sync(
            &sender,
            receiver_device.device_id(),
            receiver_device.exchange_public_key(),
            pair.sender.master_seed(),
        )
        .unwrap();
    let ciphertexts: Vec<Vec<u8>> = phase
        .relay()
        .connection()
        .transport()
        .sent_messages()
        .iter()
        .map(|message| match &message.payload {
            MessagePayload::EncryptedUpdate(update) => update.ciphertext.clone(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert!(ciphertexts.len() > 1, "exercise actual byte chunking");
    for ciphertext in ciphertexts.iter().rev() {
        let plaintext = pair.plaintext(ciphertext);
        let decoded = crate::sync::decode_sync_items_tolerantly(&plaintext).unwrap();
        assert!(plaintext.len() <= 48 * 1024 || decoded.known.len() == 1);
        pair.apply(ciphertext);
    }
    assert_eq!(
        pair.receiver
            .get_contact(&id)
            .unwrap()
            .unwrap()
            .display_name(),
        "Version 3"
    );
    for ciphertext in &ciphertexts {
        assert_eq!(pair.apply(ciphertext), 0);
    }
    assert_eq!(
        pair.sender()
            .pending_for_device(receiver_device.device_id())
            .len(),
        4,
        "send retries retain durable queue"
    );
}
