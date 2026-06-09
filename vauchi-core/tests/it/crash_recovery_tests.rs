// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform Edge Cases: Crash Recovery Tests
//!
//! Feature file: features/platform_edge_cases.feature @crash-recovery
//! Tests for atomic sync checkpoint persistence and crash resume.

use crate::common;

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::sync::SyncItem;

/// Helper: create test sync items for a batch.
fn create_batch_items(count: usize) -> Vec<SyncItem> {
    (0..count)
        .map(|i| SyncItem::CardUpdated {
            field_label: format!("field_{}", i),
            new_value: format!("value_{}", i),
            timestamp: 1000 + i as u64,
        })
        .collect()
}

// ============================================================
// Device Sync Checkpoint Tests (V6 table)
// ============================================================

// @scenario: platform_edge_cases :: Sync state persisted atomically
// @internal
#[test]
fn test_checkpoint_save_and_load() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let target_device = [0xAAu8; 32];
    let items = create_batch_items(50);

    storage
        .sync()
        .save_sync_checkpoint(&target_device, &items, 25)
        .unwrap();

    let loaded = storage.sync().load_sync_checkpoint(&target_device).unwrap();
    assert!(loaded.is_some(), "Checkpoint should be loaded");

    let (loaded_items, sent_count) = loaded.unwrap();
    assert_eq!(sent_count, 25, "Sent count should be preserved");
    assert_eq!(loaded_items.len(), 50, "All items should be preserved");
}

// @scenario: sync_updates :: Sync survives device reboot
// @internal
#[test]
fn test_checkpoint_resume_from_correct_position() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let target_device = [0xBBu8; 32];
    let items = create_batch_items(50);

    // Simulate: save checkpoint at 25, "crash", resume
    storage
        .sync()
        .save_sync_checkpoint(&target_device, &items, 25)
        .unwrap();

    // Simulate resume: load checkpoint and continue from sent_count
    let (loaded_items, sent_count) = storage
        .sync()
        .load_sync_checkpoint(&target_device)
        .unwrap()
        .unwrap();

    let remaining = &loaded_items[sent_count..];
    assert_eq!(remaining.len(), 25, "Should resume with 25 remaining items");

    // Verify the resumed items are correct (items 25..50)
    for (i, item) in remaining.iter().enumerate() {
        match item {
            SyncItem::CardUpdated { field_label, .. } => {
                assert_eq!(*field_label, format!("field_{}", 25 + i));
            }
            _ => panic!("Expected CardUpdated item"),
        }
    }
}

// @internal
#[test]
fn test_checkpoint_update_progress() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let target_device = [0xCCu8; 32];
    let items = create_batch_items(50);

    storage
        .sync()
        .save_sync_checkpoint(&target_device, &items, 10)
        .unwrap();

    // Update to 30 (simulating continued progress)
    storage
        .sync()
        .save_sync_checkpoint(&target_device, &items, 30)
        .unwrap();

    let (_, sent_count) = storage
        .sync()
        .load_sync_checkpoint(&target_device)
        .unwrap()
        .unwrap();
    assert_eq!(sent_count, 30, "Checkpoint should reflect updated progress");
}

// @internal
#[test]
fn test_checkpoint_clear_after_completion() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let target_device = [0xDDu8; 32];
    let items = create_batch_items(50);

    storage
        .sync()
        .save_sync_checkpoint(&target_device, &items, 50)
        .unwrap();

    storage
        .sync()
        .clear_sync_checkpoint(&target_device)
        .unwrap();

    let loaded = storage.sync().load_sync_checkpoint(&target_device).unwrap();
    assert!(loaded.is_none(), "Checkpoint should be cleared");
}

// @internal
#[test]
fn test_no_checkpoint_returns_none() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let target_device = [0xEEu8; 32];

    let loaded = storage.sync().load_sync_checkpoint(&target_device).unwrap();
    assert!(
        loaded.is_none(),
        "Non-existent checkpoint should return None"
    );
}

// @internal
#[test]
fn test_multiple_device_checkpoints_independent() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let device_a = [0x01u8; 32];
    let device_b = [0x02u8; 32];

    let items_a = create_batch_items(30);
    let items_b = create_batch_items(40);

    storage
        .sync()
        .save_sync_checkpoint(&device_a, &items_a, 15)
        .unwrap();
    storage
        .sync()
        .save_sync_checkpoint(&device_b, &items_b, 20)
        .unwrap();

    let (loaded_a, count_a) = storage
        .sync()
        .load_sync_checkpoint(&device_a)
        .unwrap()
        .unwrap();
    let (loaded_b, count_b) = storage
        .sync()
        .load_sync_checkpoint(&device_b)
        .unwrap()
        .unwrap();

    assert_eq!(loaded_a.len(), 30);
    assert_eq!(count_a, 15);
    assert_eq!(loaded_b.len(), 40);
    assert_eq!(count_b, 20);

    // Clearing one doesn't affect the other
    storage.sync().clear_sync_checkpoint(&device_a).unwrap();
    assert!(
        storage
            .sync()
            .load_sync_checkpoint(&device_a)
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .sync()
            .load_sync_checkpoint(&device_b)
            .unwrap()
            .is_some(),
        "expected Some value"
    );
}

// ============================================================
// Batch Checkpoint Tests (V12 table)
// ============================================================

// @internal
#[test]
fn test_batch_checkpoint_save_and_load() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let batch_id = "batch-001";

    storage
        .sync()
        .save_batch_checkpoint(batch_id, 50, 25, "{\"state\":\"mid\"}")
        .unwrap();

    let loaded = storage.sync().load_batch_checkpoint(batch_id).unwrap();
    assert!(loaded.is_some(), "expected Some value");

    let (total, processed, state) = loaded.unwrap();
    assert_eq!(total, 50);
    assert_eq!(processed, 25);
    assert_eq!(state, "{\"state\":\"mid\"}");
}

// @internal
#[test]
fn test_batch_checkpoint_update_progress() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let batch_id = "batch-002";

    storage
        .sync()
        .save_batch_checkpoint(batch_id, 100, 10, "{\"step\":1}")
        .unwrap();

    storage
        .sync()
        .update_batch_checkpoint(batch_id, 50, "{\"step\":5}")
        .unwrap();

    let (total, processed, state) = storage
        .sync()
        .load_batch_checkpoint(batch_id)
        .unwrap()
        .unwrap();
    assert_eq!(total, 100, "Total should be unchanged");
    assert_eq!(processed, 50, "Processed should be updated");
    assert_eq!(state, "{\"step\":5}", "State should be updated");
}

// @internal
#[test]
fn test_batch_checkpoint_clear() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let batch_id = "batch-003";

    storage
        .sync()
        .save_batch_checkpoint(batch_id, 50, 50, "{\"done\":true}")
        .unwrap();

    storage.sync().clear_batch_checkpoint(batch_id).unwrap();

    let loaded = storage.sync().load_batch_checkpoint(batch_id).unwrap();
    assert!(loaded.is_none(), "Checkpoint should be cleared");
}

// @internal
#[test]
fn test_batch_checkpoint_crash_resume_scenario() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let batch_id = "batch-crash-test";
    let items = create_batch_items(50);

    // Step 1: Start batch, checkpoint at 25
    storage
        .sync()
        .save_batch_checkpoint(batch_id, items.len(), 25, "{\"last_item\":24}")
        .unwrap();

    // Step 2: Simulate crash (nothing else happens)

    // Step 3: Resume - load checkpoint
    let (total, processed, _state) = storage
        .sync()
        .load_batch_checkpoint(batch_id)
        .unwrap()
        .expect("Checkpoint should survive crash");

    assert_eq!(total, 50);
    assert_eq!(processed, 25);

    // Step 4: Process remaining items (25..50)
    let remaining = &items[processed..];
    assert_eq!(remaining.len(), 25);

    // Step 5: Update checkpoint to completion
    storage
        .sync()
        .update_batch_checkpoint(batch_id, 50, "{\"done\":true}")
        .unwrap();

    // Step 6: Clear checkpoint
    storage.sync().clear_batch_checkpoint(batch_id).unwrap();
    assert!(
        storage
            .sync()
            .load_batch_checkpoint(batch_id)
            .unwrap()
            .is_none()
    );
}

// @internal
#[test]
fn test_batch_checkpoint_no_orphans() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    storage
        .sync()
        .save_batch_checkpoint("b1", 50, 25, "{}")
        .unwrap();
    storage
        .sync()
        .save_batch_checkpoint("b2", 100, 50, "{}")
        .unwrap();

    storage.sync().clear_batch_checkpoint("b1").unwrap();

    assert!(
        storage
            .sync()
            .load_batch_checkpoint("b1")
            .unwrap()
            .is_none()
    );
    assert!(
        storage
            .sync()
            .load_batch_checkpoint("b2")
            .unwrap()
            .is_some(),
        "expected Some value"
    );
}
