//! Sync and Update Propagation E2E Tests
//!
//! Feature: sync_updates.feature
//! Feature: relay_network.feature

use vauchi_core::{
    crypto::ratchet::DoubleRatchetState,
    exchange::X3DHKeyPair,
    network::{MockTransport, RelayClient, RelayClientConfig, TransportConfig},
    sync::{CardDelta, SyncManager},
    Contact, ContactCard, ContactField, FieldType, Storage, SymmetricKey, Vauchi,
};

/// Tests the sync and update propagation workflow.
///
/// Feature: sync_updates.feature
/// Scenarios: Update propagates to contacts, Queued updates delivered
#[test]
fn test_sync_update_propagation_happy_path() {
    // Step 1: Set up Alice and Bob with existing exchange
    let mut alice_wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    let mut bob_wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    alice_wb.create_identity("Alice").unwrap();
    bob_wb.create_identity("Bob").unwrap();

    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@old-company.com",
        ))
        .unwrap();

    let alice_public_key = *alice_wb.identity().unwrap().signing_public_key();
    let bob_public_key = *bob_wb.identity().unwrap().signing_public_key();

    let shared_secret = SymmetricKey::generate();

    let bob_contact = Contact::from_exchange(
        bob_public_key,
        ContactCard::new("Bob"),
        shared_secret.clone(),
    );
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_contact = Contact::from_exchange(
        alice_public_key,
        alice_wb.own_card().unwrap().unwrap(),
        shared_secret.clone(),
    );
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    // Initialize ratchets
    let bob_dh = X3DHKeyPair::generate();
    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_dh);

    // Step 2: Alice updates her card
    let old_card = alice_wb.own_card().unwrap().unwrap();
    let email_field_id = old_card.fields()[0].id().to_string();
    let mut modified_card = old_card.clone();
    modified_card
        .update_field_value(&email_field_id, "alice@new-company.com")
        .unwrap();
    alice_wb.update_own_card(&modified_card).unwrap();

    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+15559999999",
        ))
        .unwrap();

    let new_card = alice_wb.own_card().unwrap().unwrap();

    // Step 3: Compute delta for the update
    let delta = CardDelta::compute(&old_card, &new_card);
    assert!(!delta.changes.is_empty());

    // Step 4: Serialize and encrypt delta for Bob
    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let encrypted_update = alice_ratchet.encrypt(&delta_bytes).unwrap();

    // Step 5: Bob receives and decrypts the update
    let decrypted_bytes = bob_ratchet.decrypt(&encrypted_update).unwrap();
    let received_delta: CardDelta = serde_json::from_slice(&decrypted_bytes).unwrap();

    // Step 6: Bob applies delta to Alice's card
    let mut alice_card_at_bob = bob_wb.get_contact(&alice_contact_id).unwrap().unwrap();
    let mut card_copy = alice_card_at_bob.card().clone();
    received_delta.apply(&mut card_copy).unwrap();
    alice_card_at_bob.update_card(card_copy);
    bob_wb.storage().save_contact(&alice_card_at_bob).unwrap();

    // Step 7: Verify Bob has updated card
    let alice_in_bob = bob_wb.get_contact(&alice_contact_id).unwrap().unwrap();
    let updated_card = alice_in_bob.card();
    assert_eq!(updated_card.fields().len(), 2);

    let email_field = updated_card
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .unwrap();
    assert_eq!(email_field.value(), "alice@new-company.com");

    let phone_field = updated_card
        .fields()
        .iter()
        .find(|f| f.label() == "mobile")
        .unwrap();
    assert_eq!(phone_field.value(), "+15559999999");
}

/// Tests the sync manager's queue and delivery workflow.
///
/// Feature: sync_updates.feature
/// Scenarios: Queued updates delivered when contact comes online
#[test]
fn test_sync_manager_queue_happy_path() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let sync_manager = SyncManager::new(&storage);

    let mut old_card = ContactCard::new("Alice");
    old_card
        .add_field(ContactField::new(FieldType::Email, "work", "alice@old.com"))
        .unwrap();

    let mut new_card = ContactCard::new("Alice");
    new_card
        .add_field(ContactField::new(FieldType::Email, "work", "alice@new.com"))
        .unwrap();

    // Queue update for offline contact
    let contact_id = "bob-123";
    let update_id = sync_manager
        .queue_card_update(contact_id, &old_card, &new_card)
        .unwrap();
    assert!(!update_id.is_empty());

    // Verify update is pending
    let pending = sync_manager.get_pending(contact_id).unwrap();
    assert_eq!(pending.len(), 1);

    let state = sync_manager.get_sync_state(contact_id).unwrap();
    assert!(matches!(
        state,
        vauchi_core::SyncState::Pending {
            queued_count: 1,
            ..
        }
    ));

    // Simulate contact coming online - deliver update
    sync_manager.mark_delivered(&update_id).unwrap();

    // Verify update is no longer pending
    let pending = sync_manager.get_pending(contact_id).unwrap();
    assert_eq!(pending.len(), 0);

    let state = sync_manager.get_sync_state(contact_id).unwrap();
    assert!(matches!(state, vauchi_core::SyncState::Synced { .. }));
}

/// Tests a complete workflow with three users demonstrating all main features.
///
/// Combines: exchange, visibility, sync
#[test]
fn test_full_three_user_workflow() {
    let mut alice_wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    let mut bob_wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    let mut carol_wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    alice_wb.create_identity("Alice").unwrap();
    bob_wb.create_identity("Bob").unwrap();
    carol_wb.create_identity("Carol").unwrap();

    // Alice sets up her card with work and personal info
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
        ))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "personal",
            "alice@home.com",
        ))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(FieldType::Phone, "work", "+15551111111"))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "personal",
            "+15552222222",
        ))
        .unwrap();

    // Exchange with Bob
    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();
    let carol_pk = *carol_wb.identity().unwrap().signing_public_key();

    let alice_bob_secret = SymmetricKey::generate();
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), alice_bob_secret.clone());
    let bob_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    let alice_card = alice_wb.own_card().unwrap().unwrap();
    let alice_for_bob = Contact::from_exchange(alice_pk, alice_card.clone(), alice_bob_secret);
    let alice_id_bob = alice_for_bob.id().to_string();
    bob_wb.add_contact(alice_for_bob).unwrap();

    // Exchange with Carol
    let alice_carol_secret = SymmetricKey::generate();
    let carol_contact = Contact::from_exchange(
        carol_pk,
        ContactCard::new("Carol"),
        alice_carol_secret.clone(),
    );
    let carol_id = carol_contact.id().to_string();
    alice_wb.add_contact(carol_contact).unwrap();

    let alice_for_carol = Contact::from_exchange(alice_pk, alice_card.clone(), alice_carol_secret);
    let alice_id_carol = alice_for_carol.id().to_string();
    carol_wb.add_contact(alice_for_carol).unwrap();

    // Set visibility - Bob sees work only, Carol sees all
    let mut bob_contact = alice_wb.get_contact(&bob_id).unwrap().unwrap();
    bob_contact.visibility_rules_mut().set_nobody("personal");
    alice_wb.storage().save_contact(&bob_contact).unwrap();

    // Verify visibility rules
    let bob_contact = alice_wb.get_contact(&bob_id).unwrap().unwrap();
    let carol_contact = alice_wb.get_contact(&carol_id).unwrap().unwrap();

    assert!(bob_contact.visibility_rules().can_see("work", &bob_id));
    assert!(!bob_contact.visibility_rules().can_see("personal", &bob_id));
    assert!(carol_contact.visibility_rules().can_see("work", &carol_id));
    assert!(carol_contact
        .visibility_rules()
        .can_see("personal", &carol_id));

    // Alice updates her card
    let old_card = alice_wb.own_card().unwrap().unwrap();
    let work_email_id = old_card
        .fields()
        .iter()
        .find(|f| f.field_type() == FieldType::Email && f.label() == "work")
        .unwrap()
        .id()
        .to_string();

    let mut modified_card = old_card.clone();
    modified_card
        .update_field_value(&work_email_id, "alice@new-company.com")
        .unwrap();
    alice_wb.update_own_card(&modified_card).unwrap();

    let new_card = alice_wb.own_card().unwrap().unwrap();
    let delta = CardDelta::compute(&old_card, &new_card);
    assert!(!delta.changes.is_empty());

    // Apply updates to Bob and Carol
    let mut bob_alice_card = bob_wb
        .get_contact(&alice_id_bob)
        .unwrap()
        .unwrap()
        .card()
        .clone();
    delta.apply(&mut bob_alice_card).unwrap();

    let mut carol_alice_card = carol_wb
        .get_contact(&alice_id_carol)
        .unwrap()
        .unwrap()
        .card()
        .clone();
    delta.apply(&mut carol_alice_card).unwrap();

    // Verify both have updated work email
    let bob_work_email = bob_alice_card
        .fields()
        .iter()
        .find(|f| f.field_type() == FieldType::Email && f.label() == "work")
        .unwrap();
    assert_eq!(bob_work_email.value(), "alice@new-company.com");

    let carol_work_email = carol_alice_card
        .fields()
        .iter()
        .find(|f| f.field_type() == FieldType::Email && f.label() == "work")
        .unwrap();
    assert_eq!(carol_work_email.value(), "alice@new-company.com");

    let carol_personal = carol_alice_card
        .fields()
        .iter()
        .find(|f| f.label() == "personal" && f.field_type() == FieldType::Email);
    assert!(carol_personal.is_some());
}

/// Tests update delivery through relay when direct P2P fails.
///
/// Feature: sync_updates.feature, relay_network.feature
#[test]
fn test_relay_update_delivery_happy_path() {
    let transport = MockTransport::new();
    let config = RelayClientConfig {
        transport: TransportConfig::default(),
        max_pending_messages: 100,
        ack_timeout_ms: 30_000,
        max_retries: 3,
    };

    let mut client = RelayClient::new(transport, config, "alice-identity".into());

    // Connect to relay
    client.connect().unwrap();
    assert!(client.is_connected());

    // Set up encryption
    let shared_secret = SymmetricKey::generate();
    let bob_dh = X3DHKeyPair::generate();
    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    // Send encrypted update through relay
    let update_payload = b"Card update data";
    let msg_id = client
        .send_update(
            "bob-recipient-id",
            &mut alice_ratchet,
            update_payload,
            "update-001",
        )
        .unwrap();

    // Verify update is tracked
    assert!(!msg_id.is_empty());
    assert_eq!(client.in_flight_count(), 1);
    assert!(client
        .in_flight_update_ids()
        .contains(&"update-001".to_string()));

    // Clean up
    client.disconnect().unwrap();
    assert!(!client.is_connected());
}
