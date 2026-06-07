// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the `Vauchi` place + per-contact exchange-location API
//! (ADR-051): named-place vocabulary, proximity suggestion, recording a
//! contact's exchange location, and retroactive naming.

use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

const ANCHOR_LAT: f64 = 52.5200;
const ANCHOR_LON: f64 = 13.4050;

fn setup() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

/// Adds an exchanged contact with a name-derived key; returns its id.
fn add_contact(wb: &Vauchi, name: &str) -> String {
    let mut pk = [0u8; 32];
    for (i, b) in name.bytes().enumerate().take(32) {
        pk[i] = b;
    }
    let contact = Contact::from_exchange(pk, ContactCard::new(name), SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

// @scenario: contact-annotations.feature - Exchange captures coordinates
// @internal
#[test]
fn set_and_get_exchange_location() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");

    wb.set_exchange_location(&bob, ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    let loc = wb.exchange_location(&bob).unwrap().unwrap();
    assert!((loc.latitude - ANCHOR_LAT).abs() < 1e-9);
    assert!((loc.longitude - ANCHOR_LON).abs() < 1e-9);
    assert!(loc.place_id.is_none(), "unnamed until named");
}

// @scenario: contact-annotations.feature - Exchange captures coordinates
// @internal
#[test]
fn set_exchange_location_unknown_contact_errors() {
    let wb = setup();
    assert!(
        wb.set_exchange_location("nope", ANCHOR_LAT, ANCHOR_LON)
            .is_err()
    );
}

// @scenario: contact-annotations.feature - Name a place
// @internal
#[test]
fn name_exchange_place_creates_and_links() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    wb.set_exchange_location(&bob, ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    let place = wb.name_exchange_place(&bob, "The Anchor Bar").unwrap();
    assert_eq!(place.name, "The Anchor Bar");

    // The contact's location is now linked to the new place.
    let loc = wb.exchange_location(&bob).unwrap().unwrap();
    assert_eq!(loc.place_id.as_deref(), Some(place.id.as_str()));
    // And the place joined the vocabulary.
    assert_eq!(wb.list_places().unwrap().len(), 1);
}

// @scenario: contact-annotations.feature - Name a place and have it auto-suggest by proximity
// @internal
#[test]
fn name_exchange_place_reuses_existing_by_name() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    let carol = add_contact(&wb, "Carol");
    wb.set_exchange_location(&bob, ANCHOR_LAT, ANCHOR_LON)
        .unwrap();
    wb.set_exchange_location(&carol, ANCHOR_LAT + 0.0001, ANCHOR_LON)
        .unwrap();

    let first = wb.name_exchange_place(&bob, "The Anchor Bar").unwrap();
    let second = wb.name_exchange_place(&carol, "the anchor bar").unwrap();

    assert_eq!(first.id, second.id, "same vocabulary entry reused");
    assert_eq!(wb.list_places().unwrap().len(), 1, "no duplicate place");
}

// @scenario: contact-annotations.feature - Name a place
// @internal
#[test]
fn name_exchange_place_without_location_errors() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    assert!(
        wb.name_exchange_place(&bob, "Somewhere").is_err(),
        "cannot name a place when no location was recorded"
    );
}

// @scenario: contact-annotations.feature - Name a place and have it auto-suggest by proximity
// @internal
#[test]
fn suggest_place_near_returns_known_place() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    wb.set_exchange_location(&bob, ANCHOR_LAT, ANCHOR_LON)
        .unwrap();
    let place = wb.name_exchange_place(&bob, "The Anchor Bar").unwrap();

    // A later exchange ~20 m away should suggest the same place.
    let suggested = wb
        .suggest_place_near(ANCHOR_LAT + 0.00018, ANCHOR_LON)
        .unwrap();
    assert_eq!(suggested.map(|p| p.id), Some(place.id));

    // ~1 km away: no suggestion.
    assert!(
        wb.suggest_place_near(ANCHOR_LAT + 0.01, ANCHOR_LON)
            .unwrap()
            .is_none()
    );
}

// @scenario: contact-annotations.feature - Exchange captures coordinates
// @internal
#[test]
fn clear_exchange_location_removes_it() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    wb.set_exchange_location(&bob, ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    wb.clear_exchange_location(&bob).unwrap();
    assert!(wb.exchange_location(&bob).unwrap().is_none());
}

// @scenario: contact-annotations.feature - Tags are never shared (at-rest)
// @internal
#[test]
fn exchange_location_is_encrypted_at_rest() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob");
    // A recognisable longitude fragment to scan for.
    wb.set_exchange_location(&bob, 52.5200, 13.405099).unwrap();

    let raw: Vec<u8> = wb
        .storage()
        .connection()
        .query_row(
            "SELECT exchange_location_encrypted FROM contacts WHERE id = ?1",
            rusqlite::params![bob],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        !raw.windows(b"13.405099".len()).any(|w| w == b"13.405099"),
        "plaintext coordinates must not appear in the stored BLOB"
    );
}
