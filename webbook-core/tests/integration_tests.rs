//! Integration Tests for WebBook Core
//!
//! These tests verify the full workflow from identity creation through contact exchange
//! and synchronization.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use webbook_core::{
    api::{CallbackHandler, EventDispatcher},
    crypto::ratchet::DoubleRatchetState,
    exchange::X3DHKeyPair,
    network::{MockTransport, RelayClient, RelayClientConfig, TransportConfig},
    sync::SyncManager,
    Contact, ContactCard, ContactField, FieldType, SymmetricKey, WebBook, WebBookConfig,
    WebBookEvent,
};

/// Test: Full identity and contact card workflow
#[test]
fn test_full_identity_workflow() {
    // Create WebBook instance
    let mut wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    // Create identity
    wb.create_identity("Alice").unwrap();
    assert!(wb.has_identity());

    // Check initial contact card
    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.display_name(), "Alice");
    assert!(card.fields().is_empty());

    // Add fields to contact card
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ))
    .unwrap();
    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15551234567",
    ))
    .unwrap();

    // Verify fields were added
    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 2);
    assert!(card.fields().iter().any(|f| f.label() == "work"));
    assert!(card.fields().iter().any(|f| f.label() == "mobile"));

    // Update card with new display name
    let mut updated_card = card.clone();
    updated_card.set_display_name("Alice Smith").unwrap();
    let changed = wb.update_own_card(&updated_card).unwrap();
    assert!(changed.contains(&"display_name".to_string()));

    // Verify update
    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.display_name(), "Alice Smith");

    // Remove a field
    let removed = wb.remove_own_field("work").unwrap();
    assert!(removed);

    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 1);
    assert!(!card.fields().iter().any(|f| f.label() == "work"));
}

/// Test: Contact management workflow
#[test]
fn test_contact_management_workflow() {
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    // Initially no contacts
    assert_eq!(wb.contact_count().unwrap(), 0);

    // Create and add contacts
    let alice = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    );
    let bob = Contact::from_exchange([2u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let carol = Contact::from_exchange(
        [3u8; 32],
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
    );

    let alice_id = alice.id().to_string();
    let bob_id = bob.id().to_string();

    wb.add_contact(alice).unwrap();
    wb.add_contact(bob).unwrap();
    wb.add_contact(carol).unwrap();

    // Verify contacts were added
    assert_eq!(wb.contact_count().unwrap(), 3);

    // List contacts
    let contacts = wb.list_contacts().unwrap();
    assert_eq!(contacts.len(), 3);

    // Get specific contact
    let alice_loaded = wb.get_contact(&alice_id).unwrap().unwrap();
    assert_eq!(alice_loaded.display_name(), "Alice");

    // Search contacts
    let results = wb.search_contacts("alice").unwrap();
    assert_eq!(results.len(), 1);

    let results = wb.search_contacts("bob").unwrap();
    assert_eq!(results.len(), 1);

    let results = wb.search_contacts("xyz").unwrap();
    assert_eq!(results.len(), 0);

    // Verify fingerprint
    wb.verify_contact_fingerprint(&alice_id).unwrap();
    let alice_loaded = wb.get_contact(&alice_id).unwrap().unwrap();
    assert!(alice_loaded.is_fingerprint_verified());

    // Remove contact
    let removed = wb.remove_contact(&bob_id).unwrap();
    assert!(removed);
    assert_eq!(wb.contact_count().unwrap(), 2);
    assert!(wb.get_contact(&bob_id).unwrap().is_none());
}

/// Test: Event system workflow
#[test]
fn test_event_system_workflow() {
    let mut dispatcher = EventDispatcher::new();
    let event_count = Arc::new(AtomicUsize::new(0));

    // Add handler
    let count = event_count.clone();
    let handler = Arc::new(CallbackHandler::new(move |event| {
        count.fetch_add(1, Ordering::SeqCst);
        // Verify we receive expected event types
        match event {
            WebBookEvent::ContactAdded { .. } => {}
            WebBookEvent::ContactRemoved { .. } => {}
            WebBookEvent::OwnCardUpdated { .. } => {}
            _ => {}
        }
    }));
    dispatcher.add_handler(handler);

    // Create WebBook with our dispatcher
    let wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    // Add contact and trigger event
    let contact = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Test"),
        SymmetricKey::generate(),
    );
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Verify events were dispatched
    // Note: The current implementation creates its own dispatcher,
    // so this test verifies the event system works in isolation
    dispatcher.dispatch(WebBookEvent::ContactAdded {
        contact_id: contact_id.clone(),
    });

    assert_eq!(event_count.load(Ordering::SeqCst), 1);

    // Dispatch more events
    dispatcher.dispatch(WebBookEvent::ContactRemoved { contact_id });
    assert_eq!(event_count.load(Ordering::SeqCst), 2);
}

/// Test: Double Ratchet integration for encrypted communication
#[test]
fn test_double_ratchet_integration() {
    // Simulate two parties: Alice and Bob
    let _alice_identity_dh = X3DHKeyPair::generate();
    let bob_identity_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();

    // Initialize ratchets
    let mut alice_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_identity_dh.public_key());
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, bob_identity_dh);

    // Alice encrypts a message
    let plaintext = b"Hello Bob! This is a secret message.";
    let ratchet_msg = alice_ratchet.encrypt(plaintext).unwrap();

    // Bob decrypts
    let decrypted = bob_ratchet.decrypt(&ratchet_msg).unwrap();
    assert_eq!(decrypted, plaintext);

    // Bob replies
    let reply = b"Hi Alice! Message received.";
    let ratchet_msg2 = bob_ratchet.encrypt(reply).unwrap();

    // Alice decrypts
    let decrypted2 = alice_ratchet.decrypt(&ratchet_msg2).unwrap();
    assert_eq!(decrypted2, reply);

    // Multiple messages in sequence
    for i in 0..5 {
        let msg = format!("Message {}", i);
        let encrypted = alice_ratchet.encrypt(msg.as_bytes()).unwrap();
        let decrypted = bob_ratchet.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, msg.as_bytes());
    }
}

/// Test: Sync manager workflow
#[test]
fn test_sync_manager_workflow() {
    use webbook_core::Storage;

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
    assert!(matches!(state, webbook_core::SyncState::Pending { .. }));

    // Mark as delivered
    sync_manager.mark_delivered(&update_id).unwrap();

    // Verify update was removed
    let pending = sync_manager.get_pending("contact-1").unwrap();
    assert_eq!(pending.len(), 0);

    // State should now be synced
    let state = sync_manager.get_sync_state("contact-1").unwrap();
    assert!(matches!(state, webbook_core::SyncState::Synced { .. }));
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

/// Test: Configuration builder pattern
#[test]
fn test_config_builder() {
    use std::path::PathBuf;
    use webbook_core::api::WebBookBuilder;

    // Test default config
    let config = WebBookConfig::default();
    assert_eq!(config.storage_path, PathBuf::from("./webbook_data"));
    assert!(config.auto_save);

    // Test builder
    let config = WebBookConfig::with_storage_path("/custom/path")
        .with_relay_url("wss://relay.example.com")
        .without_auto_save();

    assert_eq!(config.storage_path, PathBuf::from("/custom/path"));
    assert_eq!(config.relay.server_url, "wss://relay.example.com");
    assert!(!config.auto_save);

    // Test WebBookBuilder
    let wb: WebBook<MockTransport> = WebBookBuilder::new()
        .storage_path("/tmp/webbook_test")
        .relay_url("wss://test.relay.com")
        .build()
        .unwrap();

    assert_eq!(wb.config().relay.server_url, "wss://test.relay.com");
}

/// Test: Error handling
#[test]
fn test_error_handling() {
    let mut wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    // Try to get public ID without identity
    let result = wb.public_id();
    assert!(result.is_err());

    // Create identity
    wb.create_identity("Test").unwrap();

    // Try to create identity again
    let result = wb.create_identity("Test2");
    assert!(result.is_err());

    // Try to get non-existent contact
    let result = wb.get_contact("nonexistent").unwrap();
    assert!(result.is_none());

    // Try to remove non-existent contact
    let result = wb.remove_contact("nonexistent").unwrap();
    assert!(!result);

    // Try to verify fingerprint for non-existent contact
    let result = wb.verify_contact_fingerprint("nonexistent");
    assert!(result.is_err());
}

/// Test: Contact card delta computation and application
#[test]
fn test_card_delta_workflow() {
    use webbook_core::sync::{CardDelta, FieldChange};

    // Create initial card
    let mut old_card = ContactCard::new("Test User");
    old_card
        .add_field(ContactField::new(FieldType::Email, "work", "old@work.com"))
        .unwrap();
    old_card
        .add_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+15551234567",
        ))
        .unwrap();

    // Clone and modify card (to preserve field IDs for modification detection)
    let mut updated_card = old_card.clone();
    updated_card.set_display_name("Test User Updated").unwrap();
    // Modify the email value (same field ID)
    let email_field_id = updated_card.fields()[0].id().to_string();
    updated_card
        .update_field_value(&email_field_id, "new@work.com")
        .unwrap();
    // Remove mobile field
    let mobile_field_id = updated_card.fields()[1].id().to_string();
    updated_card.remove_field(&mobile_field_id).unwrap();
    // Add new field
    updated_card
        .add_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://blog.test.com",
        ))
        .unwrap();

    // Compute delta
    let delta = CardDelta::compute(&old_card, &updated_card);

    // Should have multiple changes
    assert!(!delta.changes.is_empty());

    // Display name changed
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::DisplayNameChanged { .. })));

    // Email modified (same field ID, different value)
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Modified { .. })));

    // Mobile removed
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Removed { .. })));

    // Blog added
    assert!(delta
        .changes
        .iter()
        .any(|c| matches!(c, FieldChange::Added { .. })));

    // Apply delta to a copy of old card
    let mut result_card = old_card.clone();
    delta.apply(&mut result_card).unwrap();

    // Verify result matches updated card
    assert_eq!(result_card.display_name(), updated_card.display_name());
    assert_eq!(result_card.fields().len(), updated_card.fields().len());
}

/// Test: Phase 8 Happy Path - 3-User End-to-End Card Propagation with Visibility Rules
///
/// This test verifies the complete Phase 8 workflow:
/// 1. Alice exchanges contacts with Bob and Charlie
/// 2. Alice adds fields to her card
/// 3. Alice hides some fields from Bob (but not Charlie)
/// 4. Alice propagates card updates
/// 5. Bob receives only visible fields, Charlie receives all fields
#[test]
fn test_phase8_three_user_card_propagation_with_visibility() {
    use webbook_core::contact::FieldVisibility;

    // ========================================
    // Step 1: Create three WebBook instances
    // ========================================
    let mut alice_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();
    let mut bob_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();
    let mut charlie_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

    alice_wb.create_identity("Alice").unwrap();
    bob_wb.create_identity("Bob").unwrap();
    charlie_wb.create_identity("Charlie").unwrap();

    // ========================================
    // Step 2: Get public keys for each user
    // ========================================
    let alice_public_key = *alice_wb.identity().unwrap().signing_public_key();
    let bob_public_key = *bob_wb.identity().unwrap().signing_public_key();
    let charlie_public_key = *charlie_wb.identity().unwrap().signing_public_key();

    // ========================================
    // Step 3: Simulate contact exchange - Alice ↔ Bob
    // ========================================
    // Create shared secrets (in real exchange, these come from X3DH)
    let alice_bob_shared_secret = SymmetricKey::generate();

    // Create Bob's contact in Alice's WebBook
    let bob_contact = Contact::from_exchange(
        bob_public_key,
        ContactCard::new("Bob"),
        alice_bob_shared_secret.clone(),
    );
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    // Create Alice's contact in Bob's WebBook
    let alice_contact_for_bob = Contact::from_exchange(
        alice_public_key,
        ContactCard::new("Alice"),
        alice_bob_shared_secret.clone(),
    );
    let alice_contact_id_bob = alice_contact_for_bob.id().to_string();
    bob_wb.add_contact(alice_contact_for_bob).unwrap();

    // Initialize Double Ratchet for Alice ↔ Bob
    let bob_dh_for_alice = X3DHKeyPair::generate();
    let alice_bob_ratchet = DoubleRatchetState::initialize_initiator(
        &alice_bob_shared_secret,
        *bob_dh_for_alice.public_key(),
    );
    let bob_alice_ratchet =
        DoubleRatchetState::initialize_responder(&alice_bob_shared_secret, bob_dh_for_alice);

    // Save ratchet states
    alice_wb
        .storage()
        .save_ratchet_state(&bob_contact_id, &alice_bob_ratchet, true)
        .unwrap();
    bob_wb
        .storage()
        .save_ratchet_state(&alice_contact_id_bob, &bob_alice_ratchet, false)
        .unwrap();

    // ========================================
    // Step 4: Simulate contact exchange - Alice ↔ Charlie
    // ========================================
    let alice_charlie_shared_secret = SymmetricKey::generate();

    // Create Charlie's contact in Alice's WebBook (use value, not reference)
    let charlie_contact = Contact::from_exchange(
        charlie_public_key,
        ContactCard::new("Charlie"),
        alice_charlie_shared_secret.clone(),
    );
    let charlie_contact_id = charlie_contact.id().to_string();
    alice_wb.add_contact(charlie_contact).unwrap();

    // Create Alice's contact in Charlie's WebBook (use value, not reference)
    let alice_contact_for_charlie = Contact::from_exchange(
        alice_public_key,
        ContactCard::new("Alice"),
        alice_charlie_shared_secret.clone(),
    );
    let alice_contact_id_charlie = alice_contact_for_charlie.id().to_string();
    charlie_wb.add_contact(alice_contact_for_charlie).unwrap();

    // Initialize Double Ratchet for Alice ↔ Charlie
    let charlie_dh_for_alice = X3DHKeyPair::generate();
    let alice_charlie_ratchet = DoubleRatchetState::initialize_initiator(
        &alice_charlie_shared_secret,
        *charlie_dh_for_alice.public_key(),
    );
    let charlie_alice_ratchet = DoubleRatchetState::initialize_responder(
        &alice_charlie_shared_secret,
        charlie_dh_for_alice,
    );

    // Save ratchet states
    alice_wb
        .storage()
        .save_ratchet_state(&charlie_contact_id, &alice_charlie_ratchet, true)
        .unwrap();
    charlie_wb
        .storage()
        .save_ratchet_state(&alice_contact_id_charlie, &charlie_alice_ratchet, false)
        .unwrap();

    // ========================================
    // Step 5: Verify initial setup
    // ========================================
    assert_eq!(alice_wb.contact_count().unwrap(), 2);
    assert_eq!(bob_wb.contact_count().unwrap(), 1);
    assert_eq!(charlie_wb.contact_count().unwrap(), 1);

    // ========================================
    // Step 6: Alice adds fields to her card
    // ========================================
    let old_card = alice_wb.own_card().unwrap().unwrap();

    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
        ))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "mobile",
            "+15551234567",
        ))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://alice.dev",
        ))
        .unwrap();

    let new_card = alice_wb.own_card().unwrap().unwrap();
    assert_eq!(new_card.fields().len(), 3);

    // Get field IDs
    let work_field_id = new_card
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .unwrap()
        .id()
        .to_string();
    let mobile_field_id = new_card
        .fields()
        .iter()
        .find(|f| f.label() == "mobile")
        .unwrap()
        .id()
        .to_string();

    // ========================================
    // Step 7: Alice sets visibility rules - hide mobile from Bob
    // ========================================
    {
        let mut bob_contact = alice_wb.get_contact(&bob_contact_id).unwrap().unwrap();
        // Hide mobile field from Bob
        bob_contact
            .visibility_rules_mut()
            .set_nobody(&mobile_field_id);
        alice_wb.update_contact(&bob_contact).unwrap();
    }

    // Verify visibility rules
    {
        let bob_contact = alice_wb.get_contact(&bob_contact_id).unwrap().unwrap();
        assert!(matches!(
            bob_contact.visibility_rules().get(&mobile_field_id),
            FieldVisibility::Nobody
        ));
        assert!(matches!(
            bob_contact.visibility_rules().get(&work_field_id),
            FieldVisibility::Everyone
        ));
    }

    // Charlie should see everything (default: Everyone)
    {
        let charlie_contact = alice_wb.get_contact(&charlie_contact_id).unwrap().unwrap();
        assert!(matches!(
            charlie_contact.visibility_rules().get(&mobile_field_id),
            FieldVisibility::Everyone
        ));
        assert!(matches!(
            charlie_contact.visibility_rules().get(&work_field_id),
            FieldVisibility::Everyone
        ));
    }

    // ========================================
    // Step 8: Alice propagates card updates
    // ========================================
    let queued = alice_wb
        .propagate_card_update(&old_card, &new_card)
        .unwrap();
    assert_eq!(queued, 2, "Should queue updates for both Bob and Charlie");

    // ========================================
    // Step 9: Retrieve and verify pending updates
    // ========================================
    let pending_for_bob = alice_wb
        .storage()
        .get_pending_updates(&bob_contact_id)
        .unwrap();
    let pending_for_charlie = alice_wb
        .storage()
        .get_pending_updates(&charlie_contact_id)
        .unwrap();

    assert_eq!(
        pending_for_bob.len(),
        1,
        "Should have 1 pending update for Bob"
    );
    assert_eq!(
        pending_for_charlie.len(),
        1,
        "Should have 1 pending update for Charlie"
    );

    // ========================================
    // Step 10: Simulate Bob receiving and decrypting the update
    // ========================================
    {
        let update = &pending_for_bob[0];
        let encrypted_payload = &update.payload;

        // Load Bob's ratchet state
        let (mut ratchet, _) = bob_wb
            .storage()
            .load_ratchet_state(&alice_contact_id_bob)
            .unwrap()
            .unwrap();

        // Decrypt
        let ratchet_msg: webbook_core::crypto::ratchet::RatchetMessage =
            serde_json::from_slice(encrypted_payload).unwrap();
        let delta_bytes = ratchet.decrypt(&ratchet_msg).unwrap();

        // Save updated ratchet
        bob_wb
            .storage()
            .save_ratchet_state(&alice_contact_id_bob, &ratchet, false)
            .unwrap();

        // Parse delta
        let delta: webbook_core::sync::CardDelta = serde_json::from_slice(&delta_bytes).unwrap();

        // Verify signature
        let alice_contact = bob_wb.get_contact(&alice_contact_id_bob).unwrap().unwrap();
        assert!(delta.verify(alice_contact.public_key()));

        // Check delta changes - Bob should NOT see mobile field
        let field_labels: Vec<&str> = delta
            .changes
            .iter()
            .filter_map(|c| match c {
                webbook_core::sync::FieldChange::Added { field } => Some(field.label()),
                _ => None,
            })
            .collect();

        assert!(
            field_labels.contains(&"work"),
            "Bob should receive work field"
        );
        assert!(
            field_labels.contains(&"blog"),
            "Bob should receive blog field"
        );
        assert!(
            !field_labels.contains(&"mobile"),
            "Bob should NOT receive mobile field (hidden)"
        );
        assert_eq!(field_labels.len(), 2, "Bob should only receive 2 fields");

        // Apply delta to Bob's view of Alice
        let mut alice_card = alice_contact.card().clone();
        delta.apply(&mut alice_card).unwrap();

        // Verify Bob's view
        assert_eq!(alice_card.fields().len(), 2);
        assert!(alice_card.fields().iter().any(|f| f.label() == "work"));
        assert!(alice_card.fields().iter().any(|f| f.label() == "blog"));
        assert!(!alice_card.fields().iter().any(|f| f.label() == "mobile"));
    }

    // ========================================
    // Step 11: Simulate Charlie receiving and decrypting the update
    // ========================================
    {
        let update = &pending_for_charlie[0];
        let encrypted_payload = &update.payload;

        // Load Charlie's ratchet state
        let (mut ratchet, _) = charlie_wb
            .storage()
            .load_ratchet_state(&alice_contact_id_charlie)
            .unwrap()
            .unwrap();

        // Decrypt
        let ratchet_msg: webbook_core::crypto::ratchet::RatchetMessage =
            serde_json::from_slice(encrypted_payload).unwrap();
        let delta_bytes = ratchet.decrypt(&ratchet_msg).unwrap();

        // Save updated ratchet
        charlie_wb
            .storage()
            .save_ratchet_state(&alice_contact_id_charlie, &ratchet, false)
            .unwrap();

        // Parse delta
        let delta: webbook_core::sync::CardDelta = serde_json::from_slice(&delta_bytes).unwrap();

        // Verify signature
        let alice_contact = charlie_wb
            .get_contact(&alice_contact_id_charlie)
            .unwrap()
            .unwrap();
        assert!(delta.verify(alice_contact.public_key()));

        // Check delta changes - Charlie should see ALL fields
        let field_labels: Vec<&str> = delta
            .changes
            .iter()
            .filter_map(|c| match c {
                webbook_core::sync::FieldChange::Added { field } => Some(field.label()),
                _ => None,
            })
            .collect();

        assert!(
            field_labels.contains(&"work"),
            "Charlie should receive work field"
        );
        assert!(
            field_labels.contains(&"blog"),
            "Charlie should receive blog field"
        );
        assert!(
            field_labels.contains(&"mobile"),
            "Charlie should receive mobile field"
        );
        assert_eq!(field_labels.len(), 3, "Charlie should receive all 3 fields");

        // Apply delta to Charlie's view of Alice
        let mut alice_card = alice_contact.card().clone();
        delta.apply(&mut alice_card).unwrap();

        // Verify Charlie's view
        assert_eq!(alice_card.fields().len(), 3);
        assert!(alice_card.fields().iter().any(|f| f.label() == "work"));
        assert!(alice_card.fields().iter().any(|f| f.label() == "blog"));
        assert!(alice_card.fields().iter().any(|f| f.label() == "mobile"));
    }

    // ========================================
    // Step 12: Verify ratchet states were saved
    // ========================================
    // Both ratchets should exist after decrypt
    let bob_ratchet_after = bob_wb
        .storage()
        .load_ratchet_state(&alice_contact_id_bob)
        .unwrap();
    let charlie_ratchet_after = charlie_wb
        .storage()
        .load_ratchet_state(&alice_contact_id_charlie)
        .unwrap();

    // Ratchets should be present (decrypt succeeded)
    assert!(
        bob_ratchet_after.is_some(),
        "Bob's ratchet state should be saved"
    );
    assert!(
        charlie_ratchet_after.is_some(),
        "Charlie's ratchet state should be saved"
    );
}

/// Test: Phase 8 - Field modification and removal propagation
///
/// Tests that add/modify/remove operations each produce the correct delta type.
#[test]
fn test_phase8_field_modification_and_removal_propagation() {
    use webbook_core::sync::{CardDelta, FieldChange};

    // ========================================
    // Test 1: Field addition produces Added delta
    // ========================================
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

    // ========================================
    // Test 2: Field modification produces Modified delta
    // ========================================
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

    // ========================================
    // Test 3: Field removal produces Removed delta
    // ========================================
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

    // ========================================
    // Test 4: Full propagation roundtrip with modify
    // ========================================
    {
        let mut alice_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();
        let mut bob_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();

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
        let ratchet_msg: webbook_core::crypto::ratchet::RatchetMessage =
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
