// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use vauchi_core::api::{DeviceSyncOrchestrator, EventDispatcher, SendPhase, SyncConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};
use vauchi_core::network::{
    MessagePayload, MockTransport, RelayClient, RelayClientConfig, TransportConfig,
};
use vauchi_core::sync::{SyncItem, decode_sync_items_tolerantly};
use vauchi_core::{Storage, Vauchi};

const ITEM_COUNT: usize = 10;
const FIELDS_PER_CARD: usize = 20;
const FIELD_VALUE_BYTES: usize = 800;
const MAX_DEVICE_SYNC_PAYLOAD_BYTES: usize = 48 * 1024;
const MASTER_SEED: [u8; 32] = [0x42; 32];

fn relay() -> RelayClient<MockTransport> {
    RelayClient::new(
        MockTransport::new(),
        RelayClientConfig {
            transport: TransportConfig::default(),
            ..Default::default()
        },
        "sender-identity".into(),
    )
}

fn large_card(index: usize) -> ContactCard {
    let mut card = ContactCard::new(&format!("Updated contact {index}"));
    for field_index in 0..FIELDS_PER_CARD {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field-{field_index}"),
            &format!("{index:02}-{}", "x".repeat(FIELD_VALUE_BYTES - 3)),
            index as u64 + 1,
        ))
        .unwrap();
    }
    card
}

// @scenario: release_privacy_multidevice_certification.feature:Every active device can exchange and update
#[test]
fn device_sync_send_chunking_reassembles_all_contact_card_updates() {
    let sender_storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut receiver = Vauchi::in_memory().unwrap();
    receiver.create_identity("Receiver").unwrap();

    let signing_key = SigningKeyPair::from_seed(&MASTER_SEED);
    let sender_device = DeviceInfo::derive(&MASTER_SEED, 0, "Sender".into(), 0);
    let receiver_device = DeviceInfo::derive(&MASTER_SEED, 1, "Receiver".into(), 0);
    let sender_device_id = *sender_device.device_id();
    let sender_public_key = *sender_device.exchange_public_key();
    let receiver_public_key = *receiver_device.exchange_public_key();
    let receiver_device_id = *receiver_device.device_id();

    let mut registry = DeviceRegistry::new(sender_device.to_registered(&MASTER_SEED), &signing_key);
    registry
        .add_device(receiver_device.to_registered(&MASTER_SEED), &signing_key)
        .unwrap();

    let mut sender_orchestrator =
        DeviceSyncOrchestrator::new(&sender_storage, sender_device, registry.clone());
    let mut expected_items = Vec::with_capacity(ITEM_COUNT);
    for index in 0..ITEM_COUNT {
        let contact = Contact::from_exchange(
            [index as u8 + 1; 32],
            ContactCard::new(&format!("Original contact {index}")),
            SymmetricKey::generate(),
            0,
        );
        let contact_id = contact.id().to_string();
        receiver
            .storage()
            .contacts()
            .save_contact(&contact)
            .unwrap();

        let item = SyncItem::ContactCardUpdated {
            contact_id,
            card_json: serde_json::to_string(&large_card(index)).unwrap(),
            timestamp: index as u64 + 1,
        };
        sender_orchestrator
            .record_local_change(item.clone())
            .unwrap();
        expected_items.push(item);
    }

    let mut send_phase = SendPhase::new(
        relay(),
        &sender_storage,
        SyncConfig::default(),
        Arc::new(EventDispatcher::new()),
    );
    send_phase
        .connect(&vauchi_core::rng::OsSecureRng::new())
        .unwrap();
    send_phase
        .send_device_sync(
            &sender_orchestrator,
            &receiver_device_id,
            &receiver_public_key,
            &MASTER_SEED,
        )
        .unwrap();

    let ciphertexts: Vec<Vec<u8>> = send_phase
        .relay()
        .connection()
        .transport()
        .sent_messages()
        .iter()
        .map(|message| match &message.payload {
            MessagePayload::EncryptedUpdate(update) => update.ciphertext.clone(),
            other => panic!("expected encrypted device-sync update, got {other:?}"),
        })
        .collect();

    // Mutation check: removing send-side chunking makes this assertion fail.
    assert!(
        ciphertexts.len() > 1,
        "a payload several times over the byte budget must use multiple blobs"
    );

    let receiver_device = DeviceInfo::derive(&MASTER_SEED, 1, "Receiver".into(), 0);
    let mut receiver_orchestrator =
        DeviceSyncOrchestrator::new(receiver.storage(), receiver_device, registry);
    let mut reassembled_items = Vec::new();
    let mut applied_count = 0;

    for ciphertext in ciphertexts {
        let plaintext = receiver_orchestrator
            .decrypt_from_device(&sender_public_key, &ciphertext)
            .unwrap();
        let decoded = decode_sync_items_tolerantly(&plaintext).unwrap();
        assert!(
            plaintext.len() <= MAX_DEVICE_SYNC_PAYLOAD_BYTES || decoded.known.len() == 1,
            "a multi-item plaintext batch exceeded the send byte budget"
        );

        let applied = receiver_orchestrator
            .process_incoming(decoded.known, &sender_device_id)
            .unwrap();
        applied_count += receiver.apply_sync_items(applied.clone()).unwrap();
        reassembled_items.extend(applied);
    }

    assert_eq!(applied_count, expected_items.len());
    assert_eq!(reassembled_items, expected_items);
    for (index, item) in expected_items.iter().enumerate() {
        let SyncItem::ContactCardUpdated { contact_id, .. } = item else {
            unreachable!("the fixture contains only contact-card updates");
        };
        let contact = receiver.get_contact(contact_id).unwrap().unwrap();
        assert_eq!(
            contact.display_name(),
            format!("Updated contact {index}").as_str()
        );
        assert_eq!(contact.card().fields().len(), FIELDS_PER_CARD);
        let expected_value = format!("{index:02}-{}", "x".repeat(FIELD_VALUE_BYTES - 3));
        for (field_index, field) in contact.card().fields().iter().enumerate() {
            assert_eq!(field.label(), format!("field-{field_index}"));
            assert_eq!(field.value(), expected_value);
        }
    }
}
