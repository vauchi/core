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

// @internal
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

// @internal
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
