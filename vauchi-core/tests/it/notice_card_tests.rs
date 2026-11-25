// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cohort body is authenticated before a contact card is installed.
#![cfg(feature = "notice-core")]

use vauchi_core::card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::notice::card::{CardError, SignedCard};

fn card() -> ContactCard {
    let mut card = ContactCard::new("Alice");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Email",
        "alice@example.org",
        0,
    ))
    .unwrap();
    card
}

// @internal
#[test]
fn a_signed_cohort_card_binds_the_owner_device_and_revision() {
    let owner = Identity::create("Alice", 0);
    let device = SigningKeyPair::generate();
    let bytes = SignedCard::sign(&card(), &owner, &device, 7).unwrap();
    let verified = SignedCard::verify(
        &bytes,
        owner.signing_public_key(),
        owner.device_id(),
        device.public_key().as_bytes(),
    )
    .unwrap();
    assert_eq!(verified.revision, 7);
    assert_eq!(verified.card.display_name(), "Alice");
    assert_eq!(verified.card.fields()[0].value(), "alice@example.org");
    let other = Identity::create("Mallory", 0);
    assert!(matches!(
        SignedCard::verify(
            &bytes,
            other.signing_public_key(),
            owner.device_id(),
            device.public_key().as_bytes()
        ),
        Err(CardError::Authentication)
    ));
    assert!(matches!(
        SignedCard::verify(
            &bytes,
            owner.signing_public_key(),
            other.device_id(),
            device.public_key().as_bytes()
        ),
        Err(CardError::Authentication)
    ));
    assert!(matches!(
        SignedCard::verify(
            &bytes,
            owner.signing_public_key(),
            owner.device_id(),
            other.signing_public_key()
        ),
        Err(CardError::Authentication)
    ));
}

// @internal
#[test]
fn changing_a_signed_cohort_revision_or_body_is_refused() {
    let owner = Identity::create("Alice", 0);
    let device = SigningKeyPair::generate();
    let bytes = SignedCard::sign(&card(), &owner, &device, 7).unwrap();
    for (field, value) in [
        ("revision", serde_json::json!(8)),
        ("card", serde_json::json!("{}")),
    ] {
        let mut changed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        changed[field] = value;
        let changed = serde_json::to_vec(&changed).unwrap();
        assert!(matches!(
            SignedCard::verify(
                &changed,
                owner.signing_public_key(),
                owner.device_id(),
                device.public_key().as_bytes()
            ),
            Err(CardError::Authentication)
        ));
    }
}

// @internal
#[test]
fn unknown_card_formats_and_oversized_bodies_are_refused() {
    let owner = Identity::create("Alice", 0);
    let device = SigningKeyPair::generate();
    let bytes = SignedCard::sign(&card(), &owner, &device, 7).unwrap();
    let mut changed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    changed["format"] = serde_json::json!(2);
    assert!(matches!(
        SignedCard::verify(
            &serde_json::to_vec(&changed).unwrap(),
            owner.signing_public_key(),
            owner.device_id(),
            device.public_key().as_bytes()
        ),
        Err(CardError::Format)
    ));
    let oversized = vec![0; vauchi_core::notice::content::MAX_BODY_BYTES + 1];
    assert!(matches!(
        SignedCard::verify(
            &oversized,
            owner.signing_public_key(),
            owner.device_id(),
            device.public_key().as_bytes()
        ),
        Err(CardError::BodySize)
    ));
}

// @internal
#[test]
fn signed_bodies_strip_private_notes_and_audience_metadata() {
    let owner = Identity::create("Alice", 0);
    let device = SigningKeyPair::generate();
    let mut card = card();
    let id = card.fields()[0].id().to_string();
    card.update_field_note(&id, Some("private-note".into()))
        .unwrap();
    card.field_visibility_mut()
        .set_contacts(&id, ["private-contact-id".to_string()].into());
    let bytes = SignedCard::sign(&card, &owner, &device, 1).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(!text.contains("private-note"));
    assert!(!text.contains("private-contact-id"));
    let verified = SignedCard::verify(
        &bytes,
        owner.signing_public_key(),
        owner.device_id(),
        device.public_key().as_bytes(),
    )
    .unwrap();
    assert_eq!(verified.card.fields()[0].note(), None);
    assert!(verified.card.field_visibility().is_empty());
}
