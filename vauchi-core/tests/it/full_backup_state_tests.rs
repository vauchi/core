// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Full password backups must preserve owner privacy choices after reopening.

use std::collections::HashSet;

use proptest::prelude::*;
use vauchi_core::contact::{Contact, Group};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::GroupSyncData;
use vauchi_core::{ContactField, FieldType, Vauchi};

const PASSWORD: &str = "correct-horse-battery-staple";

// @scenario: backup_format_versioning :: Full backup includes identity, contacts, own card, and labels
#[test]
fn full_backup_state_removed_fields_do_not_make_export_unrestorable() {
    let (source, bob, email, phone) = setup();
    let mut group = Group::new("work".into(), "Work", 10);
    group.add_contact(&bob, 20);
    group.set_visible_fields(HashSet::from([email.clone(), phone.clone()]), 30);
    source.storage().labels().save_group(&group).unwrap();
    source
        .storage()
        .labels()
        .save_contact_override(&bob, &email, false)
        .unwrap();
    source
        .storage()
        .labels()
        .save_contact_override(&bob, &phone, false)
        .unwrap();
    assert!(source.remove_own_field_by_id(&email).unwrap());
    let mut target = Vauchi::in_memory().unwrap();
    target
        .import_full_backup(&source.export_full_backup(PASSWORD).unwrap(), PASSWORD)
        .unwrap();
    let restored = target.storage().labels().load_group("work").unwrap();
    assert_eq!(restored.visible_fields(), &HashSet::from([phone.clone()]));
    assert_eq!(
        target
            .storage()
            .labels()
            .load_contact_overrides(&bob)
            .unwrap(),
        std::collections::HashMap::from([(phone, false)])
    );
}

// @scenario: backup_format_versioning :: Full backup includes identity, contacts, own card, and labels
#[test]
fn full_backup_state_deleted_group_members_are_not_resurrected() {
    let (source, bob, _, _) = setup();
    let mut contact = source
        .storage()
        .contacts()
        .load_contact(&bob)
        .unwrap()
        .unwrap();
    let mut group = Group::new("work".into(), "Work", 10);
    group.add_contact(&bob, 20);
    source.storage().labels().save_group(&group).unwrap();
    contact.soft_delete(30);
    source.storage().contacts().save_contact(&contact).unwrap();
    let mut target = Vauchi::in_memory().unwrap();
    target
        .import_full_backup(&source.export_full_backup(PASSWORD).unwrap(), PASSWORD)
        .unwrap();
    assert!(
        target
            .storage()
            .contacts()
            .load_contact(&bob)
            .unwrap()
            .is_none()
    );
    assert!(
        target
            .storage()
            .labels()
            .load_group("work")
            .unwrap()
            .contacts()
            .is_empty()
    );
}

fn setup() -> (Vauchi, String, String, String) {
    let mut source = Vauchi::in_memory().unwrap();
    source.create_identity("Alice").unwrap();
    let email = ContactField::new(FieldType::Email, "Email", "alice@example.test", 10);
    let phone = ContactField::new(FieldType::Phone, "Phone", "+12025550164", 10);
    let email_id = email.id().to_owned();
    let phone_id = phone.id().to_owned();
    source.add_own_field(email).unwrap();
    source.add_own_field(phone).unwrap();
    let bob = Contact::from_exchange(
        [0x42; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        10,
    );
    let bob_id = bob.id().to_owned();
    source.update_contact(&bob).unwrap();
    (source, bob_id, email_id, phone_id)
}

// @scenario: backup_format_versioning :: Full backup includes identity, contacts, own card, and labels
#[test]
fn full_backup_state_preserves_complete_groups_and_explicit_allow_and_deny() {
    let (source, bob, email, phone) = setup();
    let mut group = Group::new("work".into(), "Work", 10);
    group.add_contact(&bob, 20);
    group.set_visible_fields(HashSet::from([email.clone(), phone.clone()]), 30);
    group
        .set_display_name_override(Some("Alice at work"), 40)
        .unwrap();
    group.set_bio_override(Some("Work biography"), 50).unwrap();
    source.storage().labels().save_group(&group).unwrap();
    source
        .storage()
        .labels()
        .save_contact_override(&bob, &email, true)
        .unwrap();
    source
        .storage()
        .labels()
        .save_contact_override(&bob, &phone, false)
        .unwrap();
    let expected_group = GroupSyncData::from_group(&group);

    let backup = source.export_full_backup(PASSWORD).unwrap();
    let mut target = Vauchi::in_memory().unwrap();
    target.import_full_backup(&backup, PASSWORD).unwrap();

    let restored = target.storage().labels().load_group("work").unwrap();
    assert_eq!(GroupSyncData::from_group(&restored), expected_group);
    assert_eq!(
        target
            .storage()
            .labels()
            .load_contact_overrides(&bob)
            .unwrap(),
        std::collections::HashMap::from([(email, true), (phone, false)])
    );
}

// @scenario: backup_format_versioning :: Full backup includes identity, contacts, own card, and labels
#[test]
fn full_backup_state_preserves_blocked_ignored_archived_contacts() {
    let (source, bob_id, _, _) = setup();
    let mut bob = source
        .storage()
        .contacts()
        .load_contact(&bob_id)
        .unwrap()
        .unwrap();
    bob.block();
    bob.set_hidden(true);
    bob.set_favorite(true);
    bob.ignore(40);
    bob.archive(50);
    bob.set_card_updated_at(Some(30));
    source.storage().contacts().save_contact(&bob).unwrap();
    assert!(source.list_contacts().unwrap().is_empty());

    let mut target = Vauchi::in_memory().unwrap();
    target
        .import_full_backup(&source.export_full_backup(PASSWORD).unwrap(), PASSWORD)
        .unwrap();
    let restored = target
        .storage()
        .contacts()
        .load_contact(&bob_id)
        .unwrap()
        .unwrap();
    assert!(restored.is_blocked());
    assert!(restored.is_hidden());
    assert!(restored.is_favorite());
    assert_eq!(restored.ignored_at(), Some(40));
    assert_eq!(restored.archived_at(), Some(50));
    assert_eq!(restored.card_updated_at(), Some(30));
    assert!(target.list_contacts().unwrap().is_empty());
}

// @scenario: backup_format_versioning :: Unknown version byte is rejected
#[test]
fn full_backup_state_old_reader_rejects_richer_password_backup() {
    let (source, _, _, _) = setup();
    let encoded = source.export_full_backup(PASSWORD).unwrap();
    let error = vauchi_core::backup::import_full_backup(&hex::decode(encoded).unwrap(), PASSWORD)
        .unwrap_err();
    assert!(matches!(
        error,
        vauchi_core::backup::BackupError::UnsupportedVersion(6)
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]
    // @scenario: backup_format_versioning :: Full backup includes identity, contacts, own card, and labels
    #[test]
    fn full_backup_state_override_roundtrip(allow_email in any::<bool>(), allow_phone in any::<bool>()) {
        let (source, bob, email, phone) = setup();
        source.storage().labels().save_contact_override(&bob, &email, allow_email).unwrap();
        source.storage().labels().save_contact_override(&bob, &phone, allow_phone).unwrap();
        let mut target = Vauchi::in_memory().unwrap();
        target.import_full_backup(&source.export_full_backup(PASSWORD).unwrap(), PASSWORD).unwrap();
        let overrides = target.storage().labels().load_contact_overrides(&bob).unwrap();
        prop_assert_eq!(overrides.get(&email), Some(&allow_email));
        prop_assert_eq!(overrides.get(&phone), Some(&allow_phone));
    }
}
