// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validation failures on the add/edit field forms must surface
//! localized validation copy inline — never the nested `Display` chain
//! of the underlying error types (verification finding TUI-12/QT-5,
//! ADR-045 Amendment 1).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, FormDialogType, UserAction, WorkflowEngine};
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

// @scenario: contact_card_management.feature :: Phone number validation
#[test]
fn invalid_phone_value_reports_localized_message_inline() {
    let mut engine = engine_on_my_info();
    fill_add_field_form(&mut engine, "phone", "not-a-phone");

    let result = submit(&mut engine);

    let ActionResult::ValidationError {
        component_id,
        message,
    } = result
    else {
        panic!("expected inline validation, got {result:?}");
    };
    assert_eq!(component_id, "field_value");
    assert_eq!(message, "Please enter a valid phone number");
}

// @scenario: contact_card_management.feature :: Email validation
#[test]
fn invalid_email_value_reports_localized_message_inline() {
    let mut engine = engine_on_my_info();
    fill_add_field_form(&mut engine, "email", "invalid-email");

    let result = submit(&mut engine);

    let ActionResult::ValidationError { message, .. } = result else {
        panic!("expected inline validation, got {result:?}");
    };
    assert_eq!(message, "Please enter a valid email address");
}

// @scenario: contact_card_management.feature :: Phone number validation
#[test]
fn empty_value_reports_localized_message_inline() {
    let mut engine = engine_on_my_info();
    fill_add_field_form(&mut engine, "phone", "");

    let result = submit(&mut engine);

    let ActionResult::ValidationError {
        component_id,
        message,
    } = result
    else {
        panic!("expected inline validation, got {result:?}");
    };
    assert_eq!(component_id, "field_value");
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

    let ActionResult::ValidationError {
        component_id,
        message,
    } = result
    else {
        panic!("expected inline validation, got {result:?}");
    };
    assert_eq!(component_id, "field_value");
    assert_eq!(message, "Please enter a valid phone number");
}
