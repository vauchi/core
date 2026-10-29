// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Leave Vauchi: scheduling the identity's deletion carries whether the
//! other devices wipe when it runs, to every device (vauchi/private#916,
//! maintainer decisions 2026-10-29).

use vauchi_core::Vauchi;
use vauchi_core::api::{DeletionChoice, DeletionManager};
use vauchi_core::sync::SyncItem;

use crate::common::device_sync::{journal_for_tablet, link_tablet};

fn phone() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

const LEAVE_AND_WIPE: DeletionChoice = DeletionChoice {
    erase_guardian_set: false,
    wipe_other_devices: true,
};

// @internal
#[test]
fn leaving_with_wipe_records_the_choice_and_journals_it_for_my_tablet() {
    let wb = phone();
    let (registry, tablet) = link_tablet(&wb, [41u8; 32]);

    wb.schedule_identity_deletion(LEAVE_AND_WIPE).unwrap();

    let journaled: Vec<bool> = journal_for_tablet(&wb, registry, &tablet)
        .iter()
        .filter_map(|item| match item {
            SyncItem::DeletionScheduled {
                wipe_other_devices, ..
            } => Some(*wipe_other_devices),
            _ => None,
        })
        .collect();
    let recorded = DeletionManager::new(wb.storage())
        .scheduled_wipes_other_devices()
        .unwrap();
    assert_eq!((journaled, recorded), (vec![true], true));
}

// @internal
#[test]
fn a_sibling_that_applies_the_schedule_keeps_the_wipe_choice() {
    let tablet = phone();

    tablet
        .apply_sync_items(vec![SyncItem::DeletionScheduled {
            scheduled_at: 1_700_000_000,
            execute_at: 1_700_604_800,
            erase_guardian_set: false,
            wipe_other_devices: true,
            timestamp: 1_700_000_000,
        }])
        .unwrap();

    assert!(
        DeletionManager::new(tablet.storage())
            .scheduled_wipes_other_devices()
            .unwrap()
    );
}

// @internal
#[test]
fn a_device_told_to_wipe_wipes_itself_once_the_items_are_applied() {
    let mut tablet = phone();

    tablet
        .apply_sync_items(vec![SyncItem::WipeThisDevice {
            timestamp: 1_700_000_000,
        }])
        .unwrap();
    let wiped = tablet.apply_pending_wipe().unwrap();

    assert_eq!(
        (
            wiped,
            tablet.identity().is_none(),
            tablet.was_wiped_by_another_device()
        ),
        (true, true, true)
    );
}

// @internal
#[test]
fn without_the_instruction_nothing_is_wiped() {
    let mut tablet = phone();

    let wiped = tablet.apply_pending_wipe().unwrap();

    assert_eq!(
        (
            wiped,
            tablet.identity().is_some(),
            tablet.was_wiped_by_another_device()
        ),
        (false, true, false)
    );
}
