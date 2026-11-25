// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Visibility partitions for cohort publications (private#1375, step 3).
#![cfg(feature = "notice-core")]

use vauchi_core::card::{ContactCard, ContactField, FieldType};
use vauchi_core::notice::groups::{Groups, GroupsError, VisibleCard};

fn card() -> ContactCard {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Email",
        "alice@example.org",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Phone",
        "+41 00 000 00 00",
        0,
    ))
    .unwrap();
    card
}

fn view(contact: &str, card: &ContactCard, fields: &[usize]) -> VisibleCard {
    let allowed = fields
        .iter()
        .map(|i| card.fields()[*i].id().to_string())
        .collect();
    VisibleCard::new(contact.to_string(), card.filtered_to(&allowed))
}

// @internal
#[test]
fn equal_visible_fields_share_a_publication_and_an_override_gets_its_own() {
    let card = card();
    let mut groups = Groups::default();
    groups
        .reconcile(vec![
            view("bob", &card, &[0]),
            view("carol", &card, &[0]),
            view("dave", &card, &[0, 1]),
        ])
        .unwrap();
    assert_eq!(groups.publications().len(), 2);
    assert_eq!(groups.for_contact("bob"), groups.for_contact("carol"));
    assert_ne!(groups.for_contact("bob"), groups.for_contact("dave"));
}

// @internal
#[test]
fn unchanged_views_keep_their_group_and_key_after_a_reload() {
    let card = card();
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let before = groups.for_contact("bob").unwrap();
    let mut loaded = Groups::load(&groups.save().unwrap()).unwrap();
    loaded.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    assert_eq!(loaded.for_contact("bob"), Some(before));
}

// @internal
#[test]
fn a_revoked_field_moves_the_contact_and_rotates_the_remaining_audience_key() {
    let card = card();
    let mut groups = Groups::default();
    groups
        .reconcile(vec![
            view("bob", &card, &[0, 1]),
            view("carol", &card, &[0, 1]),
        ])
        .unwrap();
    let before = groups.for_contact("bob").unwrap();
    groups
        .reconcile(vec![
            view("bob", &card, &[0]),
            view("carol", &card, &[0, 1]),
        ])
        .unwrap();
    let bob = groups.for_contact("bob").unwrap();
    let carol = groups.for_contact("carol").unwrap();
    assert_ne!(bob.group, carol.group);
    assert_eq!(carol.group, before.group);
    assert_ne!(carol.update_key, before.update_key);
    assert_ne!(bob.update_key, before.update_key);
    let bytes = groups.card_for_contact("bob").unwrap();
    assert_eq!(bytes.fields().len(), 1);
    assert_eq!(bytes.fields()[0].value(), "alice@example.org");
}

// @internal
#[test]
fn visibility_metadata_is_never_published_to_an_audience() {
    let mut card = card();
    let field = card.fields()[0].id().to_string();
    card.field_visibility_mut()
        .set_contacts(&field, ["private-contact-id".to_string()].into());
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let card = groups.card_for_contact("bob").unwrap();
    assert!(card.field_visibility().is_empty());
    assert!(
        !serde_json::to_string(&card)
            .unwrap()
            .contains("private-contact-id")
    );
}

// @internal
#[test]
fn private_field_notes_are_not_part_of_a_publication() {
    let mut card = card();
    let id = card.fields()[0].id().to_string();
    card.update_field_note(&id, Some("private-owner-note".into()))
        .unwrap();
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let card = groups.card_for_contact("bob").unwrap();
    assert_eq!(card.fields()[0].note(), None);
    assert!(
        !serde_json::to_string(&card)
            .unwrap()
            .contains("private-owner-note")
    );
}

// @internal
#[test]
fn an_oversized_card_is_refused_without_replacing_an_existing_publication() {
    let mut card = card();
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let before = groups.for_contact("bob");
    for index in 0..12 {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("Note {index}"),
            &"x".repeat(400),
            0,
        ))
        .unwrap();
    }
    let fields: Vec<_> = (0..card.fields().len()).collect();
    assert_eq!(
        groups.reconcile(vec![view("bob", &card, &fields)]),
        Err(GroupsError::BodySize)
    );
    assert_eq!(groups.for_contact("bob"), before);
}

// @internal
#[test]
fn a_private_note_edit_keeps_the_same_publication() {
    let mut card = card();
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let before = groups.for_contact("bob");
    let id = card.fields()[0].id().to_string();
    card.update_field_note(&id, Some("owner-only".into()))
        .unwrap();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    assert_eq!(groups.for_contact("bob"), before);
}

// @internal
#[test]
fn an_edit_rotates_the_key_and_a_removed_contact_has_no_publication() {
    let mut card = card();
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let before = groups.for_contact("bob").unwrap();
    let id = card.fields()[0].id().to_string();
    card.update_field_value(&id, "new@example.org", 1).unwrap();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let after = groups.for_contact("bob").unwrap();
    assert_eq!(after.group, before.group);
    assert_ne!(after.update_key, before.update_key);
    groups.reconcile(Vec::new()).unwrap();
    assert_eq!(groups.for_contact("bob"), None);
    assert_eq!(groups.publications().len(), 0);
}

// @internal
#[test]
fn duplicate_or_inconsistent_views_are_refused_without_changing_the_partition() {
    let card = card();
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let before = groups.for_contact("bob");
    assert_eq!(
        groups.reconcile(vec![view("bob", &card, &[0]), view("bob", &card, &[0])]),
        Err(GroupsError::DuplicateContact)
    );
    let mut other = card.clone();
    let id = card.fields()[0].id().to_string();
    other
        .update_field_value(&id, "other@example.org", 1)
        .unwrap();
    assert_eq!(
        groups.reconcile(vec![view("bob", &card, &[0]), view("carol", &other, &[0])]),
        Err(GroupsError::InconsistentView)
    );
    assert_eq!(groups.for_contact("bob"), before);
}

proptest::proptest! {
    // @internal
    #[test]
    fn a_partition_groups_exactly_the_equal_views(masks in proptest::collection::vec(0u8..4, 1..20)) {
        let card = card();
        let mut groups = Groups::default();
        let projections = masks.iter().enumerate().map(|(index, mask)| {
            let fields: Vec<_> = (0..2).filter(|field| mask & (1 << field) != 0).collect();
            view(&index.to_string(), &card, &fields)
        }).collect();
        groups.reconcile(projections).unwrap();
        for (left, left_mask) in masks.iter().enumerate() {
            for (right, right_mask) in masks.iter().enumerate() {
                let left = groups.for_contact(&left.to_string()).unwrap();
                let right = groups.for_contact(&right.to_string()).unwrap();
                proptest::prop_assert_eq!(left.group == right.group, left_mask == right_mask);
            }
        }
    }
}

// @internal
#[test]
fn an_inconsistent_publisher_is_refused_without_losing_the_audience() {
    let card = card();
    let mut groups = Groups::default();
    groups.reconcile(vec![view("bob", &card, &[0])]).unwrap();
    let before = groups.for_contact("bob");
    groups.publications_mut()[0] =
        vauchi_core::notice::publish::Publisher::for_group(u32::MAX, [1; 32], b"other");
    assert!(matches!(
        Groups::load(&groups.save().unwrap()),
        Err(vauchi_core::notice::saved::SavedError)
    ));
    assert_eq!(
        groups.reconcile(vec![view("bob", &card, &[0])]),
        Err(GroupsError::Encoding)
    );
    assert_eq!(groups.for_contact("bob"), before);
}
