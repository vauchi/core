// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Under duress every export format holds the decoy book only, and the
//! flow looks exactly like a normal one (#712 D5, ADR-032).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::{Vauchi, import_encrypted};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{AuthMode, SymmetricKey};

const APP_PIN: &str = "app-password-123";
const DURESS_PIN: &str = "135790";
const EXPORT_PASSWORD: &str = "correct-horse-battery-staple";

/// Real contact Rita and decoy Dora; unlocked with the duress PIN and past
/// the export's password step, which the duress PIN confirms.
fn duress_format_choice() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let rita = Contact::from_exchange(
        [9u8; 32],
        ContactCard::new("RealRita"),
        SymmetricKey::generate(),
        0,
    );
    vauchi.add_contact(rita).unwrap();
    vauchi
        .add_decoy_contact("decoy-dora", "Dora", &ContactCard::new("Dora"))
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.vauchi_mut().setup_app_password(APP_PIN).unwrap();
    engine
        .vauchi_mut()
        .setup_duress_password(DURESS_PIN)
        .unwrap();
    assert_eq!(
        engine.vauchi_mut().authenticate(DURESS_PIN).unwrap(),
        AuthMode::Duress
    );
    engine.navigate_to(AppScreen::Privacy);
    let _ = press(&mut engine, "export");
    type_into(&mut engine, "pin", DURESS_PIN);
    let _ = press(&mut engine, "confirm_pin");
    assert_eq!(engine.current_screen().screen_id, "export_choose_format");
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

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn a_duress_vcard_export_holds_only_the_decoys() {
    let mut engine = duress_format_choice();
    let _ = press(&mut engine, "export_vcard");

    let ActionResult::VcardExportComplete { vcf } = press(&mut engine, "confirm_readable_export")
    else {
        panic!("the duress session exports a vCard like any other");
    };

    assert!(vcf.contains("FN:Dora"), "{vcf}");
    assert!(!vcf.contains("RealRita"), "a real contact leaked: {vcf}");
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn a_duress_encrypted_export_holds_only_the_decoys() {
    let mut engine = duress_format_choice();
    let _ = press(&mut engine, "export_encrypted");
    type_into(&mut engine, "export_password", EXPORT_PASSWORD);
    type_into(&mut engine, "export_password_confirm", EXPORT_PASSWORD);

    let ActionResult::EncryptedExportComplete { data } =
        press(&mut engine, "confirm_export_password")
    else {
        panic!("the duress session exports an encrypted file like any other");
    };

    let export = import_encrypted(&data, EXPORT_PASSWORD).unwrap();
    let json = serde_json::to_string(&export).unwrap();
    assert!(json.contains("Dora"), "{json}");
    assert!(!json.contains("RealRita"), "a real contact leaked: {json}");
}
