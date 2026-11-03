// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Turning on "Trust for recovery" asks which guardian group the contact
//! joins, once a group exists: one of the groups, listed with their
//! members, or a group of its own (vauchi/private#796, step 1g, D17). The
//! first guardian needs no question. Turning it off asks nothing.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey};

/// Alice and verified, exchanged contacts named `names`; returns their ids.
fn alice_with(names: &[&str]) -> (AppEngine, Vec<String>) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let ids = names
        .iter()
        .map(|name| {
            let peer = Identity::create(name, 0);
            let contact = Contact::from_exchange(
                *peer.signing_public_key(),
                ContactCard::new(name),
                SymmetricKey::generate(),
                0,
            );
            let id = contact.id().to_string();
            vauchi.add_contact(contact).unwrap();
            vauchi.verify_contact_fingerprint(&id).unwrap();
            id
        })
        .collect();
    (AppEngine::new(vauchi), ids)
}

fn open(engine: &mut AppEngine, contact_id: &str) -> ScreenModel {
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.into(),
    });
    engine.current_screen()
}

fn screen_of(result: ActionResult) -> ScreenModel {
    match result {
        ActionResult::UpdateScreen(screen) | ActionResult::NavigateTo(screen) => screen,
        other => panic!("expected a screen, got {other:?}"),
    }
}

fn toggle_recovery(engine: &mut AppEngine) -> ScreenModel {
    screen_of(engine.handle_action(UserAction::SettingsToggled {
        component_id: "recovery_permissions".into(),
        item_id: "recovery_trusted".into(),
    }))
}

fn pick(engine: &mut AppEngine, component_id: &str, item_id: &str) -> ScreenModel {
    screen_of(engine.handle_action(UserAction::ListItemSelected {
        component_id: component_id.into(),
        item_id: item_id.into(),
    }))
}

/// The picker's rows as (id, label, detail), or `None` when it is closed.
fn picker(screen: &ScreenModel) -> Option<Vec<(String, String, Option<String>)>> {
    screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::ActionList { id, items } if id == "guardian_group" => Some(
                items
                    .iter()
                    .map(|item| (item.id.clone(), item.label.clone(), item.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
}

fn group_of(engine: &AppEngine, contact_id: &str) -> Option<u8> {
    engine
        .vauchi()
        .guardian_groups()
        .unwrap()
        .into_iter()
        .find(|(_, members)| members.iter().any(|member| member == contact_id))
        .map(|(group, _)| group)
}

// @internal
#[test]
fn the_first_guardian_is_trusted_at_once_in_a_group_of_its_own() {
    let (mut engine, ids) = alice_with(&["Bob"]);
    open(&mut engine, &ids[0]);

    let screen = toggle_recovery(&mut engine);

    assert_eq!(picker(&screen), None);
    assert_eq!(group_of(&engine, &ids[0]), Some(1));
}

// @internal
#[test]
fn a_later_guardian_is_asked_which_group_listing_each_with_its_members() {
    let (mut engine, ids) = alice_with(&["Bob", "Carol"]);
    open(&mut engine, &ids[0]);
    toggle_recovery(&mut engine);
    open(&mut engine, &ids[1]);

    let screen = toggle_recovery(&mut engine);

    assert_eq!(
        picker(&screen),
        Some(vec![
            ("group:1".into(), "Group 1".into(), Some("Bob".into())),
            (
                "new_group".into(),
                "A new group".into(),
                Some("Only Carol".into())
            ),
        ])
    );
    assert_eq!(group_of(&engine, &ids[1]), None, "nothing chosen yet");
}

// @internal
#[test]
fn picking_a_group_trusts_the_contact_in_it_and_closes_the_question() {
    let (mut engine, ids) = alice_with(&["Bob", "Carol"]);
    open(&mut engine, &ids[0]);
    toggle_recovery(&mut engine);
    open(&mut engine, &ids[1]);
    toggle_recovery(&mut engine);

    let screen = pick(&mut engine, "guardian_group", "group:1");

    assert_eq!(picker(&screen), None);
    assert_eq!(group_of(&engine, &ids[1]), Some(1));
}

// @internal
#[test]
fn picking_a_new_group_puts_the_contact_in_a_group_of_its_own() {
    let (mut engine, ids) = alice_with(&["Bob", "Carol"]);
    open(&mut engine, &ids[0]);
    toggle_recovery(&mut engine);
    open(&mut engine, &ids[1]);
    toggle_recovery(&mut engine);

    pick(&mut engine, "guardian_group", "new_group");

    assert_eq!(group_of(&engine, &ids[1]), Some(2));
}

// @internal
#[test]
fn cancelling_the_question_trusts_nobody() {
    let (mut engine, ids) = alice_with(&["Bob", "Carol"]);
    open(&mut engine, &ids[0]);
    toggle_recovery(&mut engine);
    open(&mut engine, &ids[1]);
    toggle_recovery(&mut engine);

    let screen = pick(&mut engine, "guardian_group_cancel", "cancel");

    assert_eq!(picker(&screen), None);
    assert_eq!(group_of(&engine, &ids[1]), None);
    assert!(
        !engine
            .vauchi()
            .get_contact(&ids[1])
            .unwrap()
            .unwrap()
            .is_recovery_trusted()
    );
}

// @internal
#[test]
fn a_group_row_that_does_not_exist_trusts_nobody() {
    let (mut engine, ids) = alice_with(&["Bob", "Carol"]);
    open(&mut engine, &ids[0]);
    toggle_recovery(&mut engine);
    open(&mut engine, &ids[1]);
    toggle_recovery(&mut engine);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "guardian_group".into(),
        item_id: "group:7".into(),
    });

    assert!(
        matches!(result, ActionResult::ValidationError { .. }),
        "{result:?}"
    );
    assert_eq!(group_of(&engine, &ids[1]), None);
}

// @internal
#[test]
fn turning_recovery_trust_off_asks_nothing_and_leaves_the_group() {
    let (mut engine, ids) = alice_with(&["Bob", "Carol"]);
    open(&mut engine, &ids[0]);
    toggle_recovery(&mut engine);
    open(&mut engine, &ids[1]);
    toggle_recovery(&mut engine);
    pick(&mut engine, "guardian_group", "group:1");

    let screen = toggle_recovery(&mut engine);

    assert_eq!(picker(&screen), None);
    assert_eq!(group_of(&engine, &ids[1]), None);
    assert_eq!(group_of(&engine, &ids[0]), Some(1));
}
