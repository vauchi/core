// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The onboarding steps that ask the user to choose something show
//! those choices in the body
//! (`2026-08-07-onboarding-steps-render-empty-bodies`).
//!
//! "Add contact info" and "What would you like to do?" used to render an
//! empty body, with every choice collapsed into the context bar's
//! overflow — a first-run user saw a step titled "Add contact info" and
//! no visible way to add contact info. The choices now also ship as an
//! `ActionList` whose item ids are the same action ids the context bar
//! dispatches, so tapping a row and pressing the overflow entry are one
//! path through the engine.

use vauchi_app::ui::{
    ActionResult, Component, OnboardingEngine, PostOnboardingDestination, ScreenModel, UserAction,
    WorkflowEngine,
};

const CONTACT_INFO_CHOICES: &str = "contact_info_choices";
const WHAT_NEXT_CHOICES: &str = "what_next_choices";

fn press(engine: &mut OnboardingEngine, action_id: &str) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    });
}

fn at_contact_info() -> OnboardingEngine {
    let mut engine = OnboardingEngine::new();
    press(&mut engine, "create_new");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Ada".into(),
    });
    press(&mut engine, "continue");
    press(&mut engine, "skip");
    assert_eq!(
        engine.current_screen().screen_id,
        "contact_info",
        "the fixture must land on the contact-info step"
    );
    engine
}

fn at_what_next() -> OnboardingEngine {
    let mut engine = at_contact_info();
    press(&mut engine, "skip");
    assert_eq!(
        engine.current_screen().screen_id,
        "what_next",
        "the fixture must land on the final step"
    );
    engine
}

fn choice_ids(screen: &ScreenModel, list_id: &str) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == list_id => {
                Some(items.iter().map(|item| item.id.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ActionList `{list_id}` on {}", screen.screen_id))
}

fn choice_label(screen: &ScreenModel, list_id: &str, item_id: &str) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == list_id => items
                .iter()
                .find(|item| item.id == item_id)
                .map(|item| item.label.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no choice `{item_id}` in `{list_id}`"))
}

fn contextual_label(screen: &ScreenModel, action_id: &str) -> String {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == action_id)
        .unwrap_or_else(|| panic!("no contextual action `{action_id}`"))
        .label
        .clone()
}

// @internal
#[test]
fn add_contact_info_step_lists_its_add_field_choices_in_the_body() {
    let engine = at_contact_info();
    let screen = engine.current_screen();

    assert_eq!(
        choice_ids(&screen, CONTACT_INFO_CHOICES),
        vec!["show_phone", "show_email", "add_social", "skip"]
    );
    for id in ["show_phone", "show_email", "add_social", "skip"] {
        assert_eq!(
            choice_label(&screen, CONTACT_INFO_CHOICES, id),
            contextual_label(&screen, id),
            "body choice `{id}` and its overflow entry must read the same"
        );
    }
}

// @internal
#[test]
fn selecting_a_body_choice_reveals_the_matching_input() {
    let mut engine = at_contact_info();

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: CONTACT_INFO_CHOICES.into(),
        item_id: "show_phone".into(),
    });
    let screen = engine.current_screen();

    let phone_input_shown = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::TextInput { id, .. } if id == "phone_input"));
    assert!(phone_input_shown, "phone input appears once chosen");
    assert_eq!(
        choice_ids(&screen, CONTACT_INFO_CHOICES),
        vec!["show_email", "add_social", "skip"],
        "a revealed input drops out of the choices"
    );
}

// @internal
#[test]
fn done_step_lists_exchange_and_import_as_body_choices() {
    let engine = at_what_next();
    let screen = engine.current_screen();

    assert_eq!(
        choice_ids(&screen, WHAT_NEXT_CHOICES),
        vec!["exchange", "import_contacts"]
    );
    for id in ["exchange", "import_contacts"] {
        assert_eq!(
            choice_label(&screen, WHAT_NEXT_CHOICES, id),
            contextual_label(&screen, id),
            "body choice `{id}` and its overflow entry must read the same"
        );
    }
}

// @internal
#[test]
fn selecting_a_done_step_body_choice_completes_onboarding_there() {
    let mut engine = at_what_next();

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: WHAT_NEXT_CHOICES.into(),
        item_id: "import_contacts".into(),
    });

    let ActionResult::OnboardingComplete { destination } = result else {
        panic!("choosing import from the body completes onboarding, got {result:?}");
    };
    assert_eq!(destination, PostOnboardingDestination::ImportContacts);
}
