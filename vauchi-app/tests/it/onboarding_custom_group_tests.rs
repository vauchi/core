// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Committing a custom group during onboarding.
//!
//! Two people concluded this screen had no Add control
//! (`2026-08-13-custom-group-add-is-hidden-in-the-overflow`): it exists,
//! but as a secondary action the context bar collapses into an overflow,
//! so nothing on the visible surface says a name can be added.
//!
//! The commit itself already produced a selected chip and cleared the
//! field — what was missing were the two moments a user expects to
//! commit in: pressing Return, or leaving the field with text still in
//! it. Both arrive as generic observations (`InputSubmitted` /
//! `InputFocusEnded`) that Core interprets.

use vauchi_app::ui::{
    ActionStyle, Component, OnboardingEngine, ScreenModel, UserAction, WorkflowEngine,
};

const CUSTOM_GROUP_INPUT: &str = "custom_group";
const ADD_ACTION: &str = "submit_custom_group";
const CONTINUE_ACTION: &str = "continue";

fn at_groups_setup() -> OnboardingEngine {
    let mut engine = OnboardingEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Ada".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: CONTINUE_ACTION.into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "groups_setup",
        "the fixture must land on the groups screen"
    );
    engine
}

fn action_style(screen: &ScreenModel, id: &str) -> ActionStyle {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("no action {id} on {}", screen.screen_id))
        .style
        .clone()
}

fn group_names(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList { items, .. } => {
                Some(items.iter().map(|i| i.label.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn input_value(screen: &ScreenModel) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::TextInput { id, value, .. } if id == CUSTOM_GROUP_INPUT => {
                Some(value.clone())
            }
            _ => None,
        })
        .expect("the custom-group field must be on the screen")
}

// @scenario: onboarding :: Return commits a custom group
#[test]
fn pressing_return_in_the_field_commits_the_group_and_clears_it() {
    let mut engine = at_groups_setup();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: CUSTOM_GROUP_INPUT.into(),
        value: "Book club".into(),
    });

    let _ = engine.handle_action(UserAction::TextSubmitted {
        component_id: CUSTOM_GROUP_INPUT.into(),
    });

    let screen = engine.current_screen();
    assert!(
        group_names(&screen).iter().any(|n| n == "Book club"),
        "the committed name must appear beside the suggested groups, which \
         is what tells the user it took: {:?}",
        group_names(&screen)
    );
    assert_eq!(
        input_value(&screen),
        "",
        "the field must clear so a second group can be typed straight away"
    );
}

// @scenario: onboarding :: leaving the field offers a visible way to commit
#[test]
fn leaving_the_field_with_text_promotes_add_onto_the_bar() {
    let mut engine = at_groups_setup();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: CUSTOM_GROUP_INPUT.into(),
        value: "Book club".into(),
    });

    // While typing, Return is the expected commit, so the bar keeps
    // Continue as its primary and Add stays in the overflow.
    let typing = engine.current_screen();
    assert_eq!(action_style(&typing, ADD_ACTION), ActionStyle::Secondary);
    assert_eq!(action_style(&typing, CONTINUE_ACTION), ActionStyle::Primary);

    let _ = engine.handle_action(UserAction::TextFocusEnded {
        component_id: CUSTOM_GROUP_INPUT.into(),
    });

    // Focus left with the text uncommitted: the most likely next act is
    // adding it, so Add takes the bar rather than staying collapsed.
    let blurred = engine.current_screen();
    assert_eq!(
        action_style(&blurred, ADD_ACTION),
        ActionStyle::Primary,
        "leaving the field must surface a way to commit, not hide it in an \
         overflow the user has already been shown not to find"
    );
    assert_eq!(
        action_style(&blurred, CONTINUE_ACTION),
        ActionStyle::Secondary
    );
}

// @scenario: onboarding :: an empty field never promotes Add
#[test]
fn leaving_an_empty_field_changes_nothing() {
    let mut engine = at_groups_setup();
    let _ = engine.handle_action(UserAction::TextFocusEnded {
        component_id: CUSTOM_GROUP_INPUT.into(),
    });

    let screen = engine.current_screen();
    assert_eq!(action_style(&screen, ADD_ACTION), ActionStyle::Secondary);
    assert_eq!(action_style(&screen, CONTINUE_ACTION), ActionStyle::Primary);
}

// @scenario: onboarding :: a second custom group can follow the first
#[test]
fn a_committed_group_leaves_the_screen_ready_for_the_next() {
    let mut engine = at_groups_setup();
    for name in ["Book club", "Choir"] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: CUSTOM_GROUP_INPUT.into(),
            value: name.into(),
        });
        let _ = engine.handle_action(UserAction::TextSubmitted {
            component_id: CUSTOM_GROUP_INPUT.into(),
        });
    }

    let screen = engine.current_screen();
    let names = group_names(&screen);
    assert!(
        names.iter().any(|n| n == "Book club") && names.iter().any(|n| n == "Choir"),
        "both custom groups must survive, so adding one does not end the \
         opportunity to add another: {names:?}"
    );
    assert_eq!(
        action_style(&screen, ADD_ACTION),
        ActionStyle::Secondary,
        "with nothing pending, the bar returns to Continue"
    );
}
