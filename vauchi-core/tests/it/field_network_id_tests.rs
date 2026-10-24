// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A social field stores the id of its network, so renaming the network
//! in the data renames every card (vauchi/private#734 D4).

use vauchi_core::contact_card::{ContactField, FieldType, ValidationError};

fn github_field() -> ContactField {
    ContactField::new(FieldType::Social, "GitHub", "octocat", 0).with_network_id("github")
}

// @internal
#[test]
fn a_social_field_keeps_its_network_id_through_json() {
    let field = github_field();
    assert_eq!(field.network_id(), Some("github"));

    let json = serde_json::to_value(&field).unwrap();
    assert_eq!(json["network_id"], "github");
    let back: ContactField = serde_json::from_value(json).unwrap();
    assert_eq!(back.network_id(), Some("github"));
    assert_eq!(back.validate(), Ok(()));
}

// @internal
#[test]
fn a_field_without_a_network_id_keeps_its_json_shape() {
    let field = ContactField::new(FieldType::Social, "My blog", "me", 0);
    assert_eq!(field.network_id(), None);
    let json = serde_json::to_value(&field).unwrap();
    assert!(json.get("network_id").is_none(), "{json}");
}

// @internal
#[test]
fn a_non_social_field_never_has_a_network_id() {
    let phone =
        ContactField::new(FieldType::Phone, "Mobile", "+41790000000", 0).with_network_id("github");
    assert_eq!(phone.network_id(), None);
}

/// A peer's card is untrusted input: an id on a non-social field, or one
/// outside the registry's id charset, is rejected (DC-01).
// @internal
#[test]
fn a_received_field_with_an_invalid_network_id_is_rejected() {
    let received = |field_type: &str, network_id: &str| -> ContactField {
        serde_json::from_value(serde_json::json!({
            "id": "f1",
            "field_type": field_type,
            "label": "L",
            "value": if field_type == "Phone" { "+41790000000" } else { "v" },
            "network_id": network_id,
        }))
        .unwrap()
    };
    assert_eq!(
        received("Phone", "github").validate(),
        Err(ValidationError::InvalidNetworkId)
    );
    for bad in ["", "GitHub", "git hub", "a\0b", &"a".repeat(33), "ü"] {
        assert_eq!(
            received("Social", bad).validate(),
            Err(ValidationError::InvalidNetworkId),
            "{bad:?}"
        );
    }
    assert_eq!(received("Social", &"a".repeat(32)).validate(), Ok(()));
}

// @internal
#[test]
fn the_display_label_is_the_named_networks_current_name() {
    let registry = vauchi_core::social::SocialNetworkRegistry::from_data_bytes(
        br#"[{"id": "twitter", "name": "X", "url": "https://x.com/{username}"}]"#,
    )
    .unwrap();
    let field = |network_id: Option<&str>| {
        let f = ContactField::new(FieldType::Social, "Twitter", "alice", 0);
        match network_id {
            Some(id) => f.with_network_id(id),
            None => f,
        }
    };
    assert_eq!(field(Some("twitter")).display_label(&registry), "X");
    assert_eq!(field(Some("nope")).display_label(&registry), "Twitter");
    assert_eq!(field(None).display_label(&registry), "Twitter");
}

// Cards stored before network ids existed get one from their label when
// they load (vauchi/private#734 D6).

// @internal
#[test]
fn a_stored_own_card_loads_with_network_ids_from_its_labels() {
    let mut vauchi = vauchi_core::Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(ContactField::new(FieldType::Social, "Twitter", "alice", 0))
        .unwrap();
    vauchi
        .add_own_field(ContactField::new(FieldType::Social, "My blog", "me", 0))
        .unwrap();

    let card = vauchi.own_card().unwrap().unwrap();
    let id_of = |label: &str| {
        card.fields()
            .iter()
            .find(|f| f.label() == label)
            .map(|f| f.network_id().map(str::to_string))
            .expect("field present")
    };
    assert_eq!(id_of("Twitter").as_deref(), Some("twitter"));
    assert_eq!(id_of("My blog"), None);
}

// @internal
#[test]
fn a_stored_contact_card_loads_with_network_ids_from_its_labels() {
    use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey};
    let mut vauchi = vauchi_core::Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let mut card = ContactCard::new("Bob");
    card.add_field(ContactField::new(FieldType::Social, "GitHub", "bob", 0))
        .unwrap();
    let contact = Contact::from_exchange(
        *Identity::create("Bob", 0).signing_public_key(),
        card,
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let loaded = vauchi.get_contact(&contact_id).unwrap().unwrap();
    assert_eq!(loaded.card().fields()[0].network_id(), Some("github"));
}
