// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cohort body is authenticated before a contact card is installed.
#![cfg(feature = "notice-core")]

use vauchi_core::card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::notice::card::{CardError, SignedCard};
use vauchi_core::notice::groups::{Groups, VisibleCard};
use vauchi_core::notice::publish::Pending;

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

// @internal
#[test]
fn a_shared_group_publishes_an_authenticated_card_that_survives_a_reload() {
    let owner = Identity::create("Alice", 0);
    let device = SigningKeyPair::generate();
    let card = card();
    let mut groups = Groups::default();
    groups
        .reconcile(vec![
            VisibleCard::signed("bob".into(), card.clone(), &owner, &device, 3).unwrap(),
            VisibleCard::signed("carol".into(), card, &owner, &device, 3).unwrap(),
        ])
        .unwrap();
    assert_eq!(groups.publications().len(), 1);
    let mut groups = Groups::load(&groups.save().unwrap()).unwrap();
    let audience = groups.for_contact("bob").unwrap();
    assert_eq!(groups.for_contact("carol"), Some(audience.clone()));
    let publisher = &mut groups.publications_mut()[0];
    let now = 1_800_000_000;
    publisher.queue_due(now);
    let records = publisher.write(now, 8);
    let verify = |body: &[u8]| {
        SignedCard::verify(
            body,
            owner.signing_public_key(),
            owner.device_id(),
            device.public_key().as_bytes(),
        )
    };
    let mut pending = Pending::default();
    pending.note(&audience.update_key, &audience.commitment, now);
    let installed = pending.read(&records, now, |body| verify(body).is_ok());
    assert_eq!(installed.len(), 1);
    let verified = verify(&installed[0]).unwrap();
    assert_eq!(verified.revision, 3);
    assert_eq!(verified.card.fields()[0].value(), "alice@example.org");
    assert_eq!(
        groups.card_for_contact("bob").unwrap().fields()[0].value(),
        "alice@example.org"
    );
}

// @internal
#[test]
fn authentic_signatures_do_not_make_malformed_card_fields_acceptable() {
    let owner = Identity::create("Alice", 0);
    let device = SigningKeyPair::generate();
    let valid = serde_json::to_value(card()).unwrap();
    let mut cases = Vec::new();
    for (field, value) in [
        ("display_name", " ".to_string()),
        ("display_name", "x".repeat(101)),
        ("bio", "x".repeat(161)),
    ] {
        let mut changed = valid.clone();
        changed[field] = serde_json::json!(value);
        cases.push(changed);
    }
    let mut long_label = valid.clone();
    long_label["fields"][0]["label"] = serde_json::json!("x".repeat(65));
    cases.push(long_label);
    let mut duplicate = valid;
    let field = duplicate["fields"][0].clone();
    duplicate["fields"].as_array_mut().unwrap().push(field);
    cases.push(duplicate);
    for changed in cases {
        let card: ContactCard = serde_json::from_value(changed).unwrap();
        let bytes = SignedCard::sign(&card, &owner, &device, 1).unwrap();
        assert!(matches!(
            SignedCard::verify(
                &bytes,
                owner.signing_public_key(),
                owner.device_id(),
                device.public_key().as_bytes()
            ),
            Err(CardError::Malformed)
        ));
    }
}
