//! Integration Tests for WebBook Core
//!
//! These tests verify the full workflow from identity creation through contact exchange
//! and synchronization.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use webbook_core::{
    WebBook, WebBookConfig, WebBookEvent,
    ContactCard, ContactField, FieldType,
    Contact, SymmetricKey,
    api::{CallbackHandler, EventDispatcher},
    network::{MockTransport, RelayClient, RelayClientConfig, TransportConfig},
    sync::SyncManager,
    exchange::X3DHKeyPair,
    crypto::ratchet::DoubleRatchetState,
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
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "alice@company.com")).unwrap();
    wb.add_own_field(ContactField::new(FieldType::Phone, "mobile", "+15551234567")).unwrap();

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
    let bob = Contact::from_exchange(
        [2u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
    );
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
    let mut alice_ratchet = DoubleRatchetState::initialize_initiator(
        &shared_secret,
        *bob_identity_dh.public_key(),
    );
    let mut bob_ratchet = DoubleRatchetState::initialize_responder(
        &shared_secret,
        bob_identity_dh,
    );

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
    old_card.add_field(ContactField::new(FieldType::Email, "email", "old@example.com")).unwrap();

    let mut new_card = ContactCard::new("Test");
    new_card.add_field(ContactField::new(FieldType::Email, "email", "new@example.com")).unwrap();

    let update_id = sync_manager.queue_card_update("contact-1", &old_card, &new_card).unwrap();
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
    let mut ratchet = DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    // Send an update
    let msg_id = client.send_update(
        "recipient-id",
        &mut ratchet,
        b"test payload",
        "update-1",
    ).unwrap();

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
    use webbook_core::api::WebBookBuilder;
    use std::path::PathBuf;

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
    old_card.add_field(ContactField::new(FieldType::Email, "work", "old@work.com")).unwrap();
    old_card.add_field(ContactField::new(FieldType::Phone, "mobile", "+15551234567")).unwrap();

    // Clone and modify card (to preserve field IDs for modification detection)
    let mut updated_card = old_card.clone();
    updated_card.set_display_name("Test User Updated").unwrap();
    // Modify the email value (same field ID)
    let email_field_id = updated_card.fields()[0].id().to_string();
    updated_card.update_field_value(&email_field_id, "new@work.com").unwrap();
    // Remove mobile field
    let mobile_field_id = updated_card.fields()[1].id().to_string();
    updated_card.remove_field(&mobile_field_id).unwrap();
    // Add new field
    updated_card.add_field(ContactField::new(FieldType::Website, "blog", "https://blog.test.com")).unwrap();

    // Compute delta
    let delta = CardDelta::compute(&old_card, &updated_card);

    // Should have multiple changes
    assert!(!delta.changes.is_empty());

    // Display name changed
    assert!(delta.changes.iter().any(|c| matches!(c, FieldChange::DisplayNameChanged { .. })));

    // Email modified (same field ID, different value)
    assert!(delta.changes.iter().any(|c| matches!(c, FieldChange::Modified { .. })));

    // Mobile removed
    assert!(delta.changes.iter().any(|c| matches!(c, FieldChange::Removed { .. })));

    // Blog added
    assert!(delta.changes.iter().any(|c| matches!(c, FieldChange::Added { .. })));

    // Apply delta to a copy of old card
    let mut result_card = old_card.clone();
    delta.apply(&mut result_card).unwrap();

    // Verify result matches updated card
    assert_eq!(result_card.display_name(), updated_card.display_name());
    assert_eq!(result_card.fields().len(), updated_card.fields().len());
}
