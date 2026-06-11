// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Component::InlineConfirm` presses arrive from renderers in the
//! colon convention (`<component_id>:confirm` / `<component_id>:cancel`
//! — iOS `InlineConfirmView.swift`, Android `InlineConfirmComponent.kt`)
//! while engines match the canonical `confirm_<id>` / `cancel_<id>`.
//! `AppEngine::handle_action` must normalize the colon form, otherwise
//! every inline confirmation is a silent no-op and dirty forms become
//! trap screens (device-verified Samsung S7,
//! `2026-06-11-add-entry-form-cannot-be-exited`).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_on_dirty_add_field_confirm() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_field".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "add_field should open the form dialog, got {result:?}"
    );

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "test@example.com".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    let screen = match result {
        ActionResult::UpdateScreen(screen) => screen,
        other => panic!("dirty cancel should re-render with InlineConfirm, got {other:?}"),
    };
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InlineConfirm { id, .. } if id == "discard")),
        "dirty cancel should arm the discard InlineConfirm"
    );
    engine
}

// @scenario: contact_card.feature - Add field to own card
#[test]
fn colon_form_confirm_discard_leaves_the_form() {
    let mut engine = engine_on_dirty_add_field_confirm();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "discard:confirm".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "renderer-form discard confirm must leave the dialog, got {result:?}"
    );
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "confirmed discard returns to MyInfo"
    );
}

// @scenario: contact_card.feature - Add field to own card
#[test]
fn colon_form_cancel_discard_keeps_editing() {
    let mut engine = engine_on_dirty_add_field_confirm();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "discard:cancel".into(),
    });

    let screen = match result {
        ActionResult::UpdateScreen(screen) => screen,
        other => panic!("keep-editing should re-render the form, got {other:?}"),
    };
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InlineConfirm { .. })),
        "keep-editing dismisses the InlineConfirm"
    );
    assert!(
        matches!(*engine.current_app_screen(), AppScreen::FormDialog { .. }),
        "keep-editing stays on the form dialog"
    );
}

// @scenario: contact_card.feature - Add field to own card
#[test]
fn canonical_confirm_discard_still_works() {
    let mut engine = engine_on_dirty_add_field_confirm();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_discard".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "canonical confirm_discard must keep working, got {result:?}"
    );
}
