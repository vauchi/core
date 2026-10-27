// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Fields the person adds to their own card, through the public API.

use crate::common::linked_peer::{LinkedPeer, link_contacts};
use vauchi_core::api::{Vauchi, VauchiConfig, process_single_card_update};
use vauchi_core::contact_card::field::MAX_VALUE_LENGTH;
use vauchi_core::contact_card::{ContactField, FieldType, validate_social_username};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::social::SocialNetworkRegistry;

fn alice() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
}

fn own_fields(vauchi: &Vauchi) -> Vec<(FieldType, String, String)> {
    vauchi
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .map(|f| (f.field_type(), f.label().to_string(), f.value().to_string()))
        .collect()
}

// @scenario: contact_card_management :: Add a physical address field
#[test]
fn an_address_field_is_saved_with_its_label_and_value() {
    let vauchi = alice();

    vauchi
        .add_own_field(ContactField::new(
            FieldType::Address,
            "Home",
            "123 Main St, City, ST 12345",
            0,
        ))
        .unwrap();

    assert_eq!(
        own_fields(&vauchi),
        vec![(
            FieldType::Address,
            "Home".to_string(),
            "123 Main St, City, ST 12345".to_string()
        )]
    );
}

// @scenario: contact_card_management :: Add a custom field
#[test]
fn a_custom_field_is_saved_with_its_label_and_value() {
    let vauchi = alice();

    vauchi
        .add_own_field(ContactField::new(
            FieldType::Custom,
            "Signal",
            "+1-555-987-6543",
            0,
        ))
        .unwrap();

    assert_eq!(
        own_fields(&vauchi),
        vec![(
            FieldType::Custom,
            "Signal".to_string(),
            "+1-555-987-6543".to_string()
        )]
    );
}

// @scenario: contact_card_management :: Add unlisted social network as custom field
#[test]
fn a_network_missing_from_the_registry_is_kept_as_a_custom_field_without_username_rules() {
    let registry = SocialNetworkRegistry::with_defaults();
    assert!(registry.resolve("signal").is_none());
    // The bundled twitter rule refuses this value, so a custom field that
    // keeps it shows no username rule was applied.
    let value = "this_is_way_too_long!";
    assert!(validate_social_username(&registry, "twitter", value).is_err());
    let vauchi = alice();

    vauchi
        .add_own_field(ContactField::new(FieldType::Custom, "Signal", value, 0))
        .unwrap();

    assert_eq!(
        own_fields(&vauchi),
        vec![(FieldType::Custom, "Signal".to_string(), value.to_string())]
    );
}

// @scenario: contact_card_management :: Field value size limit
#[test]
fn a_value_over_the_size_limit_is_refused_and_not_saved() {
    let vauchi = alice();
    let oversized = "x".repeat(MAX_VALUE_LENGTH + 1);

    let error = vauchi
        .add_own_field(ContactField::new(FieldType::Custom, "Note", &oversized, 0))
        .unwrap_err();

    assert!(
        error.to_string().contains(&format!(
            "Value too long (max {MAX_VALUE_LENGTH} characters)"
        )),
        "got {error}"
    );
    assert_eq!(own_fields(&vauchi), Vec::new());
}

// @scenario: contact_card_management :: Field order persists after restart
#[test]
fn a_reordered_card_keeps_its_order_when_the_app_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let open = || {
        Vauchi::new(
            VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
                .with_storage_key(key.clone()),
        )
        .unwrap()
    };
    let mut vauchi = open();
    vauchi.create_identity("Alice").unwrap();
    for (field_type, label, value) in [
        (FieldType::Phone, "Mobile", "+41 79 123 45 67"),
        (FieldType::Email, "Email", "alice@example.com"),
        (FieldType::Social, "Twitter", "alicesmith"),
    ] {
        vauchi
            .add_own_field(ContactField::new(field_type, label, value, 0))
            .unwrap();
    }
    let mut card = vauchi.own_card().unwrap().unwrap();
    let id_of = |label: &str| {
        card.fields()
            .iter()
            .find(|f| f.label() == label)
            .unwrap()
            .id()
            .to_string()
    };
    let order = [id_of("Twitter"), id_of("Mobile"), id_of("Email")];
    card.reorder_fields(&order.iter().map(String::as_str).collect::<Vec<_>>())
        .unwrap();
    vauchi.update_own_card(&card).unwrap();
    drop(vauchi);

    let reopened = open();

    let labels: Vec<String> = own_fields(&reopened)
        .into_iter()
        .map(|(_, label, _)| label)
        .collect();
    assert_eq!(labels, ["Twitter", "Mobile", "Email"]);
}

/// Delivers every update `sender` has queued for `recipient` and returns the
/// labels `recipient` then holds on the sender's card.
fn deliver_owed(sender: &Vauchi, recipient: &Vauchi, linked: &LinkedPeer) -> Vec<String> {
    sender.run_owed_repropagation().unwrap();
    let pending = sender
        .storage()
        .pending()
        .get_pending_updates(&linked.host_contact_id)
        .unwrap();
    for update in pending {
        process_single_card_update(
            recipient.identity().unwrap(),
            recipient.storage(),
            &linked.peer_contact_id,
            &update.payload,
        )
        .unwrap();
        sender
            .storage()
            .pending()
            .mark_update_sent(&update.id)
            .unwrap();
    }
    recipient
        .get_contact(&linked.peer_contact_id)
        .unwrap()
        .unwrap()
        .card()
        .fields()
        .iter()
        .map(|f| f.label().to_string())
        .collect()
}

// @scenario: contact_card_management :: Remove field updates contacts
#[test]
fn a_removed_field_disappears_from_the_card_a_contact_holds() {
    let bob = alice();
    let alice = alice();
    let linked = link_contacts(&bob, &alice, "Alice");
    alice
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 123 45 67",
            0,
        ))
        .unwrap();
    let mobile = alice.own_card().unwrap().unwrap().fields()[0]
        .id()
        .to_string();
    alice.set_own_field_public(&mobile).unwrap();
    assert_eq!(deliver_owed(&alice, &bob, &linked), ["Mobile"]);

    assert!(alice.remove_own_field("Mobile").unwrap());

    assert_eq!(deliver_owed(&alice, &bob, &linked), Vec::<String>::new());
}
