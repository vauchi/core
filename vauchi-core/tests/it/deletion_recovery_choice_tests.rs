// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The user decides when scheduling a deletion whether the relay also
//! erases their guardian set; the choice waits out the grace period with
//! the schedule and reaches the purge that runs seven days later (#711).

use vauchi_core::api::DeletionManager;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{DeletionState, Storage};

fn storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// @scenario: privacy_compliance :: The relay purge carries my recovery choice
#[test]
fn erasing_everything_is_remembered_with_the_schedule() {
    let storage = storage();
    let deletion = DeletionManager::new(&storage);

    deletion
        .schedule_deletion_erasing_guardian_set(true)
        .unwrap();

    assert!(deletion.scheduled_erases_guardian_set().unwrap());
    assert!(matches!(
        deletion.deletion_state().unwrap(),
        DeletionState::Scheduled {
            erase_guardian_set: true,
            ..
        }
    ));
}

// @scenario: privacy_compliance :: The relay purge carries my recovery choice
#[test]
fn keeping_recovery_is_remembered_with_the_schedule() {
    let storage = storage();
    let deletion = DeletionManager::new(&storage);

    deletion
        .schedule_deletion_erasing_guardian_set(false)
        .unwrap();

    assert!(!deletion.scheduled_erases_guardian_set().unwrap());
}

// @internal
#[test]
fn the_plain_schedule_keeps_recovery() {
    let storage = storage();
    let deletion = DeletionManager::new(&storage);

    deletion.schedule_deletion().unwrap();

    assert!(!deletion.scheduled_erases_guardian_set().unwrap());
}

// @internal
#[test]
fn nothing_scheduled_erases_nothing() {
    let storage = storage();
    let deletion = DeletionManager::new(&storage);
    deletion
        .schedule_deletion_erasing_guardian_set(true)
        .unwrap();

    deletion.cancel_deletion().unwrap();

    assert!(!deletion.scheduled_erases_guardian_set().unwrap());
}

// A schedule saved before the choice existed, or synced from a device that
// predates it, keeps recovery.
// @scenario: privacy_compliance :: The relay purge carries my recovery choice
#[test]
fn a_schedule_from_before_the_choice_keeps_recovery() {
    let old: DeletionState =
        serde_json::from_str(r#"{"Scheduled":{"scheduled_at":10,"execute_at":20}}"#).unwrap();

    assert_eq!(
        old,
        DeletionState::Scheduled {
            scheduled_at: 10,
            execute_at: 20,
            erase_guardian_set: false,
        }
    );
}
