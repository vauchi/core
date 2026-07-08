// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the `Vauchi` tag API (owner-private annotation vocabulary).
//!
//! Covers autocomplete-or-create, per-contact tag listing, suggestions, and
//! validation. See `ADR-051`.

use vauchi_core::exchange::{X3DH, X3DHKeyPair};
use vauchi_core::{ContactField, FieldType, Vauchi};

/// Vauchi with an identity.
fn setup() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

/// Adds an exchanged contact with the given display name, returns its id.
fn add_contact(wb: &Vauchi, name: &str) -> String {
    let their_identity = X3DHKeyPair::generate();
    let their_ephemeral = X3DHKeyPair::generate();
    let our_x3dh = wb.identity().unwrap().x3dh_keypair();
    let (_, their_ephemeral_pub) = X3DH::initiate(&their_ephemeral, our_x3dh.public_key()).unwrap();
    wb.accept_relay_exchange(their_identity.public_key(), &their_ephemeral_pub, name)
        .unwrap()
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn create_tag_appears_in_vocabulary() {
    let wb = setup();

    let tag = wb.create_tag("climbing-gym").unwrap();

    let names: Vec<String> = wb
        .list_tags()
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["climbing-gym"]);
    assert_eq!(tag.name, "climbing-gym");
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn create_tag_rejects_empty_or_whitespace() {
    let wb = setup();

    assert!(wb.create_tag("").is_err(), "empty name rejected");
    assert!(
        wb.create_tag("   ").is_err(),
        "whitespace-only name rejected"
    );
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn create_tag_trims_surrounding_whitespace() {
    let wb = setup();

    let tag = wb.create_tag("  work  ").unwrap();
    assert_eq!(tag.name, "work");
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn add_tag_to_contact_creates_and_applies() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");

    let tag = wb.add_tag_to_contact(&bob, "climbing-gym").unwrap();
    assert!(
        tag.contains(&bob),
        "returned tag reflects the new membership"
    );

    let on_bob: Vec<String> = wb
        .tags_for_contact(&bob)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(on_bob, vec!["climbing-gym"]);
}

// @scenario: contact-annotations.feature - Adding a tag autocompletes an existing one
// @internal
#[test]
fn add_tag_reuses_existing_case_insensitively() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    let carol = add_contact(&wb, "Carol");

    let first = wb.add_tag_to_contact(&bob, "Climbing-Gym").unwrap();
    // Different casing must reuse the same tag, not create a duplicate.
    let second = wb.add_tag_to_contact(&carol, "climbing-gym").unwrap();

    assert_eq!(first.id, second.id, "same vocabulary entry reused");
    assert_eq!(wb.list_tags().unwrap().len(), 1, "no duplicate created");
    assert!(second.contains(&bob) && second.contains(&carol));
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn add_tag_to_contact_is_idempotent() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");

    wb.add_tag_to_contact(&bob, "work").unwrap();
    wb.add_tag_to_contact(&bob, "work").unwrap();

    assert_eq!(wb.list_tags().unwrap().len(), 1);
    assert_eq!(wb.tags_for_contact(&bob).unwrap().len(), 1);
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn add_tag_to_unknown_contact_errors() {
    let wb = setup();

    let result = wb.add_tag_to_contact("no-such-contact", "work");
    assert!(result.is_err(), "tagging a missing contact must error");
    // And no orphan tag was created.
    assert!(wb.list_tags().unwrap().is_empty());
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn remove_tag_from_contact_leaves_vocabulary_intact() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    let tag = wb.add_tag_to_contact(&bob, "work").unwrap();

    wb.remove_tag_from_contact(&tag.id, &bob).unwrap();

    assert!(wb.tags_for_contact(&bob).unwrap().is_empty(), "untagged");
    assert_eq!(wb.list_tags().unwrap().len(), 1, "tag stays in vocabulary");
}

// @scenario: contact-annotations.feature - Adding a tag autocompletes an existing one
// @internal
#[test]
fn tag_name_suggestions_match_prefix_case_insensitively() {
    let wb = setup();
    wb.create_tag("climbing-gym").unwrap();
    wb.create_tag("climbing-trip").unwrap();
    wb.create_tag("work").unwrap();

    let mut suggestions = wb.tag_name_suggestions("CLIMB").unwrap();
    suggestions.sort();
    assert_eq!(suggestions, vec!["climbing-gym", "climbing-trip"]);

    // Empty prefix returns the whole vocabulary.
    assert_eq!(wb.tag_name_suggestions("").unwrap().len(), 3);
}

// @scenario: contact-annotations.feature - Adding a tag autocompletes an existing one
// @internal
#[test]
fn find_tag_by_name_is_trimmed_and_case_insensitive() {
    let wb = setup();
    let created = wb.create_tag("Work").unwrap();

    let found = wb.find_tag_by_name("  work  ").unwrap().unwrap();
    assert_eq!(found.id, created.id);
    assert!(wb.find_tag_by_name("missing").unwrap().is_none());
}

// ── Tag → Group promotion (draft → review → confirm) ──────────────────────────

// @scenario: contact-annotations.feature - Promoting a tag drafts a group
// @internal
#[test]
fn begin_tag_promotion_drafts_members_and_persists_nothing() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    let carol = add_contact(&wb, "Carol");
    let tag = wb.add_tag_to_contact(&bob, "work").unwrap();
    wb.add_tag_to_contact(&carol, "work").unwrap();

    let draft = wb.begin_tag_promotion(&tag.id).unwrap();

    assert_eq!(draft.name, "work");
    assert_eq!(draft.contact_ids.len(), 2);
    assert!(draft.contact_ids.contains(&bob) && draft.contact_ids.contains(&carol));

    // Draft is side-effect-free: no group saved, tag still present (= cancel).
    assert!(wb.list_groups().unwrap().is_empty(), "no group persisted");
    assert_eq!(wb.list_tags().unwrap().len(), 1, "tag untouched");
}

// @scenario: contact-annotations.feature - Promoting a tag drafts a group
// @internal
#[test]
fn begin_tag_promotion_inherits_default_visible_fields() {
    let wb = setup();
    wb.add_own_field(ContactField::new(FieldType::Email, "Work", "a@b.com", 0))
        .unwrap();
    let card = wb.own_card().unwrap().unwrap();
    let field_id = card.fields()[0].id().to_string();
    wb.set_field_shown(&field_id, true).unwrap(); // → Everyone (default-visible)

    let bob = add_contact(&wb, "Bob");
    let tag = wb.add_tag_to_contact(&bob, "work").unwrap();

    let draft = wb.begin_tag_promotion(&tag.id).unwrap();
    assert!(
        draft.visible_fields.contains(&field_id),
        "draft inherits the owner's current default-visible fields"
    );
}

// @scenario: contact-annotations.feature - Confirming the promotion saves and consumes
// @internal
#[test]
fn confirm_tag_promotion_creates_group_with_reviewed_fields_and_consumes_tag() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    let tag = wb.add_tag_to_contact(&bob, "work").unwrap();

    // Owner confirms with a reviewed visible-field set (the edit step).
    let group_id = wb
        .confirm_tag_promotion(&tag.id, vec!["Work Email".to_string()])
        .unwrap();

    let group = wb.get_group(&group_id).unwrap();
    assert_eq!(group.name(), "work");
    assert!(group.contains_contact(&bob), "members carried over");
    assert!(
        group.is_field_visible("Work Email"),
        "reviewed visible field applied"
    );

    // Replace semantics: the tag is consumed.
    assert!(
        wb.list_tags().unwrap().is_empty(),
        "tag deleted after promotion"
    );
}

// @scenario: contact-annotations.feature - Confirming the promotion saves and consumes
// @internal
#[test]
fn confirm_tag_promotion_uses_reviewed_not_inherited_fields() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    let tag = wb.add_tag_to_contact(&bob, "work").unwrap();

    // Confirm with an empty reviewed set (owner hid everything on review).
    let group_id = wb.confirm_tag_promotion(&tag.id, vec![]).unwrap();

    let group = wb.get_group(&group_id).unwrap();
    assert!(
        group.visible_fields().is_empty(),
        "confirm honours the reviewed set, not auto-inherited defaults"
    );
}

// @scenario: contact-annotations.feature - Promoting a tag drafts a group
// @internal
#[test]
fn begin_tag_promotion_unknown_tag_errors() {
    let wb = setup();
    assert!(wb.begin_tag_promotion("no-such-tag").is_err());
}

// ── No-leak regression (security): tags never reach the exchange wire ──────────

// @scenario: contact-annotations.feature - Tags are never shared with the contact
// @internal
#[test]
fn tags_never_appear_in_card_wire_form() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    wb.add_tag_to_contact(&bob, "WIRE-LEAK-CANARY").unwrap();

    // The own card is the exchange wire form. Serializing it directly must never
    // carry tag data — tags live in a separate store, off the card entirely.
    let card = wb.own_card().unwrap().unwrap();
    let bytes = serde_json::to_vec(&card).expect("serialize card");
    let wire = String::from_utf8_lossy(&bytes);

    assert!(
        !wire.contains("WIRE-LEAK-CANARY"),
        "tag name must never reach the exchanged card"
    );
}
