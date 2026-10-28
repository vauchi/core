// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Managing groups through the app: a cancelled delete keeps the group, a
//! rename onto a taken name is refused, and the first group explains what
//! happens to entries outside every group.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey};

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn group_names(engine: &AppEngine) -> Vec<String> {
    let mut names: Vec<String> = engine
        .vauchi()
        .list_groups()
        .unwrap()
        .iter()
        .map(|g| g.name().to_string())
        .collect();
    names.sort();
    names
}

// @scenario: visibility_labels :: Cancel label deletion
#[test]
fn cancelling_a_group_delete_keeps_the_group() {
    let mut engine = engine();
    let important = engine.vauchi().create_group("Important").unwrap();
    engine.navigate_to(AppScreen::GroupDetail {
        group_id: important.id().to_string(),
    });

    let _ = press(&mut engine, "delete_group");
    let _ = press(&mut engine, "cancel_delete_group");

    assert_eq!(group_names(&engine), ["Important"]);
}

// @scenario: visibility_labels :: Cannot rename to existing label name
#[test]
fn renaming_friends_to_family_is_refused_and_friends_keeps_its_name() {
    let mut engine = engine();
    let friends = engine.vauchi().create_group("Friends").unwrap();
    engine.vauchi().create_group("Family").unwrap();
    engine.navigate_to(AppScreen::GroupDetail {
        group_id: friends.id().to_string(),
    });

    let _ = press(&mut engine, "rename");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "group_name".into(),
        value: "Family".into(),
    });
    let result = press(&mut engine, "submit");

    let ActionResult::ShowAlert { message, .. } = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert!(message.contains("Family"), "{message}");
    assert_eq!(group_names(&engine), ["Family", "Friends"]);
}

// @scenario: visibility_labels :: Transition from no-group to groups mode
#[test]
fn the_first_group_keeps_a_shown_entry_shown_and_says_why() {
    let mut engine = engine();
    engine
        .vauchi()
        .add_own_field(ContactField::new(
            FieldType::Email,
            "Email",
            "alice@example.com",
            0,
        ))
        .unwrap();
    let email = engine.vauchi().own_card().unwrap().unwrap().fields()[0]
        .id()
        .to_string();
    engine.vauchi().set_field_shown(&email, true).unwrap();
    let bob = Identity::create("Bob", 0);
    engine
        .vauchi()
        .add_contact(Contact::from_exchange(
            *bob.signing_public_key(),
            ContactCard::new("Bob"),
            SymmetricKey::generate(),
            0,
        ))
        .unwrap();

    engine.vauchi().create_group("Family").unwrap();
    let screen = engine.navigate_to(AppScreen::Groups);

    assert!(
        engine
            .vauchi()
            .own_card()
            .unwrap()
            .unwrap()
            .is_field_shown(&email)
    );
    let texts: Vec<&str> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.contains(
            &"Entries you assign to a group are visible only to its members. \
              Entries outside every group keep their own Visible/Hidden toggle."
        ),
        "{texts:?}"
    );
}
