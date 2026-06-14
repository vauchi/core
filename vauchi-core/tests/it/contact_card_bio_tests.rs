// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `ContactCard.bio` — the ADR-054 D2 presentation field (Phase 2a).
//!
//! A short, shared self-description (≤160 chars), normalized like the display
//! name and optional-additive on the wire (old cards read `bio: None`).

use proptest::prelude::*;
use vauchi_core::contact_card::{ContactCard, ContactCardError, MAX_BIO_LENGTH};

// @internal
#[test]
fn set_and_get_bio() {
    let mut card = ContactCard::new("Alice");
    assert_eq!(card.bio(), None, "a new card has no bio");

    card.set_bio("Builder of privacy tools").unwrap();
    assert_eq!(card.bio(), Some("Builder of privacy tools"));
}

// @internal
#[test]
fn set_bio_trims_and_normalizes() {
    let mut card = ContactCard::new("Alice");
    card.set_bio("  hi there  ").unwrap();
    assert_eq!(
        card.bio(),
        Some("hi there"),
        "bio is trimmed (normalize_text)"
    );
}

// @internal
#[test]
fn empty_bio_clears() {
    let mut card = ContactCard::new("Alice");
    card.set_bio("something").unwrap();
    assert_eq!(card.bio(), Some("something"));

    card.set_bio("   ").unwrap();
    assert_eq!(card.bio(), None, "a whitespace-only bio clears to None");
}

// @internal
#[test]
fn bio_at_cap_ok_over_cap_errs_and_leaves_card_unchanged() {
    let mut card = ContactCard::new("Alice");

    let at_cap: String = "a".repeat(MAX_BIO_LENGTH);
    card.set_bio(&at_cap).unwrap();
    assert_eq!(
        card.bio().map(str::len),
        Some(MAX_BIO_LENGTH),
        "exactly the cap is ok"
    );

    let over_cap: String = "a".repeat(MAX_BIO_LENGTH + 1);
    let err = card.set_bio(&over_cap).unwrap_err();
    assert!(matches!(err, ContactCardError::BioTooLong));
    assert_eq!(
        card.bio().map(str::len),
        Some(MAX_BIO_LENGTH),
        "a rejected over-cap bio leaves the previous value unchanged"
    );
}

// @internal
#[test]
fn bio_cap_counts_chars_not_bytes() {
    let mut card = ContactCard::new("Alice");

    // 'é' (U+00E9) is one Unicode scalar but two UTF-8 bytes; 160 of them are
    // within the char cap even though they are 320 bytes.
    let multibyte: String = "é".repeat(MAX_BIO_LENGTH);
    card.set_bio(&multibyte).unwrap();
    assert_eq!(card.bio().map(|b| b.chars().count()), Some(MAX_BIO_LENGTH));

    let over: String = "é".repeat(MAX_BIO_LENGTH + 1);
    assert!(matches!(
        card.set_bio(&over).unwrap_err(),
        ContactCardError::BioTooLong
    ));
}

// @internal
#[test]
fn bio_absent_in_legacy_json_deserializes_none() {
    // A card serialized before `bio` existed (no `bio` key) must read as None.
    let json = r#"{
        "schema_version": 1,
        "id": "aabb",
        "display_name": "Legacy",
        "fields": []
    }"#;
    let card: ContactCard = serde_json::from_str(json).unwrap();
    assert_eq!(card.bio(), None, "absent bio key → None (back-compat)");
}

fn arb_bio() -> impl Strategy<Value = Option<String>> {
    prop::option::of(
        prop::collection::vec(prop::char::range('a', 'z'), 0..=MAX_BIO_LENGTH)
            .prop_map(|v| v.into_iter().collect::<String>()),
    )
}

proptest! {
    // @scenario: schema_compat :: bio survives a serde roundtrip
    #[test]
    fn bio_serde_roundtrip(bio in arb_bio()) {
        let mut card = ContactCard::new("Alice");
        if let Some(ref b) = bio {
            card.set_bio(b).unwrap();
        }
        let json = serde_json::to_string(&card).unwrap();
        let loaded: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(card.bio(), loaded.bio());
    }
}
