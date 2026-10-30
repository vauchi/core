// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A refused field value must reach the shell as the input's validation
//! error through the generic command/event protocol the shells use, not
//! only through the legacy `handle_action` path.

use crate::event_path_support::{action, activate, first_input, row, validation_errors};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::{BindingId, Command, Event, InputValue, SurfaceId};

struct SavedForm {
    engine: AppEngine,
    saved: Vec<Command>,
    value_input: (SurfaceId, BindingId),
}

/// Opens Add Entry > Email from My Card, types `value` and presses Save.
fn save_email(value: &str) -> SavedForm {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(vauchi_core::contact_card::ContactField::new(
            vauchi_core::contact_card::FieldType::Email,
            "Work",
            "alice@example.com",
            0,
        ))
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let commands = engine.initial_commands().unwrap();
    let (surface_id, more) = action(&commands, "Actions").expect("Actions");
    let menu = activate(&mut engine, surface_id, &more);
    let (surface_id, add) = action(&menu, "Add Entry").expect("Add Entry");
    let commands = activate(&mut engine, surface_id, &add);
    let (surface_id, email) = row(&commands, "Email");
    let form = activate(&mut engine, surface_id, &email);
    let value_input = first_input(&form);
    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id: value_input.0.clone(),
            binding_id: value_input.1.clone(),
            value: InputValue::Text(value.into()),
        })
        .unwrap();
    let (surface_id, save) = action(&typed, "Save")
        .or_else(|| action(&form, "Save"))
        .expect("Save");
    let saved = activate(&mut engine, surface_id, &save);
    SavedForm {
        engine,
        saved,
        value_input,
    }
}

// @scenario: contact_card_management :: Email validation
#[test]
fn an_invalid_email_on_the_add_entry_form_reaches_the_shell_as_the_input_error() {
    let SavedForm { saved, .. } = save_email("notanemail");

    assert_eq!(
        validation_errors(&saved),
        vec!["Please enter a valid email address".to_string()],
        "{saved:#?}"
    );
}

// @scenario: contact_card_management :: Email validation
#[test]
fn the_input_error_clears_once_the_value_is_typed_again() {
    let SavedForm {
        mut engine,
        value_input: (surface_id, binding_id),
        ..
    } = save_email("notanemail");

    let retyped = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("alice@work.example".into()),
        })
        .unwrap();

    assert_eq!(validation_errors(&retyped), Vec::<String>::new());
}

// iOS reports the field losing focus after Save re-renders the form; that
// is not a new value, so the error must stay until the value is typed again
// (vauchi/private#845, iPhone 2026-10-30: no message after an invalid Save).
// @scenario: contact_card_management :: Email validation
#[test]
fn the_input_error_survives_the_field_losing_focus() {
    let SavedForm {
        mut engine, saved, ..
    } = save_email("notanemail");
    let (surface_id, binding_id) = first_input(&saved);

    let unfocused = engine
        .dispatch(Event::InputFocusEnded {
            surface_id,
            binding_id,
        })
        .unwrap();

    assert_eq!(
        validation_errors(&unfocused),
        vec!["Please enter a valid email address".to_string()],
        "{unfocused:#?}"
    );
}

// iOS also re-sends the field's unchanged text right after Save; the same
// value is not new input, so the error must stay (vauchi/private#845,
// iPhone console 2026-10-30: ValueChanged 26 ms after the Save reply).
// @scenario: contact_card_management :: Email validation
#[test]
fn the_input_error_survives_the_same_value_sent_again() {
    let SavedForm {
        mut engine, saved, ..
    } = save_email("notanemail");
    let (surface_id, binding_id) = first_input(&saved);

    let resent = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("notanemail".into()),
        })
        .unwrap();

    assert_eq!(
        validation_errors(&resent),
        vec!["Please enter a valid email address".to_string()],
        "{resent:#?}"
    );
}
