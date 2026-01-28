// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Workflow Integration Tests
//!
//! Tests for sync manager, relay client, and card propagation.

use vauchi_core::{
    crypto::ratchet::DoubleRatchetState,
    exchange::X3DHKeyPair,
    network::{MockTransport, RelayClient, RelayClientConfig, TransportConfig},
    sync::SyncManager,
    Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi,
};

/// Test: Sync manager workflow
#[test]
fn test_sync_manager_workflow() {
    use vauchi_core::Storage;

    // Create storage
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let sync_manager = SyncManager::new(&storage);

    // Queue a card update
    let mut old_card = ContactCard::new("Test");
    old_card
        .add_field(ContactField::new(
            FieldType::Email,
            "email",
            "old@example.com",
        ))
        .unwrap();

    let mut new_card = ContactCard::new("Test");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "email",
            "new@example.com",
        ))
        .unwrap();

    let update_id = sync_manager
        .queue_card_update("contact-1", &old_card, &new_card)
        .unwrap();
    assert!(!update_id.is_empty());

    // Check pending updates
    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 1);

    // Check sync state
    let state = sync_manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, vauchi_core::SyncState::Pending { .. }));

    // Mark as delivered
    sync_manager.mark_delivered(&update_id).unwrap();

    // Verify update was removed
    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 0);

    // State should now be synced
    let state = sync_manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, vauchi_core::SyncState::Synced { .. }));
}

/// Test: Relay client with mock transport
#[test]
fn test_relay_client_workflow() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
    };

    let mut client = RelayClient::new(transport, config, "test-identity".into());

    // Connect
    client.connect().unwrap();
    assert!(client.is_connected());

    // Set up ratchet for encryption
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();
    let mut ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    // Send an update
    let msg_id = client
        .send_update("recipient-id", &mut ratchet, b"test payload", "update-1")
        .unwrap();

    assert!(!msg_id.is_empty());
    assert_eq!(client.in_flight_count(), 1);

    // Check in-flight tracking
    let update_ids = client.in_flight_update_ids();
    assert!(update_ids.contains(&"update-1".to_string()));

    // Disconnect
    client.disconnect().unwrap();
    assert!(!client.is_connected());
}

/// Test: Field modification and removal propagation
///
/// Tests that add/modify/remove operations each produce the correct delta type.
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
        ))
        .unwrap();

        let delta = CardDelta::compute(&old, &new);

        assert!(!delta.is_empty());
        assert!(
            delta
                .changes
                .iter()
                .any(|c| matches!(c, FieldChange::Added { .. })),
            "Adding a field should produce an Added delta"
        );
    }

    // Test 2: Field modification produces Modified delta
    {
        let mut card = ContactCard::new("Alice");
        card.add_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
        ))
        .unwrap();
        let old = card.clone();

        // Get field ID and modify
        let field_id = card.fields()[0].id().to_string();
        card.update_field_value(&field_id, "alice.smith@newcompany.com")
            .unwrap();
        let new = card;

        let delta = CardDelta::compute(&old, &new);

        assert!(!delta.is_empty());
        assert!(
            delta
                .changes
                .iter()
                .any(|c| matches!(c, FieldChange::Modified { .. })),
            "Modifying a field value should produce a Modified delta"
        );
    }

    // Test 3: Field removal produces Removed delta
    {
        let mut old = ContactCard::new("Alice");
        let field = ContactField::new(FieldType::Email, "work", "alice@company.com");
        let field_id = field.id().to_string();
        old.add_field(field).unwrap();

        let new = ContactCard::new("Alice");

        let delta = CardDelta::compute(&old, &new);

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
        let mut alice_wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
        let mut bob_wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

        alice_wb.create_identity("Alice").unwrap();
        bob_wb.create_identity("Bob").unwrap();

        let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
        let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
        let shared_secret = SymmetricKey::generate();

        let bob_contact =
            Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone());
        let bob_id = bob_contact.id().to_string();
        alice_wb.add_contact(bob_contact).unwrap();

        let alice_contact =
            Contact::from_exchange(alice_pk, ContactCard::new("Alice"), shared_secret.clone());
        let alice_id = alice_contact.id().to_string();
        bob_wb.add_contact(alice_contact).unwrap();

        // Set up ratchets
        let bob_dh = X3DHKeyPair::generate();
        let alice_ratchet =
            DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
        let bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

        alice_wb
            .storage()
            .save_ratchet_state(&bob_id, &alice_ratchet, true)
            .unwrap();
        bob_wb
            .storage()
            .save_ratchet_state(&alice_id, &bob_ratchet, false)
            .unwrap();

        // Alice adds a field
        let old_card = alice_wb.own_card().unwrap().unwrap();
        alice_wb
            .add_own_field(ContactField::new(
                FieldType::Email,
                "work",
                "alice@company.com",
            ))
            .unwrap();
        let new_card = alice_wb.own_card().unwrap().unwrap();

        let queued = alice_wb
            .propagate_card_update(&old_card, &new_card)
            .unwrap();
        assert_eq!(queued, 1, "Should queue update for Bob");

        // Verify Bob can decrypt and receive the added field
        let pending = alice_wb.storage().get_pending_updates(&bob_id).unwrap();
        assert!(!pending.is_empty(), "Should have pending update");

        let (mut ratchet, _) = bob_wb
            .storage()
            .load_ratchet_state(&alice_id)
            .unwrap()
            .unwrap();
        let ratchet_msg: vauchi_core::crypto::ratchet::RatchetMessage =
            serde_json::from_slice(&pending[0].payload).unwrap();
        let delta_bytes = ratchet.decrypt(&ratchet_msg).unwrap();
        let delta: CardDelta = serde_json::from_slice(&delta_bytes).unwrap();

        // Verify the delta contains the added field
        assert!(
            delta
                .changes
                .iter()
                .any(|c| { matches!(c, FieldChange::Added { field } if field.label() == "work") }),
            "Bob should receive the work field in the delta"
        );
    }
}
