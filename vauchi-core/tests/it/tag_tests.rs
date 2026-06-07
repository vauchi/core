// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the `Tag` domain type (owner-private annotation vocabulary).
//!
//! Storage CRUD and encryption are covered separately (T1.1b / T1.2);
//! these exercise the in-memory type behaviour. See `ADR-051`.

use vauchi_core::contact::Tag;

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn new_tag_has_name_empty_membership_and_stamped_time() {
    let tag = Tag::new("climbing-gym", 1_700_000_000);

    assert_eq!(tag.name, "climbing-gym");
    assert!(
        tag.contact_ids.is_empty(),
        "new tag starts with no contacts"
    );
    assert_eq!(tag.created_at, 1_700_000_000);
    assert!(!tag.id.is_empty(), "tag must have a generated id");
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn two_tags_get_distinct_ids() {
    let a = Tag::new("work", 0);
    let b = Tag::new("work", 0);

    assert_ne!(a.id, b.id, "each Tag::new must generate a fresh UUID");
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn add_contact_is_idempotent_and_reports_newness() {
    let mut tag = Tag::new("berlin-trip", 0);

    assert!(tag.add_contact("c1"), "first add reports newly added");
    assert!(
        !tag.add_contact("c1"),
        "re-adding the same contact reports false"
    );
    assert_eq!(tag.contact_ids.len(), 1, "no duplicate membership");
    assert!(tag.contains("c1"));
    assert!(!tag.contains("c2"));
}

// @scenario: contact-annotations.feature - Create a new tag on a contact
// @internal
#[test]
fn remove_contact_reports_presence() {
    let mut tag = Tag::new("berlin-trip", 0);
    tag.add_contact("c1");

    assert!(
        tag.remove_contact("c1"),
        "removing present contact reports true"
    );
    assert!(
        !tag.remove_contact("c1"),
        "removing absent contact reports false"
    );
    assert!(!tag.contains("c1"));
    assert!(tag.contact_ids.is_empty());
}
