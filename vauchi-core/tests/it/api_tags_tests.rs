// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the `Vauchi` tag API (owner-private annotation vocabulary).
//!
//! Covers autocomplete-or-create, per-contact tag listing, suggestions, and
//! validation. See `ADR-051`.

use vauchi_core::Vauchi;
use vauchi_core::exchange::{X3DH, X3DHKeyPair};

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
