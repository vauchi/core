// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! After the password, the user picks how the data leaves the app: an
//! encrypted file under its own strong password, or — behind a warning that
//! the file is unprotected — every contact as vCard 4.0 or everything as JSON
//! (#712; owner decision 2026-10-20).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::api::gdpr::import_encrypted;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{AuthMode, SymmetricKey};

const APP_PIN: &str = "app-password-123";
const EXPORT_PASSWORD: &str = "correct-horse-battery-staple";

/// Alice, with Bob as her one contact, past the export's password step.
fn choosing_a_format() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let bob = Contact::from_exchange(
        [5u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    vauchi.add_contact(bob).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.vauchi_mut().setup_app_password(APP_PIN).unwrap();
    assert_eq!(
        engine.vauchi_mut().authenticate(APP_PIN).unwrap(),
        AuthMode::Normal
    );
    engine.navigate_to(AppScreen::Privacy);
    let _ = press(&mut engine, "export");
    type_into(&mut engine, "pin", APP_PIN);
    let _ = press(&mut engine, "confirm_pin");
    engine
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    });
}

fn screen_id(engine: &AppEngine) -> String {
    engine.current_screen().screen_id
}

fn action_ids(engine: &AppEngine) -> Vec<String> {
    engine
        .current_screen()
        .contextual_actions
        .into_iter()
        .map(|action| action.id)
        .collect()
}

fn set_export_password(engine: &mut AppEngine, password: &str, repeated: &str) -> ActionResult {
    type_into(engine, "export_password", password);
    type_into(engine, "export_password_confirm", repeated);
    press(engine, "confirm_export_password")
}

fn validation_error(engine: &AppEngine) -> Option<String> {
    engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::TextInput {
                validation_error: Some(error),
                ..
            } => Some(error),
            _ => None,
        })
}

// @internal
#[test]
fn after_the_password_the_user_chooses_a_format_encrypted_first() {
    let engine = choosing_a_format();

    assert_eq!(screen_id(&engine), "export_choose_format");
    assert_eq!(
        action_ids(&engine),
        vec!["export_encrypted", "export_vcard", "export_json", "cancel"]
    );
}

// @internal
#[test]
fn the_encrypted_export_opens_only_with_its_own_password() {
    let mut engine = choosing_a_format();
    let _ = press(&mut engine, "export_encrypted");
    assert_eq!(screen_id(&engine), "export_password");

    let result = set_export_password(&mut engine, EXPORT_PASSWORD, EXPORT_PASSWORD);

    let ActionResult::EncryptedExportComplete { data } = result else {
        panic!("a strong, repeated password exports, got {result:?}");
    };
    let export = import_encrypted(&data, EXPORT_PASSWORD).expect("the file opens with it");
    assert_eq!(export.contacts.len(), 1);
    assert!(import_encrypted(&data, APP_PIN).is_err());
}

// @internal
#[test]
fn a_weak_or_mismatched_export_password_exports_nothing() {
    let mut engine = choosing_a_format();
    let _ = press(&mut engine, "export_encrypted");

    for (password, repeated) in [
        ("short", "short"),
        ("password123", "password123"),
        (EXPORT_PASSWORD, "correct-horse-battery-stapler"),
    ] {
        let result = set_export_password(&mut engine, password, repeated);
        assert!(
            !matches!(result, ActionResult::EncryptedExportComplete { .. }),
            "{password:?}/{repeated:?} must not export, got {result:?}"
        );
        assert_eq!(screen_id(&engine), "export_password");
        assert!(
            validation_error(&engine).is_some(),
            "{password:?}/{repeated:?} says why"
        );
    }
}

// @internal
#[test]
fn the_vcard_export_warns_then_writes_every_contact() {
    let mut engine = choosing_a_format();
    let _ = press(&mut engine, "export_vcard");
    assert_eq!(screen_id(&engine), "export_readable_warning");

    let result = press(&mut engine, "confirm_readable_export");

    let ActionResult::VcardExportComplete { vcf } = result else {
        panic!("confirming the warning exports the vCard, got {result:?}");
    };
    assert_eq!(vcf.matches("BEGIN:VCARD").count(), 1, "{vcf}");
    assert!(vcf.contains("VERSION:4.0"), "{vcf}");
    assert!(vcf.contains("FN:Bob"), "{vcf}");
}

// @internal
#[test]
fn the_json_export_warns_then_writes_everything() {
    let mut engine = choosing_a_format();
    let _ = press(&mut engine, "export_json");
    assert_eq!(screen_id(&engine), "export_readable_warning");

    let result = press(&mut engine, "confirm_readable_export");

    let ActionResult::GdprExportComplete { json } = result else {
        panic!("confirming the warning exports the JSON, got {result:?}");
    };
    assert!(json.contains("\"Alice\""), "{json}");
    assert!(json.contains("\"Bob\""), "{json}");
}

// @internal
#[test]
fn backing_out_of_the_warning_exports_nothing_and_the_next_export_asks_again() {
    let mut engine = choosing_a_format();
    let _ = press(&mut engine, "export_vcard");

    let result = press(&mut engine, "cancel");

    assert!(!matches!(result, ActionResult::VcardExportComplete { .. }));
    assert_eq!(screen_id(&engine), "privacy_settings");
    let _ = press(&mut engine, "export");
    assert_eq!(screen_id(&engine), "export_confirm_pin");
}
