// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A social entry is checked only against the username rule its network
//! carries in the loaded networks list (the CDN copy); Core holds no rule
//! of its own (vauchi/private#734).

use crate::event_path_support::{action, activate, first_input, row, validation_errors};
use vauchi_app::social::load_networks_from_bytes;
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event, InputValue};

/// Only in this list, so a rule it enforces can come from nowhere else.
const LIST_ONLY_NETWORK: &str = "Example Net";

fn load_list_with_a_five_character_rule() {
    load_networks_from_bytes(
        br#"[
            {"id": "examplenet", "name": "Example Net", "url": "https://example.net/{username}",
             "username": {"max_len": 5, "charset": "alnum_underscore", "strip_at": true}},
            {"id": "freenet", "name": "Free Net", "url": "https://free.example/{username}"}
        ]"#,
    )
    .expect("valid network data");
}

struct Saved {
    engine: AppEngine,
    commands: Vec<Command>,
}

/// Opens Add Entry > `network` from My Card, types `value` and presses Save.
fn save_social(network: &str, value: &str) -> Saved {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let my_card = engine.initial_commands().unwrap();
    let (surface_id, actions) = action(&my_card, "Actions").expect("Actions");
    let menu = activate(&mut engine, surface_id, &actions);
    let (surface_id, add) = action(&menu, "Add Entry").expect("Add Entry");
    let types = activate(&mut engine, surface_id, &add);
    let (surface_id, entry_type) = row(&types, network);
    let form = activate(&mut engine, surface_id, &entry_type);
    let (surface_id, binding_id) = first_input(&form);
    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text(value.into()),
        })
        .unwrap();
    let (surface_id, save) = action(&typed, "Save")
        .or_else(|| action(&form, "Save"))
        .expect("Save");
    let commands = activate(&mut engine, surface_id, &save);
    Saved { engine, commands }
}

fn own_values(engine: &AppEngine) -> Vec<String> {
    engine
        .vauchi()
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .map(|f| f.value().to_string())
        .collect()
}

// @scenario: contact_card_management :: Validate social network username format
#[test]
fn a_value_breaking_the_lists_rule_is_refused_with_the_generic_message() {
    load_list_with_a_five_character_rule();

    let Saved { engine, commands } = save_social(LIST_ONLY_NETWORK, "alice_smith");

    assert_eq!(
        validation_errors(&commands),
        vec!["Invalid format".to_string()]
    );
    assert_eq!(own_values(&engine), Vec::<String>::new());
}

// @scenario: contact_card_management :: Validate social network username format
#[test]
fn a_value_within_the_lists_rule_is_saved() {
    load_list_with_a_five_character_rule();

    let Saved { engine, commands } = save_social(LIST_ONLY_NETWORK, "@alice");

    assert_eq!(validation_errors(&commands), Vec::<String>::new());
    assert_eq!(own_values(&engine), vec!["@alice".to_string()]);
}

// @scenario: contact_card_management :: Validate social network username format
#[test]
fn a_network_listed_without_a_rule_accepts_any_value() {
    load_list_with_a_five_character_rule();

    let Saved { engine, .. } = save_social("Free Net", "anything goes, really");

    assert_eq!(
        own_values(&engine),
        vec!["anything goes, really".to_string()]
    );
}
