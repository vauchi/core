// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-wiring tests for `EmergencyBroadcastEngine` driven through
//! `AppEngine` (ADR-021 / ADR-043 / CC-24).
//!
//! The configure *behaviour* — selected recipients + a default message
//! persist via core, and an empty selection is rejected — is core's, not a
//! frontend's. The `contact_ids` step became a `ToggleList` picker over
//! `available_contacts` (`2026-07-03-coercion-safety-config-gaps`); before
//! this file the only coverage of the picker→persist path lived in a TUI
//! humble test, which the rework silently broke. These tests pin the
//! behaviour at the engine boundary.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::ImportSource;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;

fn engine_ready() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// Adds a contact and returns the id the picker will present it under.
fn add_contact(engine: &AppEngine, name: &str) -> String {
    let before = contact_ids(engine);
    let contact = Contact::from_import(ContactCard::new(name), ImportSource::VcardFile, None, 0);
    engine.vauchi().add_contact(contact).unwrap();
    contact_ids(engine)
        .into_iter()
        .find(|id| !before.contains(id))
        .expect("newly added contact id")
}

fn contact_ids(engine: &AppEngine) -> Vec<String> {
    engine
        .vauchi()
        .list_contacts()
        .unwrap()
        .iter()
        .map(|c| c.id().to_string())
        .collect()
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn toggle(engine: &mut AppEngine, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ItemToggled {
        component_id: "contact_ids".into(),
        item_id: item_id.into(),
    })
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

// @scenario: emergency_broadcast :: Configure emergency broadcast
#[test]
fn emergency_configure_persists_selected_contacts_and_default_message() {
    let mut engine = engine_ready();
    let bob = add_contact(&engine, "Bob");
    let carol = add_contact(&engine, "Carol");
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let _ = press(&mut engine, "configure");
    let _ = toggle(&mut engine, &bob);
    let _ = toggle(&mut engine, &carol);
    let _ = press(&mut engine, "continue"); // contacts → message (default pre-filled)
    let result = press(&mut engine, "save"); // message → complete → persist

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "save completes and navigates back, got {result:?}"
    );
    let cfg = engine
        .vauchi()
        .load_emergency_config()
        .unwrap()
        .expect("emergency must be configured after the flow");
    assert_eq!(sorted(cfg.trusted_contact_ids), sorted(vec![bob, carol]));
    assert!(!cfg.message.is_empty(), "a default message is persisted");
}

// @internal — an empty selection is rejected at the contacts step; nothing
// advances and nothing persists.
#[test]
fn emergency_empty_selection_is_rejected() {
    let mut engine = engine_ready();
    add_contact(&engine, "Bob"); // pool non-empty; we select no-one
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let _ = press(&mut engine, "configure");
    let result = press(&mut engine, "continue");

    // An empty selection must not advance to the message step: the engine
    // re-renders the contacts picker (Humble contract — the rejection is
    // the returned screen, not a bare error to the frontend).
    let stayed_on_picker = matches!(
        &result,
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s)
            if s.components.iter().any(|c| matches!(
                c, Component::ToggleList { id, .. } if id == "contact_ids"
            ))
    );
    assert!(
        stayed_on_picker,
        "empty selection must stay on the contacts picker, got {result:?}"
    );
    assert!(
        engine.vauchi().load_emergency_config().unwrap().is_none(),
        "no emergency config may be persisted from a rejected flow"
    );
}
