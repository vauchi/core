// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validation failures on the add/edit field forms must surface
//! localized validation copy inline — never the nested `Display` chain
//! of the underlying error types (verification finding TUI-12/QT-5,
//! ADR-045 Amendment 1).
//!
//! The engine resolves `ValidationError` into an `UpdateScreen` carrying
//! the message on the offending component, so a shell never receives
//! `ValidationError` and never patches the `ScreenModel` itself
//! (`resolve_validation_error`, ADR-066 "commands are complete"). These
//! tests therefore assert the prepared screen, not the internal variant.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, FormDialogType, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};

fn act(engine: &mut AppEngine, action: UserAction) {
    let _ = engine.handle_action(action);
}

fn engine_on_my_info() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Ana").expect("identity created");
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::MyInfo);
    engine
}

/// Open the add-field form from MyInfo, pick an entry type, and type a
/// value. Stops before save.
fn fill_add_field_form(engine: &mut AppEngine, entry_type: &str, value: &str) {
    act(
        engine,
        UserAction::ActionPressed {
            action_id: "add_field".into(),
        },
    );
    act(
        engine,
        UserAction::ListItemSelected {
            component_id: "entry_types".into(),
            item_id: entry_type.into(),
        },
    );
    act(
        engine,
        UserAction::TextChanged {
            component_id: "field_value".into(),
            value: value.into(),
        },
    );
}

fn submit(engine: &mut AppEngine) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    })
}

/// The localized validation message the prepared screen carries for
/// `component_id`, or a panic describing what came back instead.
fn inline_validation_error(result: ActionResult, component_id: &str) -> String {
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("expected an updated screen, got {result:?}");
    };
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::TextInput {
                id,
                validation_error: Some(message),
                ..
            } if id == component_id => Some(message.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "no validation error on {component_id}: {:?}",
                screen.components
            )
        })
}

// @scenario: contact_card_management.feature :: Phone number validation
#[test]
fn invalid_phone_value_reports_localized_message_inline() {
    let mut engine = engine_on_my_info();
    fill_add_field_form(&mut engine, "phone", "not-a-phone");

    let result = submit(&mut engine);

    let message = inline_validation_error(result, "field_value");
    assert_eq!(message, "Please enter a valid phone number");
}

// @scenario: contact_card_management.feature :: Email validation
#[test]
fn invalid_email_value_reports_localized_message_inline() {
    let mut engine = engine_on_my_info();
    fill_add_field_form(&mut engine, "email", "invalid-email");

    let result = submit(&mut engine);

    let message = inline_validation_error(result, "field_value");
    assert_eq!(message, "Please enter a valid email address");
}

// @scenario: contact_card_management.feature :: Phone number validation
#[test]
fn empty_value_reports_localized_message_inline() {
    let mut engine = engine_on_my_info();
    fill_add_field_form(&mut engine, "phone", "");

    let result = submit(&mut engine);

    let message = inline_validation_error(result, "field_value");
    assert_eq!(message, "Field cannot be empty");
}

// @scenario: contact_card_management.feature :: Edit an existing field value
#[test]
fn editing_a_field_to_an_invalid_value_reports_localized_message_inline() {
    let mut engine = engine_on_my_info();
    let field = ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+41 79 000 00 00",
        engine.vauchi().clock().unix_seconds(),
    );
    let field_id = field.id().to_string();
    engine
        .vauchi_mut()
        .add_own_field(field)
        .expect("valid field saves");

    let _ = engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditField {
            field_id,
            field_label: "Mobile".into(),
            current_value: "+41 79 000 00 00".into(),
            current_note: None,
        },
    });
    act(
        &mut engine,
        UserAction::TextChanged {
            component_id: "field_value".into(),
            value: "not-a-phone".into(),
        },
    );

    let result = submit(&mut engine);

    let message = inline_validation_error(result, "field_value");
    assert_eq!(message, "Please enter a valid phone number");
}
