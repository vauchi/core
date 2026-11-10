// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The owner's other devices get cover too: a duress edit journals a
//! device-sync item of the serialized length the real item would have,
//! and a device that receives one applies nothing (ADR-032,
//! vauchi/private#811).

use proptest::prelude::*;
use vauchi_core::api::{AuthMode, DeviceSyncOrchestrator};
use vauchi_core::identity::DeviceRegistry;
use vauchi_core::sync::SyncItem;
use vauchi_core::{ContactField, FieldType, Vauchi};

use crate::common::device_sync::{create_test_contact, link_tablet};
use crate::common::helpers::create_vauchi_with_identity;

const APP_PASSWORD: &str = "my-pin-1234";
const DURESS_PIN: &str = "654321";

/// Alice with a linked tablet, unlocked with `credential`.
fn alice(credential: &str, expected_mode: AuthMode) -> (Vauchi, DeviceRegistry, [u8; 32]) {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password(APP_PASSWORD).unwrap();
    wb.setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .unwrap();
    let (registry, tablet_id) = link_tablet(&wb, [9u8; 32]);
    assert_eq!(wb.authenticate(credential).unwrap(), expected_mode);
    (wb, registry, tablet_id)
}

/// Under duress the real journal is out of the foreground's reach
/// (ADR-080): the unlock with the app password brings the real profile
/// back, which first journals the cover the session's edits owe.
fn back_to_the_real_profile(wb: &mut Vauchi) {
    assert_eq!(wb.authenticate(APP_PASSWORD).unwrap(), AuthMode::Normal);
}

fn journaled_for_tablet(
    wb: &Vauchi,
    registry: DeviceRegistry,
    tablet_id: &[u8; 32],
) -> Vec<SyncItem> {
    DeviceSyncOrchestrator::load(
        wb.storage(),
        wb.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap()
    .pending_for_device(tablet_id)
    .to_vec()
}

fn serialized_len(item: &SyncItem) -> usize {
    serde_json::to_vec(item).unwrap().len()
}

fn field_item(label: &str, value: &str, timestamp: u64) -> SyncItem {
    SyncItem::CardFieldSynced {
        field: ContactField::new(FieldType::Custom, label, value, 0),
        field_visibility: None,
        timestamp,
    }
}

fn fill(buf: &mut [u8]) {
    buf.fill(0xa5);
}

// @claim: S14
// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_cover_item_has_the_length_and_timestamp_of_the_item_it_stands_for() {
    let item = field_item("Phone", "+41 79 000 00 00", 1_700_000_000);

    let cover = SyncItem::cover_of(&item, fill).unwrap();

    assert!(matches!(cover, SyncItem::Cover { .. }), "got {cover:?}");
    assert_eq!(serialized_len(&cover), serialized_len(&item));
    assert_eq!(cover.timestamp(), item.timestamp());
}

// @claim: S14
// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn applying_a_cover_item_changes_nothing() {
    let wb = create_vauchi_with_identity("Alice");
    wb.add_own_field(ContactField::new(
        FieldType::Phone,
        "Phone",
        "+41 79 000 00 00",
        0,
    ))
    .unwrap();
    let before = wb.own_card().unwrap().unwrap();
    let cover = SyncItem::cover_of(&field_item("Email", "a@b.example", 9), fill).unwrap();

    wb.apply_sync_items(vec![cover]).unwrap();

    let after = wb.own_card().unwrap().unwrap();
    assert_eq!(after.display_name(), before.display_name());
    assert_eq!(after.fields(), before.fields());
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_duress_edit_journals_cover_of_the_real_item_s_length_for_the_other_device() {
    let (normal, normal_registry, tablet) = alice(APP_PASSWORD, AuthMode::Normal);
    let (mut duress, duress_registry, _) = alice(DURESS_PIN, AuthMode::Duress);
    for wb in [&normal, &duress] {
        let field = ContactField::new(FieldType::Phone, "Phone", "+41 79 000 00 00", 0);
        let field_id = field.id().to_string();
        wb.add_own_field(field).unwrap();
        assert!(wb.remove_own_field_by_id(&field_id).unwrap());
    }

    back_to_the_real_profile(&mut duress);
    let real = journaled_for_tablet(&normal, normal_registry, &tablet);
    let cover = journaled_for_tablet(&duress, duress_registry, &tablet);

    assert!(
        matches!(
            real.as_slice(),
            [
                SyncItem::CardFieldSynced { .. },
                SyncItem::CardFieldRemoved { .. }
            ]
        ),
        "got {real:?}"
    );
    assert!(
        matches!(
            cover.as_slice(),
            [SyncItem::Cover { .. }, SyncItem::Cover { .. }]
        ),
        "got {cover:?}"
    );
    let lengths = |items: &[SyncItem]| items.iter().map(serialized_len).collect::<Vec<_>>();
    assert_eq!(lengths(&cover), lengths(&real));
}

fn add_and_annotate_bob(wb: &Vauchi) {
    let bob = create_test_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();
    wb.block_contact(&bob_id).unwrap();
    wb.archive_contact(&bob_id).unwrap();
    wb.ignore_contact(&bob_id).unwrap();
    wb.add_personal_note(&bob_id, "met at the station").unwrap();
}

// @claim: S14
// @scenario: duress_mode :: Duress mode looks identical to normal mode
// @scenario: duress_mode :: Decoy contacts are static (no sync)
// @scenario: duress_mode :: Duress mode sync is real but carries nothing real
#[test]
fn a_duress_session_never_journals_a_non_cover_item() {
    let (normal, normal_registry, tablet) = alice(APP_PASSWORD, AuthMode::Normal);
    let (mut duress, duress_registry, _) = alice(DURESS_PIN, AuthMode::Duress);
    add_and_annotate_bob(&normal);
    add_and_annotate_bob(&duress);

    back_to_the_real_profile(&mut duress);
    let real = journaled_for_tablet(&normal, normal_registry, &tablet);
    let cover = journaled_for_tablet(&duress, duress_registry, &tablet);

    assert_eq!(real.len(), 5, "got {real:?}");
    let non_cover: Vec<_> = cover
        .iter()
        .filter(|item| !matches!(item, SyncItem::Cover { .. }))
        .collect();
    assert!(
        non_cover.is_empty(),
        "journaled for the tablet: {non_cover:?}"
    );
    let lengths = |items: &[SyncItem]| items.iter().map(serialized_len).collect::<Vec<_>>();
    assert_eq!(lengths(&cover), lengths(&real));
}

fn edit_tags_and_groups(wb: &Vauchi) {
    let tag = wb.create_tag("Work").unwrap();
    assert!(wb.delete_tag(&tag.id).unwrap());
    let group = wb.create_group("Family").unwrap();
    wb.delete_group(group.id()).unwrap();
}

// @claim: S14
// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn a_duress_tag_or_group_change_journals_cover_of_the_real_item_s_length() {
    let (normal, normal_registry, tablet) = alice(APP_PASSWORD, AuthMode::Normal);
    let (mut duress, duress_registry, _) = alice(DURESS_PIN, AuthMode::Duress);
    edit_tags_and_groups(&normal);
    edit_tags_and_groups(&duress);

    back_to_the_real_profile(&mut duress);
    let real = journaled_for_tablet(&normal, normal_registry, &tablet);
    let cover = journaled_for_tablet(&duress, duress_registry, &tablet);

    assert_eq!(real.len(), 4, "got {real:?}");
    assert!(
        cover
            .iter()
            .all(|item| matches!(item, SyncItem::Cover { .. })),
        "got {cover:?}"
    );
    let lengths = |items: &[SyncItem]| items.iter().map(serialized_len).collect::<Vec<_>>();
    assert_eq!(lengths(&cover), lengths(&real));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @scenario: duress_mode :: Duress mode looks identical to normal mode
    #[test]
    fn any_field_item_s_cover_has_its_length(
        label in "[a-z]{1,30}",
        value in "[a-z0-9 ]{0,200}",
        timestamp in any::<u64>(),
    ) {
        let item = field_item(&label, &value, timestamp);
        let cover = SyncItem::cover_of(&item, fill).unwrap();
        prop_assert_eq!(serialized_len(&cover), serialized_len(&item));
        let round_trip: SyncItem = serde_json::from_slice(&serde_json::to_vec(&cover).unwrap()).unwrap();
        prop_assert_eq!(round_trip, cover);
    }
}
