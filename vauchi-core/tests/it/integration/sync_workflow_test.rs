// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Workflow Integration Tests
//!
//! Tests for sync manager, relay client, and card propagation.

use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, SymmetricKey, SyncManager, Vauchi,
    crypto::ratchet::DoubleRatchetState,
    exchange::X3DHKeyPair,
    network::{MockTransport, RelayClient, RelayClientConfig, TransportConfig},
};

/// Test: Sync manager workflow
// @internal
#[test]
fn test_sync_manager_workflow() {
    use vauchi_core::Storage;

    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);
    let rng = vauchi_core::rng::OsSecureRng::new();

    let mut old_card = ContactCard::new("Test");
    old_card
        .add_field(ContactField::new(
            FieldType::Email,
            "email",
            "old@example.com",
            0,
        ))
        .unwrap();

    let mut new_card = ContactCard::new("Test");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "email",
            "new@example.com",
            0,
        ))
        .unwrap();

    let update_id = sync_manager
        .queue_card_update(&rng, "contact-1", &old_card, &new_card)
        .unwrap();
    assert!(!update_id.as_str().is_empty());

    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 1);

    let state = sync_manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, vauchi_core::SyncState::Pending { .. }));

    sync_manager.mark_delivered(&update_id).unwrap();

    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 0);

    let state = sync_manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, vauchi_core::SyncState::Synced { .. }));
}

/// Test: Relay client with mock transport
// @internal
#[test]
fn test_relay_client_workflow() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
        ..Default::default()
    };

    let mut client = RelayClient::new(transport, config, "test-identity".into());

    client.connect().unwrap();
    assert!(client.is_connected());

    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();

    let msg_id = client
        .send_update(
            0,
            "recipient-id",
            &mut ratchet,
            b"test payload",
            "update-1",
            None,
        )
        .unwrap();

    assert!(!msg_id.as_str().is_empty());
    assert_eq!(client.in_flight_count(), 1);

    let update_ids = client.in_flight_update_ids();
    assert!(update_ids.contains(&"update-1".to_string()));

    client.disconnect().unwrap();
    assert!(!client.is_connected());
}

/// Test: Field modification and removal propagation
///
/// Tests that add/modify/remove operations each produce the correct delta type.
// @internal
#[test]
fn test_field_modification_and_removal_propagation() {
    use vauchi_core::sync::{CardDelta, FieldChange};

    // Test 1: Field addition produces Added delta
    {
        let old = ContactCard::new("Alice");
        let mut new = ContactCard::new("Alice");
        new.add_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
            0,
        ))
        .unwrap();

        let delta = CardDelta::compute(&old, &new, 0);

        assert!(!delta.is_empty());
        assert!(
            delta
                .changes
                .iter()
                .any(|c| matches!(c, FieldChange::Added { .. })),
            "Adding a field should produce an Added delta"
        );
    }

    // Test 2: Field modification produces a complete-field Added upsert
    {
        let mut card = ContactCard::new("Alice");
        card.add_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
            0,
        ))
        .unwrap();
        let old = card.clone();

        let field_id = card.fields()[0].id().to_string();
        card.update_field_value(&field_id, "alice.smith@newcompany.com", 0)
            .unwrap();
        let new = card;

        let delta = CardDelta::compute(&old, &new, 0);

        assert!(!delta.is_empty());
        assert!(
            delta.changes.iter().any(|change| {
                matches!(change, FieldChange::Added { field } if field.id() == field_id)
            }),
            "modifying a field must preserve its complete timestamped value"
        );
    }

    // Test 3: Field removal produces Removed delta
    {
        let mut old = ContactCard::new("Alice");
        let field = ContactField::new(FieldType::Email, "work", "alice@company.com", 0);
        let field_id = field.id().to_string();
        old.add_field(field).unwrap();

        let new = ContactCard::new("Alice");

        let delta = CardDelta::compute(&old, &new, 0);

        assert!(!delta.is_empty());
        assert!(
            delta
                .changes
                .iter()
                .any(|c| matches!(c, FieldChange::Removed { field_id: id } if *id == field_id)),
            "Removing a field should produce a Removed delta"
        );
    }

    // Test 4: Full propagation roundtrip with modify
    {
        let mut alice_wb: Vauchi = Vauchi::in_memory().unwrap();
        let mut bob_wb: Vauchi = Vauchi::in_memory().unwrap();

        alice_wb.create_identity("Alice").unwrap();
        bob_wb.create_identity("Bob").unwrap();

        let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
        let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
        let shared_secret = SymmetricKey::generate();

        let bob_contact =
            Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone(), 0);
        let bob_id = bob_contact.id().to_string();
        alice_wb.add_contact(bob_contact).unwrap();

        let alice_contact = Contact::from_exchange(
            alice_pk,
            ContactCard::new("Alice"),
            shared_secret.clone(),
            0,
        );
        let alice_id = alice_contact.id().to_string();
        bob_wb.add_contact(alice_contact).unwrap();

        let bob_dh = X3DHKeyPair::generate();
        let alice_ratchet =
            DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key()).unwrap();
        let bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        alice_wb
            .storage()
            .ratchets()
            .save_ratchet_state(&bob_id, &alice_ratchet, true)
            .unwrap();
        bob_wb
            .storage()
            .ratchets()
            .save_ratchet_state(&alice_id, &bob_ratchet, false)
            .unwrap();

        let old_card = alice_wb.own_card().unwrap().unwrap();
        let field = ContactField::new(FieldType::Email, "work", "alice@company.com", 0);
        let field_id = field.id().to_string();
        alice_wb.add_own_field(field).unwrap();
        // Fields default hidden (field-centric model) — toggle Visible so
        // the roundtrip under test has a delta to carry.
        alice_wb.set_own_field_public(&field_id).unwrap();
        let new_card = alice_wb.own_card().unwrap().unwrap();

        let queued = alice_wb
            .propagate_card_update(&old_card, &new_card)
            .unwrap();
        assert_eq!(queued, 1, "Should queue update for Bob");

        let pending = alice_wb
            .storage()
            .pending()
            .get_pending_updates(&bob_id)
            .unwrap();
        assert!(!pending.is_empty(), "Should have pending update");

        let (mut ratchet, _) = bob_wb
            .storage()
            .ratchets()
            .load_ratchet_state(&alice_id)
            .unwrap()
            .unwrap();
        let ratchet_msg: vauchi_core::crypto::ratchet::RatchetMessage =
            serde_json::from_slice(&pending[0].payload).unwrap();
        let payload_bytes = ratchet.decrypt(&ratchet_msg).unwrap();

        // Payload is CEK-wrapped (version 0x02) — unwrap to get delta
        use vauchi_core::sync::delta::VersionedPayload;
        let vp = VersionedPayload::decode(&payload_bytes).unwrap();
        let delta_bytes = match vp {
            VersionedPayload::CekWrapped(wrapped) => {
                use vauchi_core::crypto::cek::ContentEncryptionKey;
                let cek = ContentEncryptionKey::from_bytes(wrapped.cek);
                cek.decrypt(&wrapped.cek_ciphertext).unwrap()
            }
            _ => panic!("expected CekWrapped variant"),
        };
        let delta: CardDelta = serde_json::from_slice(&delta_bytes).unwrap();

        assert!(
            delta
                .changes
                .iter()
                .any(|c| { matches!(c, FieldChange::Added { field } if field.label() == "work") }),
            "Bob should receive the work field in the delta"
        );
    }
}
