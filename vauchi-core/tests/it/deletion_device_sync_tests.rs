// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduling or cancelling my identity's deletion reaches my other
//! devices, so each shows the countdown or its end (vauchi/private#723).

use vauchi_core::Vauchi;
use vauchi_core::api::DeletionChoice;
use vauchi_core::sync::SyncItem;

use crate::common::device_sync::{journal_for_tablet, link_tablet};

const GRACE_SECS: u64 = 7 * 24 * 60 * 60;

fn phone() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

// @scenario: privacy_compliance :: Scheduling or cancelling my deletion reaches my other devices
#[test]
fn scheduling_my_identitys_deletion_is_journaled_for_my_tablet() {
    let wb = phone();
    let (registry, tablet) = link_tablet(&wb, [31u8; 32]);

    wb.schedule_identity_deletion(DeletionChoice {
        erase_guardian_set: true,
        wipe_other_devices: false,
    })
    .unwrap();

    let scheduled: Vec<(u64, bool)> = journal_for_tablet(&wb, registry, &tablet)
        .iter()
        .filter_map(|item| match item {
            SyncItem::DeletionScheduled {
                scheduled_at,
                execute_at,
                erase_guardian_set,
                ..
            } => Some((execute_at - scheduled_at, *erase_guardian_set)),
            _ => None,
        })
        .collect();
    assert_eq!(scheduled, vec![(GRACE_SECS, true)]);
}

// @scenario: privacy_compliance :: Scheduling or cancelling my deletion reaches my other devices
#[test]
fn cancelling_my_identitys_deletion_is_journaled_for_my_tablet() {
    let wb = phone();
    wb.schedule_identity_deletion(DeletionChoice::default())
        .unwrap();
    let (registry, tablet) = link_tablet(&wb, [32u8; 32]);

    wb.cancel_identity_deletion().unwrap();

    let cancelled = journal_for_tablet(&wb, registry, &tablet)
        .iter()
        .filter(|item| matches!(item, SyncItem::DeletionCancelled { .. }))
        .count();
    assert_eq!(cancelled, 1);
}

// @scenario: privacy_compliance :: Scheduling or cancelling my deletion reaches my other devices
#[test]
fn a_delayed_older_schedule_cannot_resurrect_a_cancelled_deletion() {
    use vauchi_core::api::sync::DeviceSyncOrchestrator;
    use vauchi_core::storage::DeletionState;

    let wb = phone();
    let identity = wb.identity().unwrap();
    let (registry, tablet) = link_tablet(&wb, *identity.master_seed());
    let cancel = SyncItem::DeletionCancelled { timestamp: 2000 };
    let schedule = SyncItem::DeletionScheduled {
        scheduled_at: 1000,
        execute_at: 1000 + GRACE_SECS,
        erase_guardian_set: false,
        wipe_other_devices: false,
        timestamp: 1000,
    };

    for item in [cancel, schedule] {
        let mut receiver = DeviceSyncOrchestrator::load(
            wb.storage(),
            identity.create_device_info(0),
            registry.clone(),
        )
        .unwrap();
        let accepted = receiver.process_incoming(vec![item], &tablet).unwrap();
        wb.apply_sync_items(accepted).unwrap();
    }

    assert_eq!(
        wb.storage().consent().load_deletion_state().unwrap(),
        DeletionState::None,
        "a delayed schedule must not reverse a newer cancellation after receiver reload",
    );
}

// @scenario: privacy_compliance :: Scheduling or cancelling my deletion reaches my other devices
#[test]
fn a_persisted_legacy_cancellation_rejects_an_older_schedule_after_upgrade() {
    use std::collections::HashMap;
    use vauchi_core::api::sync::DeviceSyncOrchestrator;
    use vauchi_core::storage::DeletionState;
    use vauchi_core::sync::FieldStamp;

    let wb = phone();
    let identity = wb.identity().unwrap();
    let (registry, tablet) = link_tablet(&wb, *identity.master_seed());
    let stamps = HashMap::from([
        (
            "deletion:scheduled".into(),
            FieldStamp {
                timestamp: 1000,
                device_id: tablet,
            },
        ),
        (
            "deletion:cancelled".into(),
            FieldStamp {
                timestamp: 2000,
                device_id: tablet,
            },
        ),
    ]);
    wb.storage().sync().save_field_timestamps(&stamps).unwrap();
    let mut receiver =
        DeviceSyncOrchestrator::load(wb.storage(), identity.create_device_info(0), registry)
            .unwrap();

    let accepted = receiver
        .process_incoming(
            vec![SyncItem::DeletionScheduled {
                scheduled_at: 1500,
                execute_at: 1500 + GRACE_SECS,
                erase_guardian_set: false,
                wipe_other_devices: false,
                timestamp: 1500,
            }],
            &tablet,
        )
        .unwrap();
    wb.apply_sync_items(accepted).unwrap();

    assert_eq!(
        wb.storage().consent().load_deletion_state().unwrap(),
        DeletionState::None
    );
}

// @scenario: privacy_compliance :: Scheduling or cancelling my deletion reaches my other devices
#[test]
fn a_newer_schedule_after_cancellation_still_applies() {
    use vauchi_core::api::sync::DeviceSyncOrchestrator;
    use vauchi_core::storage::DeletionState;

    let wb = phone();
    let identity = wb.identity().unwrap();
    let (registry, tablet) = link_tablet(&wb, *identity.master_seed());
    for item in [
        SyncItem::DeletionCancelled { timestamp: 2000 },
        SyncItem::DeletionScheduled {
            scheduled_at: 3000,
            execute_at: 3000 + GRACE_SECS,
            erase_guardian_set: true,
            wipe_other_devices: false,
            timestamp: 3000,
        },
    ] {
        let mut receiver = DeviceSyncOrchestrator::load(
            wb.storage(),
            identity.create_device_info(0),
            registry.clone(),
        )
        .unwrap();
        let accepted = receiver.process_incoming(vec![item], &tablet).unwrap();
        wb.apply_sync_items(accepted).unwrap();
    }

    assert_eq!(
        wb.storage().consent().load_deletion_state().unwrap(),
        DeletionState::Scheduled {
            scheduled_at: 3000,
            execute_at: 3000 + GRACE_SECS,
            erase_guardian_set: true,
            wipe_other_devices: false,
        },
    );
}
