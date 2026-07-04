// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S3c+S3d (`2026-07-03-core-screens-bypass-i18n`, design D3.2): the
//! duress-PIN wizard and the backup/recovery wizard render in the user's
//! locale — the last two destructive/security confirmation families.
//! Exact German assertions per CC-03; keys in `resistance.duress.*` /
//! `backup.wizard.*` (locales!82, which also heals the placeholder-English
//! duress translations).

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    ActionResult, BackupRecoveryEngine, Component, DuressConfig, DuressPinEngine, ScreenModel,
    UserAction, WorkflowEngine,
};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn action_label(screen: &ScreenModel, id: &str) -> String {
    screen
        .actions
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("action {id} present"))
        .label
        .clone()
}

// @scenario: security :: duress PIN wizard renders in the active locale
// @internal
#[test]
fn duress_wizard_renders_german() {
    load_german();
    let mut engine = DuressPinEngine::new(DuressConfig::default(), Locale::German);

    let overview = engine.current_screen();
    assert_eq!(overview.title, "Duress-PIN");
    assert_eq!(action_label(&overview, "configure"), "PIN einrichten");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    let enter = engine.current_screen();
    assert_eq!(enter.title, "Duress-PIN festlegen");
    let Component::PinInput { label, .. } = &enter.components[0] else {
        panic!("enter step leads with the PIN input");
    };
    assert_eq!(label, "Duress-PIN eingeben");
    assert_eq!(action_label(&enter, "back"), "Zurück");

    // Empty PIN → localized validation.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("empty PIN must validation-error, got {result:?}");
    };
    assert_eq!(message, "Bitte geben Sie eine PIN ein");

    // Mismatched confirm → localized mismatch error.
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let confirm = engine.current_screen();
    assert_eq!(confirm.title, "Duress-PIN bestätigen");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirm_pin".into(),
        value: "654321".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("mismatched PINs must validation-error, got {result:?}");
    };
    assert_eq!(message, "PINs stimmen nicht überein");
}

// @scenario: security :: backup wizard renders in the active locale
// @internal
#[test]
fn backup_wizard_renders_german() {
    load_german();
    let mut engine = BackupRecoveryEngine::new(None, true, Locale::German);

    let choose = engine.current_screen();
    assert_eq!(choose.screen_id, "backup_choose");
    assert_eq!(choose.title, "Sicherung & Wiederherstellung");
    assert_eq!(action_label(&choose, "create"), "Sicherung erstellen");
    assert_eq!(
        action_label(&choose, "restore"),
        "Sicherung wiederherstellen"
    );

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create".into(),
    });
    let password = engine.current_screen();
    assert_eq!(password.screen_id, "backup_password");
    assert_eq!(password.title, "Sicherungspasswort");
    let Component::TextInput { label, .. } = &password.components[0] else {
        panic!("password step leads with the password input");
    };
    assert_eq!(label, "Wählen Sie ein Sicherungspasswort");
}

// English copy unchanged (regression pin for both wizards).
// @internal
#[test]
fn duress_and_backup_english_copy_unchanged() {
    let engine = DuressPinEngine::new(DuressConfig::default(), Locale::English);
    let overview = engine.current_screen();
    assert_eq!(overview.title, "Duress PIN");
    assert_eq!(action_label(&overview, "configure"), "Set Up PIN");

    let backup = BackupRecoveryEngine::new(None, true, Locale::English);
    let choose = backup.current_screen();
    assert_eq!(choose.title, "Backup & Recovery");
    assert_eq!(action_label(&choose, "create"), "Create Backup");
}
