// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn input_with_value(value: &Value, expected: &str) -> Option<Value> {
    match value {
        Value::Object(fields) => {
            if fields.contains_key("binding_id") && fields.get("value") == Some(&json!(expected)) {
                return Some(value.clone());
            }
            fields
                .values()
                .find_map(|value| input_with_value(value, expected))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| input_with_value(value, expected)),
        _ => None,
    }
}

// @scenario: core_file_picker :: FilePickedFromUser on Contacts imports vCard
#[test]
fn native_import_preserves_an_unsaved_editor() {
    let mut engine = engine();
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: vauchi_app::ui::FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });
    let commands = availability(&mut engine, true);
    let shown = menu(&commands);
    let input = input_with_value(&serde_json::to_value(&commands).unwrap(), "Alice").unwrap();
    engine
        .dispatch(
            serde_json::from_value(json!({"ValueChanged": {
                "surface_id": shown["surface_id"], "binding_id": input["binding_id"],
                "value": {"text": "Unsaved name"}
            }}))
            .unwrap(),
        )
        .unwrap();
    let shown = menu(&engine.initial_commands().unwrap());
    engine.dispatch(activation(&shown)).unwrap();
    let commands = engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "contacts.vcf".into(),
        })
        .unwrap();
    let input = input_with_value(&serde_json::to_value(commands).unwrap(), "Unsaved name").unwrap();
    assert_eq!(input["value"], "Unsaved name");
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 1);
    assert_eq!(
        engine.vauchi().own_card().unwrap().unwrap().display_name(),
        "Alice"
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn locking_invalidates_import_even_after_unlocking() {
    assert_lock_invalidates_import(AppScreen::Settings);
    assert_lock_invalidates_import(AppScreen::Contacts);
}

fn assert_lock_invalidates_import(destination: AppScreen) {
    let mut engine = engine();
    engine.set_device_capabilities(
        vauchi_core::exchange::capability::types::DeviceCapabilities {
            has_biometrics: true,
            ..Default::default()
        },
    );
    engine
        .vauchi_mut()
        .setup_app_password("app-password-123")
        .unwrap();
    let shown = menu(&availability(&mut engine, true));
    engine.dispatch(activation(&shown)).unwrap();
    engine.navigate_to(AppScreen::Lock);
    engine.initial_commands().unwrap();
    engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();
    engine.navigate_to(destination.clone());
    let unlocked = menu(&engine.initial_commands().unwrap());
    assert_eq!(unlocked["surface_id"], destination.screen_id());
    engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "late.vcf".into(),
        })
        .unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn contacts_does_not_import_without_an_outstanding_request() {
    let mut engine = engine();
    engine.navigate_to(AppScreen::Contacts);
    engine.initial_commands().unwrap();
    engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "unsolicited.vcf".into(),
        })
        .unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: core_file_picker :: import_contacts action emits FilePickFromUser
#[test]
fn native_import_dismisses_an_open_overlay_before_showing_the_picker() {
    let mut engine = engine();
    let commands = availability(&mut engine, true);
    let shown = menu(&commands);
    let navigation = commands
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar
                .navigation
                .as_ref()
                .map(|action| Event::ActionActivated {
                    surface_id: surface_id.clone(),
                    interaction_id: action.interaction_id.clone(),
                }),
            _ => None,
        })
        .unwrap();
    let opened = engine.dispatch(navigation).unwrap();
    assert_eq!(
        opened
            .iter()
            .filter(|c| matches!(c, Command::PresentOverlay { .. }))
            .count(),
        1
    );
    let commands = engine.dispatch(activation(&shown)).unwrap();
    let effects: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::DismissOverlay { .. } => Some("dismiss"),
            Command::FilePickFromUser { .. } => Some("pick"),
            _ => None,
        })
        .collect();
    assert_eq!(effects, vec!["dismiss", "pick"]);
}
