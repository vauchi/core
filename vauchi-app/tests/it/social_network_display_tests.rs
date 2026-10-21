// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A social field that names its network shows the network's current name,
//! so renaming the network in the data renames every card
//! (vauchi/private#734 D5).

use vauchi_app::social::load_networks_from_bytes;
use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

const HANDLE: &str = "alicesmith";

fn rename_twitter_to_x() {
    load_networks_from_bytes(
        br#"[{"id": "twitter", "name": "X", "url": "https://x.com/{username}"}]"#,
    )
    .expect("valid network data");
}

fn twitter_field(network_id: &str) -> ContactField {
    ContactField::new(FieldType::Social, "Twitter", HANDLE, 0).with_network_id(network_id)
}

/// The label rendered beside the field whose value is [`HANDLE`], wherever
/// on the screen it sits.
fn label_beside_handle(engine: &AppEngine) -> String {
    fn find(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Object(map) => {
                // Field lists pair value with label; My Card's entry list
                // shows the value as the row's name and the label beneath.
                for (value_key, label_key) in [("value", "label"), ("name", "subtitle")] {
                    if map.get(value_key).and_then(|v| v.as_str()) == Some(HANDLE)
                        && let Some(label) = map.get(label_key).and_then(|l| l.as_str())
                    {
                        return Some(label.to_string());
                    }
                }
                map.values().find_map(find)
            }
            serde_json::Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    let screen = serde_json::to_value(engine.current_screen()).unwrap();
    find(&screen).unwrap_or_else(|| panic!("no field with value {HANDLE}: {screen}"))
}

fn engine_with_own_field(field: ContactField) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.add_own_field(field).unwrap();
    AppEngine::new(vauchi)
}

// @scenario: contact_card_management :: Generate profile URL from social field
#[test]
fn own_card_shows_the_networks_current_name() {
    rename_twitter_to_x();
    let mut engine = engine_with_own_field(twitter_field("twitter"));
    let _ = engine.navigate_to(AppScreen::MyInfo);
    assert_eq!(label_beside_handle(&engine), "X");
}

// @scenario: contact_card_management :: Generate profile URL from social field
#[test]
fn an_unknown_network_id_shows_the_stored_label() {
    rename_twitter_to_x();
    let mut engine = engine_with_own_field(twitter_field("nope"));
    let _ = engine.navigate_to(AppScreen::MyInfo);
    assert_eq!(label_beside_handle(&engine), "Twitter");
}

// @scenario: contact_card_management :: Generate profile URL from social field
#[test]
fn contact_detail_shows_the_networks_current_name() {
    rename_twitter_to_x();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let mut card = ContactCard::new("Bob");
    card.add_field(twitter_field("twitter")).unwrap();
    let contact = Contact::from_exchange(
        *Identity::create("Bob", 0).signing_public_key(),
        card,
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    let mut engine = AppEngine::new(vauchi);

    let _ = engine.navigate_to(AppScreen::ContactDetail { contact_id });

    assert_eq!(label_beside_handle(&engine), "X");
}

// @scenario: contact_card_management :: Generate profile URL from social field
#[test]
fn exchange_preview_shows_the_networks_current_name() {
    rename_twitter_to_x();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let field = twitter_field("twitter");
    let field_id = field.id().to_string();
    vauchi.add_own_field(field).unwrap();
    let work = vauchi.create_group("Work").unwrap();
    let work_id = work.id().to_string();
    vauchi
        .set_group_field_visibility(&work_id, &field_id, true)
        .unwrap();
    let mut engine = AppEngine::new(vauchi);

    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: work_id,
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:fun".into(),
        item_id: "mode:tap_hover_shake".into(),
    });
    assert_eq!(engine.current_screen().screen_id, "exchange_field_preview");

    assert_eq!(label_beside_handle(&engine), "X");
}
