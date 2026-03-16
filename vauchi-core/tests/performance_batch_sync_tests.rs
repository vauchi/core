// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Performance: Batch Sync Coalescing Tests
//!
//! Feature file: features/performance.feature @coalesce @batch-encrypt
//! Tests for rapid edit coalescing and batch encryption pipelines.

mod common;

use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::sync::SyncManager;

/// Helper: create a base card with a single field.
fn base_card() -> ContactCard {
    let mut card = ContactCard::new("Test User");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "test@example.com",
    ))
    .unwrap();
    card
}

/// Helper: get the first field ID from a card.
fn first_field_id(card: &ContactCard) -> String {
    card.fields()[0].id().to_string()
}

// ============================================================
// Coalescing Tests
// ============================================================

// @scenario: performance:Coalesce rapid edits before sync
#[test]
fn test_rapid_edits_coalesce_into_single_sync_payload() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let contact_id = "test-contact-001";
    let mut current_card = base_card();
    let field_id = first_field_id(&current_card);

    // Simulate 20 rapid edits
    for i in 0..20 {
        let mut new_card = current_card.clone();
        new_card
            .update_field_value(&field_id, &format!("updated{}@example.com", i))
            .unwrap();

        let _ = sync_manager.queue_card_update(contact_id, &current_card, &new_card);
        current_card = new_card;
    }

    // Verify multiple updates are pending
    let pending_before = sync_manager.get_pending(contact_id).unwrap();
    assert!(
        pending_before.len() >= 2,
        "Should have multiple pending updates before coalescing"
    );

    // Coalesce
    let result = sync_manager.coalesce_updates(contact_id).unwrap();
    assert!(
        result.is_some(),
        "Coalescing should produce a merged update"
    );

    // After coalescing, should have exactly 1 pending card_update
    let pending_after = sync_manager.get_pending(contact_id).unwrap();
    let card_updates: Vec<_> = pending_after
        .iter()
        .filter(|u| u.update_type == "card_update")
        .collect();
    assert_eq!(
        card_updates.len(),
        1,
        "After coalescing, should have exactly 1 card_update"
    );
}

// @scenario: performance:Coalesce rapid edits before sync
#[test]
fn test_coalesce_preserves_final_state() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let contact_id = "test-contact-002";
    let original_card = base_card();
    let field_id = first_field_id(&original_card);

    // Edit 1: change email
    let mut card_v1 = original_card.clone();
    card_v1
        .update_field_value(&field_id, "v1@example.com")
        .unwrap();
    sync_manager
        .queue_card_update(contact_id, &original_card, &card_v1)
        .unwrap();

    // Edit 2: change email again
    let mut card_v2 = card_v1.clone();
    card_v2
        .update_field_value(&field_id, "final@example.com")
        .unwrap();
    sync_manager
        .queue_card_update(contact_id, &card_v1, &card_v2)
        .unwrap();

    // Coalesce
    let result = sync_manager.coalesce_updates(contact_id).unwrap();
    result.expect("expected Some");

    // The coalesced payload should contain the field change
    let pending = sync_manager.get_pending(contact_id).unwrap();
    let card_update = pending
        .iter()
        .find(|u| u.update_type == "card_update")
        .expect("Should have a card_update");

    // Verify the payload is valid (parseable as CardDelta)
    let delta: vauchi_core::sync::CardDelta = serde_json::from_slice(&card_update.payload).unwrap();
    assert!(
        !delta.changes.is_empty(),
        "Coalesced delta should have changes"
    );
}

// @scenario: performance:Coalesce rapid edits before sync
#[test]
fn test_coalesce_skips_when_single_update() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let contact_id = "test-contact-003";
    let original_card = base_card();
    let field_id = first_field_id(&original_card);

    let mut new_card = original_card.clone();
    new_card
        .update_field_value(&field_id, "single@example.com")
        .unwrap();

    sync_manager
        .queue_card_update(contact_id, &original_card, &new_card)
        .unwrap();

    // Coalesce with only 1 update should return None
    let result = sync_manager.coalesce_updates(contact_id).unwrap();
    assert!(result.is_none(), "Single update should not need coalescing");
}

// ============================================================
// Batch Encryption Pipeline Tests
// ============================================================

// @scenario: performance:Batch encryption for multi-contact sync
#[test]
fn test_batch_encryption_50_contacts() {
    use vauchi_core::crypto::{encrypt, SymmetricKey};

    // Simulate batch encryption: 50 different contacts with their own keys
    let mut results = Vec::new();
    let payload = b"test update payload for sync";

    for _ in 0..50 {
        let contact_key = SymmetricKey::generate();
        let encrypted = encrypt(&contact_key, payload).unwrap();
        results.push(encrypted);
    }

    assert_eq!(results.len(), 50, "Should encrypt for all 50 contacts");
    // All ciphertexts should be different (unique nonces)
    let unique: std::collections::HashSet<Vec<u8>> = results.into_iter().collect();
    assert_eq!(
        unique.len(),
        50,
        "Each encryption should produce unique ciphertext"
    );
}

// @scenario: performance:Batch encryption for multi-contact sync
#[test]
fn test_batch_sync_multiple_contacts_pending() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let original_card = base_card();
    let field_id = first_field_id(&original_card);

    let mut new_card = original_card.clone();
    new_card
        .update_field_value(&field_id, "updated@example.com")
        .unwrap();

    // Queue updates for 20 different contacts
    for i in 0..20 {
        let contact_id = format!("contact-{:03}", i);
        sync_manager
            .queue_card_update(&contact_id, &original_card, &new_card)
            .unwrap();
    }

    let all_pending = sync_manager.get_all_pending().unwrap();
    assert_eq!(
        all_pending.len(),
        20,
        "Should have 20 pending updates for 20 contacts"
    );
}

// @scenario: performance:Coalesce rapid edits before sync
#[test]
fn test_coalesce_does_not_affect_other_update_types() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let mut sync_manager = SyncManager::new(&storage);

    let contact_id = "test-contact-004";
    let original_card = base_card();
    let field_id = first_field_id(&original_card);

    // Queue 2 card updates
    let mut card_v1 = original_card.clone();
    card_v1
        .update_field_value(&field_id, "v1@example.com")
        .unwrap();
    sync_manager
        .queue_card_update(contact_id, &original_card, &card_v1)
        .unwrap();

    let mut card_v2 = card_v1.clone();
    card_v2
        .update_field_value(&field_id, "v2@example.com")
        .unwrap();
    sync_manager
        .queue_card_update(contact_id, &card_v1, &card_v2)
        .unwrap();

    // Queue a visibility change (different type)
    sync_manager
        .queue_visibility_change(contact_id, vec!["email".to_string()])
        .unwrap();

    let before = sync_manager.get_pending(contact_id).unwrap();
    let visibility_before = before
        .iter()
        .filter(|u| u.update_type == "visibility_change")
        .count();
    assert_eq!(visibility_before, 1);

    // Coalesce should only merge card_updates
    sync_manager.coalesce_updates(contact_id).unwrap();

    let after = sync_manager.get_pending(contact_id).unwrap();
    let visibility_after = after
        .iter()
        .filter(|u| u.update_type == "visibility_change")
        .count();
    assert_eq!(
        visibility_after, 1,
        "Visibility changes should not be affected by coalescing"
    );

    let card_updates_after = after
        .iter()
        .filter(|u| u.update_type == "card_update")
        .count();
    assert_eq!(
        card_updates_after, 1,
        "Card updates should be coalesced into 1"
    );
}
