// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR: Deletion Grace Period Tests
//!
//! Feature file: features/privacy_compliance.feature @deletion
//! Tests for scheduled deletion with 7-day grace period.

mod common;

use vauchi_core::api::DeletionManager;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::storage::DeletionState;
use vauchi_core::storage::Storage;

// ============================================================
// Deletion Grace Period Tests
// ============================================================

// @scenario: privacy_compliance.feature:Grace period before permanent deletion
#[test]
fn test_schedule_deletion_sets_grace_period() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let manager = DeletionManager::new(&storage);

    let result = manager.schedule_deletion();
    assert!(result.is_ok(), "Scheduling deletion should succeed");

    let state = manager.deletion_state().unwrap();
    match state {
        DeletionState::Scheduled {
            scheduled_at,
            execute_at,
        } => {
            // Grace period should be ~7 days (604800 seconds)
            let grace_period = execute_at - scheduled_at;
            assert_eq!(grace_period, 604800, "Grace period should be 7 days");
        }
        other => panic!("Expected Scheduled state, got {:?}", other),
    }
}

// @scenario: privacy_compliance.feature:Cancel deletion during grace period
// @scenario: emergency_shred.feature:Cancel soft shred during grace period
#[test]
fn test_cancel_deletion_within_grace_period() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let manager = DeletionManager::new(&storage);

    manager.schedule_deletion().unwrap();

    // Cancel should work within grace period
    let result = manager.cancel_deletion();
    assert!(
        result.is_ok(),
        "Canceling within grace period should succeed"
    );

    let state = manager.deletion_state().unwrap();
    assert!(
        matches!(state, DeletionState::None),
        "After cancel, state should be None"
    );
}

// @scenario: privacy_compliance.feature:Execute deletion requires grace period to have elapsed
#[test]
fn test_execute_deletion_fails_before_grace_period() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let manager = DeletionManager::new(&storage);

    manager.schedule_deletion().unwrap();

    // Execute should fail because grace period hasn't elapsed
    let identity = Identity::create("Test");
    let result = manager.execute_deletion(&identity);
    assert!(result.is_err(), "Execution before grace period should fail");
}

// @scenario: privacy_compliance.feature:Execute deletion after grace period sends revocations and purge
#[test]
fn test_execute_deletion_succeeds_after_grace_period() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let manager = DeletionManager::new(&storage);

    // Schedule with a past execute_at time (simulate elapsed grace period)
    manager
        .schedule_deletion_with_execute_at(1000, 999)
        .unwrap();

    // Now execute should succeed
    let identity = Identity::create("Test");
    let result = manager.execute_deletion(&identity);
    assert!(
        result.is_ok(),
        "Execution after grace period should succeed"
    );

    let state = manager.deletion_state().unwrap();
    assert!(
        matches!(state, DeletionState::Executed { .. }),
        "State should be Executed"
    );
}

// @scenario: privacy_compliance.feature:Grace period before permanent deletion
#[test]
fn test_deletion_state_persisted_across_manager_instances() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    {
        let manager = DeletionManager::new(&storage);
        manager.schedule_deletion().unwrap();
    }

    // New manager instance should see the scheduled state
    let manager2 = DeletionManager::new(&storage);
    let state = manager2.deletion_state().unwrap();
    assert!(
        matches!(state, DeletionState::Scheduled { .. }),
        "Deletion state should persist"
    );
}

// @scenario: privacy_compliance.feature:Cancel deletion during grace period
#[test]
fn test_cancel_deletion_when_not_scheduled() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let manager = DeletionManager::new(&storage);

    // Should be a no-op or return Ok
    let result = manager.cancel_deletion();
    assert!(result.is_ok(), "Cancel when not scheduled should be ok");
}

// @scenario: privacy_compliance.feature:Delete my account
#[test]
fn test_schedule_deletion_when_already_scheduled() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let manager = DeletionManager::new(&storage);

    manager.schedule_deletion().unwrap();

    // Scheduling again should be an error or no-op
    let result = manager.schedule_deletion();
    assert!(
        result.is_err(),
        "Cannot schedule deletion when already scheduled"
    );
}
