//! Concurrency Tests for SQLite Storage
//!
//! These tests verify thread safety and concurrent access patterns for the
//! Storage module. SQLite connections themselves aren't Sync, but we test:
//! 1. Sequential operations remain consistent
//! 2. Multiple connections to the same file work correctly
//! 3. Read-after-write consistency
//! 4. WAL mode concurrent access (if enabled)

use rand::Rng;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;
use webbook_core::contact::Contact;
use webbook_core::crypto::SymmetricKey;
use webbook_core::storage::Storage;
use webbook_core::{ContactCard, ContactField, FieldType};

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

fn create_test_contact(name: &str) -> Contact {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        &format!("{}@example.com", name.to_lowercase().replace(' ', ".")),
    ))
    .unwrap();

    // Generate a random public key so each contact has a unique ID
    let mut public_key = [0u8; 32];
    rand::thread_rng().fill(&mut public_key);

    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

// =============================================================================
// SEQUENTIAL OPERATION TESTS
// =============================================================================

#[test]
fn test_sequential_contact_operations() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Perform many sequential operations
    let mut contact_ids = Vec::new();

    for i in 0..100 {
        let contact = create_test_contact(&format!("User {}", i));
        let id = contact.id().to_string();
        storage.save_contact(&contact).unwrap();
        contact_ids.push(id);
    }

    // Verify all contacts exist
    let contacts = storage.list_contacts().unwrap();
    assert_eq!(contacts.len(), 100);

    // Verify each can be loaded individually
    for id in &contact_ids {
        let loaded = storage.load_contact(id).unwrap();
        assert!(loaded.is_some());
    }

    // Delete half and verify
    for id in contact_ids.iter().take(50) {
        storage.delete_contact(id).unwrap();
    }

    let remaining = storage.list_contacts().unwrap();
    assert_eq!(remaining.len(), 50);
}

#[test]
fn test_sequential_pending_update_operations() {
use webbook_core::storage::{PendingUpdate, UpdateStatus};

    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Queue many updates
    for i in 0..50 {
        let update = PendingUpdate {
            id: format!("update-{}", i),
            contact_id: format!("contact-{}", i % 10),
            update_type: "card_delta".to_string(),
            payload: vec![i as u8; 100],
            created_at: 1700000000 + i as u64,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };
        storage.queue_update(&update).unwrap();
    }

    // Verify all queued
    let updates = storage.get_all_pending_updates().unwrap();
    assert_eq!(updates.len(), 50);

    // Mark some as sent (delete)
    for i in 0..25 {
        storage.mark_update_sent(&format!("update-{}", i)).unwrap();
    }

    let remaining = storage.get_all_pending_updates().unwrap();
    assert_eq!(remaining.len(), 25);
}

// =============================================================================
// FILE-BASED CONCURRENT ACCESS TESTS
// =============================================================================

#[test]
fn test_multiple_connections_same_file() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("concurrent.db");

    let key = SymmetricKey::generate();

    // Create first connection and write data
    {
        let storage1 = Storage::open(&db_path, key.clone()).unwrap();
        let contact = create_test_contact("Alice");
        storage1.save_contact(&contact).unwrap();
    }

    // Create second connection and read data
    {
        let storage2 = Storage::open(&db_path, key.clone()).unwrap();
        let contacts = storage2.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].card().display_name(), "Alice");
    }

    // Create third connection and add more
    {
        let storage3 = Storage::open(&db_path, key.clone()).unwrap();
        let contact = create_test_contact("Bob");
        storage3.save_contact(&contact).unwrap();
    }

    // Verify with fourth connection
    {
        let storage4 = Storage::open(&db_path, key).unwrap();
        let contacts = storage4.list_contacts().unwrap();
        assert_eq!(contacts.len(), 2);
    }
}

#[test]
fn test_concurrent_readers_file_based() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("readers.db");

    let key = SymmetricKey::generate();

    // Setup: create some data
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        for i in 0..10 {
            let contact = create_test_contact(&format!("Contact {}", i));
            storage.save_contact(&contact).unwrap();
        }
    }

    // Spawn multiple reader threads
    let path = db_path.clone();
    let barrier = Arc::new(Barrier::new(5));
    let mut handles = Vec::new();

    for thread_id in 0..5 {
        let thread_path = path.clone();
        let thread_key = key.clone();
        let thread_barrier = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            // Wait for all threads to be ready
            thread_barrier.wait();

            // Open connection and read
            let storage = Storage::open(&thread_path, thread_key).unwrap();
            let contacts = storage.list_contacts().unwrap();

            // Verify we read correct data
            assert_eq!(contacts.len(), 10, "Thread {} saw wrong count", thread_id);

            // Read each contact
            for contact in &contacts {
                let loaded = storage.load_contact(contact.id()).unwrap();
                assert!(loaded.is_some());
            }

            thread_id
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        let thread_id = handle.join().expect("Thread panicked");
        assert!(thread_id < 5);
    }
}

#[test]
fn test_sequential_writers_file_based() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("writers.db");

    let key = SymmetricKey::generate();

    // Initialize database
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let _ = storage.list_contacts().unwrap(); // Just init
    }

    // Sequential writers from different threads
    let path = db_path.clone();
    let mut handles = Vec::new();

    for thread_id in 0..5 {
        let thread_path = path.clone();
        let thread_key = key.clone();

        let handle = thread::spawn(move || {
            // Each thread opens connection and writes
            let storage = Storage::open(&thread_path, thread_key).unwrap();

            for i in 0..10 {
                let contact = create_test_contact(&format!("Thread{}Contact{}", thread_id, i));
                storage.save_contact(&contact).unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Verify all data was written
    let storage = Storage::open(&db_path, key).unwrap();
    let contacts = storage.list_contacts().unwrap();

    // Should have 5 threads * 10 contacts = 50 total
    assert_eq!(contacts.len(), 50);
}

// =============================================================================
// READ-AFTER-WRITE CONSISTENCY TESTS
// =============================================================================

#[test]
fn test_read_after_write_consistency() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("consistency.db");

    let key = SymmetricKey::generate();

    // Write from one connection
    let contact = create_test_contact("Consistency Test");
    let contact_id = contact.id().to_string();
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        storage.save_contact(&contact).unwrap();
    }

    // Immediately read from new connection
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let loaded = storage.load_contact(&contact_id).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().card().display_name(), "Consistency Test");
    }
}

#[test]
fn test_update_visibility_consistency() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("visibility.db");

    let key = SymmetricKey::generate();

    // Create contact
    let contact = create_test_contact("Visibility Test");
    let contact_id = contact.id().to_string();

    // Save and update from different connections
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        storage.save_contact(&contact).unwrap();
    }

    // Update own card from another connection
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let card = ContactCard::new("Updated Name");
        storage.save_own_card(&card).unwrap();
    }

    // Verify both changes persisted
    {
        let storage = Storage::open(&db_path, key).unwrap();

        // Contact should exist
        let loaded = storage.load_contact(&contact_id).unwrap();
        assert!(loaded.is_some());

        // Own card should be updated
        let own_card = storage.load_own_card().unwrap();
        assert!(own_card.is_some());
        assert_eq!(own_card.unwrap().display_name(), "Updated Name");
    }
}

// =============================================================================
// STRESS TESTS
// =============================================================================

#[test]
fn test_rapid_open_close_cycles() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("rapid.db");

    let key = SymmetricKey::generate();

    // Initialize
    {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let contact = create_test_contact("Initial");
        storage.save_contact(&contact).unwrap();
    }

    // Rapid open/read/close cycles
    for i in 0..50 {
        let storage = Storage::open(&db_path, key.clone()).unwrap();
        let contacts = storage.list_contacts().unwrap();
        assert!(!contacts.is_empty(), "Iteration {} found no contacts", i);
    }
}

#[test]
fn test_interleaved_reads_writes() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("interleaved.db");

    let key = SymmetricKey::generate();

    // Initialize
    {
        let _ = Storage::open(&db_path, key.clone()).unwrap();
    }

    // Interleaved read-write operations
    for i in 0..20 {
        // Write
        {
            let storage = Storage::open(&db_path, key.clone()).unwrap();
            let contact = create_test_contact(&format!("Contact {}", i));
            storage.save_contact(&contact).unwrap();
        }

        // Read and verify count
        {
            let storage = Storage::open(&db_path, key.clone()).unwrap();
            let contacts = storage.list_contacts().unwrap();
            assert_eq!(contacts.len(), i + 1, "Wrong count after iteration {}", i);
        }
    }
}

// =============================================================================
// ERROR HANDLING UNDER CONCURRENT ACCESS
// =============================================================================

#[test]
fn test_delete_nonexistent_is_idempotent() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Deleting non-existent contact should not error
    let result = storage.delete_contact("does-not-exist");
    assert!(result.is_ok());

    // Deleting same non-existent ID multiple times should all succeed
    for _ in 0..10 {
        let result = storage.delete_contact("still-does-not-exist");
        assert!(result.is_ok());
    }
}

#[test]
fn test_double_save_overwrites() {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    // Save contact
    let mut card = ContactCard::new("Original Name");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "original@example.com",
    ))
    .unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([1u8; 32], card, shared_key);
    let id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    // Load and verify
    let loaded = storage.load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded.card().display_name(), "Original Name");

    // Create new contact with same public key to get same ID
    let mut card2 = ContactCard::new("Updated Name");
    card2
        .add_field(ContactField::new(
            FieldType::Email,
            "Work",
            "updated@example.com",
        ))
        .unwrap();
    let shared_key2 = SymmetricKey::generate();
    let contact2 = Contact::from_exchange([1u8; 32], card2, shared_key2);

    // Save should overwrite (upsert behavior)
    storage.save_contact(&contact2).unwrap();

    // Verify updated
    let loaded2 = storage.load_contact(&id).unwrap().unwrap();
    assert_eq!(loaded2.card().display_name(), "Updated Name");
}
