// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for faceted contact search (ADR-051): default name-only vs opt-in
//! matching of tags, comment, place, and an exchange-time range — including the
//! driving "who I met Saturday night at that bar" query.

use vauchi_core::{Contact, ContactCard, SearchFacets, SymmetricKey, Vauchi};

fn setup() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

/// Adds an exchanged contact with a given display name and exchange time
/// (→ `acquired_at`); returns its id.
fn add_contact_at(wb: &Vauchi, name: &str, exchanged_at: u64) -> String {
    let mut pk = [0u8; 32];
    for (i, b) in name.bytes().enumerate().take(32) {
        pk[i] = b;
    }
    let contact = Contact::from_exchange(
        pk,
        ContactCard::new(name),
        SymmetricKey::generate(),
        exchanged_at,
    );
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

fn ids(contacts: Vec<Contact>) -> Vec<String> {
    let mut v: Vec<String> = contacts.into_iter().map(|c| c.id().to_string()).collect();
    v.sort();
    v
}

// @scenario: contact-annotations.feature - Default search matches name only
// @internal
#[test]
fn default_facets_match_name_only() {
    let wb = setup();
    let bob = add_contact_at(&wb, "Bob", 0);
    wb.add_tag_to_contact(&bob, "climbing").unwrap();

    // Facet-less faceted search ignores the tag — name only.
    let by_tag = wb
        .search_contacts_faceted("climbing", &SearchFacets::default())
        .unwrap();
    assert!(by_tag.is_empty(), "tag not matched without the tags facet");

    let by_name = wb
        .search_contacts_faceted("bob", &SearchFacets::default())
        .unwrap();
    assert_eq!(ids(by_name), vec![bob]);
}

// @scenario: contact-annotations.feature - Faceted search matches tags, comment, and place
// @internal
#[test]
fn tags_facet_matches_tag_names() {
    let wb = setup();
    let bob = add_contact_at(&wb, "Bob", 0);
    wb.add_tag_to_contact(&bob, "climbing-gym").unwrap();

    let facets = SearchFacets {
        tags: true,
        ..Default::default()
    };
    assert_eq!(
        ids(wb.search_contacts_faceted("climbing", &facets).unwrap()),
        vec![bob]
    );
}

// @scenario: contact-annotations.feature - Faceted search matches tags, comment, and place
// @internal
#[test]
fn comment_facet_matches_personal_notes() {
    let wb = setup();
    let bob = add_contact_at(&wb, "Bob", 0);
    wb.add_personal_note(&bob, "loves bouldering").unwrap();

    let facets = SearchFacets {
        comment: true,
        ..Default::default()
    };
    assert_eq!(
        ids(wb.search_contacts_faceted("bouldering", &facets).unwrap()),
        vec![bob]
    );
    // Without the comment facet, no match.
    assert!(
        wb.search_contacts_faceted("bouldering", &SearchFacets::default())
            .unwrap()
            .is_empty()
    );
}

// @scenario: contact-annotations.feature - Faceted search matches tags, comment, and place
// @internal
#[test]
fn place_facet_matches_named_place() {
    let wb = setup();
    let bob = add_contact_at(&wb, "Bob", 0);
    wb.set_exchange_location(&bob, 52.52, 13.405).unwrap();
    wb.name_exchange_place(&bob, "The Anchor Bar").unwrap();

    let facets = SearchFacets {
        place: true,
        ..Default::default()
    };
    assert_eq!(
        ids(wb.search_contacts_faceted("anchor", &facets).unwrap()),
        vec![bob]
    );
}

// @scenario: contact-annotations.feature - The driving query
// @internal
#[test]
fn saturday_night_at_that_bar() {
    let wb = setup();
    // Saturday window (inclusive epoch range).
    let sat_start = 1_700_000_000u64;
    let sat_end = 1_700_086_400u64; // +24h

    // Bob: met Saturday night at The Anchor Bar.
    let bob = add_contact_at(&wb, "Bob", 1_700_040_000);
    wb.set_exchange_location(&bob, 52.52, 13.405).unwrap();
    wb.name_exchange_place(&bob, "The Anchor Bar").unwrap();

    // Carol: met Tuesday at The Office.
    let carol = add_contact_at(&wb, "Carol", 1_699_000_000);
    wb.set_exchange_location(&carol, 48.0, 2.0).unwrap();
    wb.name_exchange_place(&carol, "The Office").unwrap();

    let facets = SearchFacets {
        place: true,
        time_range: Some((sat_start, sat_end)),
        ..Default::default()
    };
    let result = ids(wb.search_contacts_faceted("anchor", &facets).unwrap());
    assert_eq!(result, vec![bob], "only the Saturday-at-the-bar contact");
    assert!(!result.contains(&carol));
}

// @scenario: contact-annotations.feature - The driving query
// @internal
#[test]
fn time_range_filters_regardless_of_text() {
    let wb = setup();
    let recent = add_contact_at(&wb, "Recent", 2_000);
    let _old = add_contact_at(&wb, "Old", 1_000);

    // Empty query + time range → everyone acquired in the window.
    let facets = SearchFacets {
        time_range: Some((1_500, 3_000)),
        ..Default::default()
    };
    assert_eq!(
        ids(wb.search_contacts_faceted("", &facets).unwrap()),
        vec![recent]
    );
}
