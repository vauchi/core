// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Own-card edits are recorded for the owner's other devices only when
//! there are other devices, and the recorded item names the changed field
//! (vauchi/private#522).

use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::identity::DeviceInfo;
use vauchi_core::sync::SyncItem;
use vauchi_core::{ContactField, FieldType, Vauchi};

const OTHER_SEED: [u8; 32] = [9u8; 32];

fn vauchi_with_devices(extra_device: bool) -> (Vauchi, Option<[u8; 32]>) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let mut registry = wb.identity().unwrap().initial_device_registry();
    let mut other = None;
    if extra_device {
        let laptop = DeviceInfo::derive(&OTHER_SEED, 1, "Laptop".into(), 0);
        other = Some(*laptop.device_id());
        registry
            .add_device_unsigned(laptop.to_registered(&OTHER_SEED))
            .unwrap();
    }
    wb.storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    wb.add_own_field(ContactField::new(FieldType::Email, "Home", "h@x.ch", 0))
        .unwrap();
    wb.add_own_field(ContactField::new(FieldType::Email, "Work", "w@x.ch", 0))
        .unwrap();
    (wb, other)
}

fn change_work(wb: &Vauchi) {
    let mut card = wb.own_card().unwrap().unwrap();
    let work_id = card
        .fields()
        .iter()
        .find(|f| f.label() == "Work")
        .unwrap()
        .id()
        .to_string();
    card.update_field_value(&work_id, "work@x.ch", 1).unwrap();
    wb.update_own_card(&card).unwrap();
}

// @internal
#[test]
fn a_single_device_records_nothing_for_other_devices() {
    let (wb, _) = vauchi_with_devices(false);

    change_work(&wb);

    assert!(wb.storage().sync().load_version_vector().unwrap().is_none());
}

// @internal
#[test]
fn a_second_device_receives_the_changed_field_by_name() {
    let (wb, other) = vauchi_with_devices(true);

    change_work(&wb);

    let identity = wb.identity().unwrap();
    let registry = wb
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    let orchestrator =
        DeviceSyncOrchestrator::load(wb.storage(), identity.create_device_info(0), registry)
            .unwrap();
    let labels: Vec<String> = orchestrator
        .pending_for_device(&other.unwrap())
        .iter()
        .filter_map(|item| match item {
            SyncItem::CardFieldSynced { field, .. } => Some(field.label().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        labels.last().map(String::as_str),
        Some("Work"),
        "{labels:?}"
    );
}
