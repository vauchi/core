// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Schema Retro-Compatibility Tests (M10)
//!
//! Verifies that ContactCard serialization is backward and forward compatible
//! across schema versions. Regression fixtures prevent silent data loss.
//!
//! Reference: _private/docs/problems/2026-02-22-content-schema-retro-compatibility/

use image::{ImageBuffer, Rgba, RgbaImage};
use std::io::Cursor;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

/// Helper: create a small valid PNG for avatar tests.
fn test_avatar_png() -> Vec<u8> {
    let img: RgbaImage = ImageBuffer::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

// ============================================================
// V0 Legacy Data (no schema_version field)
// ============================================================

/// V0 fixture: ContactCard serialized WITHOUT schema_version.
/// This is the format from all app versions before schema versioning.
/// The v1 parser MUST accept this data without loss.
// Pre-versioning cards are not read back (vauchi/private#574).
// @internal
#[test]
fn a_v0_card_without_a_schema_version_is_refused() {
    let fixture = include_str!("../fixtures/content/contact_card_v0.json");

    let err = serde_json::from_str::<ContactCard>(fixture).unwrap_err();

    assert!(
        err.to_string().contains("schema_version"),
        "unexpected error: {err}"
    );
}

// ============================================================
// V1 Data (with schema_version field)
// ============================================================

// @scenario: schema_compat :: Current version roundtrips correctly
#[test]
fn test_v1_roundtrip_preserves_all_fields() {
    let mut card = ContactCard::new("Bob Roundtrip");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Home",
        "+41-44-111-2233",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "Personal",
        "bob@test.ch",
        0,
    ))
    .unwrap();
    card.set_avatar(test_avatar_png()).unwrap();

    let json = serde_json::to_string(&card).expect("serialize should succeed");
    let loaded: ContactCard = serde_json::from_str(&json).expect("deserialize should succeed");

    assert_eq!(loaded.display_name(), "Bob Roundtrip");
    let avatar = loaded.avatar().expect("avatar should be present");
    assert_eq!(
        &avatar[0..4],
        b"RIFF",
        "avatar should be WebP after normalization"
    );
    assert_eq!(loaded.fields().len(), 2);
    assert_eq!(loaded.schema_version(), 1, "new cards should be schema v1");
}

// @scenario: schema_compat :: schema_version is serialized
#[test]
fn test_schema_version_appears_in_json() {
    let card = ContactCard::new("Version Check");
    let json = serde_json::to_string(&card).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(
        value.get("schema_version").and_then(|v| v.as_u64()),
        Some(1),
        "serialized JSON must contain schema_version: 1"
    );
}

// ============================================================
// Forward Compatibility (unknown future versions)
// ============================================================

// @scenario: schema_compat :: Future version fields don't break parser
#[test]
fn test_future_fields_ignored_gracefully() {
    // Simulate a v2 card that has extra fields unknown to v1 parser.
    // serde(deny_unknown_fields) is NOT used, so this should succeed.
    let json = r#"{
        "schema_version": 2,
        "id": "aabb",
        "display_name": "Future Card",
        "fields": [],
        "future_field": "something new"
    }"#;

    let card: ContactCard =
        serde_json::from_str(json).expect("future fields should be silently ignored");
    assert_eq!(card.display_name(), "Future Card");
}

// ============================================================
// Proptest: Serialize-Deserialize Roundtrip
// ============================================================

mod proptest_compat {
    use proptest::prelude::*;
    use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};

    fn arb_field_type() -> impl Strategy<Value = FieldType> {
        prop_oneof![
            Just(FieldType::Phone),
            Just(FieldType::Email),
            Just(FieldType::Website),
            Just(FieldType::Address),
            Just(FieldType::Social),
            Just(FieldType::Custom),
        ]
    }

    fn arb_contact_field() -> impl Strategy<Value = ContactField> {
        (
            arb_field_type(),
            "[a-zA-Z ]{1,20}",
            "[a-zA-Z0-9@.+\\- ]{1,50}",
        )
            .prop_map(|(ft, label, value)| ContactField::new(ft, &label, &value, 0))
    }

    fn arb_contact_card() -> impl Strategy<Value = ContactCard> {
        (
            "[a-zA-Z ]{1,30}",
            proptest::collection::vec(arb_contact_field(), 0..5),
        )
            .prop_map(|(name, fields)| {
                let mut card = ContactCard::new(&name);
                for f in fields {
                    let _ = card.add_field(f); // ignore errors (e.g. dup birthday)
                }
                card
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        // @scenario: schema_compat :: Any valid ContactCard survives roundtrip
        #[test]
        fn roundtrip_preserves_card(card in arb_contact_card()) {
            let json = serde_json::to_vec(&card).expect("serialize");
            let loaded: ContactCard = serde_json::from_slice(&json).expect("deserialize");

            prop_assert_eq!(card.display_name(), loaded.display_name());
            prop_assert_eq!(card.fields().len(), loaded.fields().len());
            prop_assert_eq!(card.schema_version(), loaded.schema_version());

            for (orig, round) in card.fields().iter().zip(loaded.fields().iter()) {
                prop_assert_eq!(orig.label(), round.label());
                prop_assert_eq!(orig.value(), round.value());
                prop_assert_eq!(orig.field_type(), round.field_type());
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        // A card without its schema_version never parses (#574).
        // @internal
        #[test]
        fn a_card_without_a_schema_version_never_parses(card in arb_contact_card()) {
            let mut value: serde_json::Value = serde_json::to_value(&card).expect("serialize");
            value.as_object_mut().expect("object").remove("schema_version");

            prop_assert!(serde_json::from_value::<ContactCard>(value).is_err());
        }
    }
}
