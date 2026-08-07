// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S3c+S3d (`2026-07-03-core-screens-bypass-i18n`, design D3.2): the
//! duress-PIN wizard and the backup/recovery wizard render in the user's
//! locale — the last two destructive/security confirmation families.
//!
//! These assert that each screen *resolved a translation*, not what the
//! translation says. Verbatim copy assertions coupled core to a repo it
//! does not own: a register unification merged in `locales` on
//! 2026-08-07 reddened five test files across every core branch at once,
//! with no commit in core. Copy correctness belongs to `locales`, which
//! owns the schema, the quality gates and the CODEOWNERS for it. See
//! `problems/2026-08-07-locale-content-consumed-from-unpinned-head/`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, BackupRecoveryEngine, Component, DuressConfig, DuressPinEngine, UserAction,
    WorkflowEngine,
};

/// Copy a shell would show walking the duress wizard to its confirm step.
struct DuressCopy {
    overview_title: String,
    configure_action: String,
    enter_title: String,
    pin_label: String,
    back_action: String,
    empty_pin_error: String,
    confirm_title: String,
    mismatch_error: String,
}

fn walk_duress(locale: Locale) -> DuressCopy {
    let mut engine = DuressPinEngine::new(DuressConfig::default(), locale);

    let overview = engine.current_screen();
    let overview_title = overview.title.clone();
    let configure_action = action_label(&overview, "configure");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "configure".into(),
    });
    let enter = engine.current_screen();
    let enter_title = enter.title.clone();
    let Component::PinInput { label, .. } = &enter.components[0] else {
        panic!("enter step leads with the PIN input");
    };
    let pin_label = label.clone();
    let back_action = action_label(&enter, "back");

    // Empty PIN → localized validation.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::ValidationError { message, .. } = result else {
        panic!("empty PIN must validation-error, got {result:?}");
    };
    let empty_pin_error = message;

    // Mismatched confirm → localized mismatch error.
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: "123456".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let confirm_title = engine.current_screen().title.clone();
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

    DuressCopy {
        overview_title,
        configure_action,
        enter_title,
        pin_label,
        back_action,
        empty_pin_error,
        confirm_title,
        mismatch_error: message,
    }
}

/// Copy a shell would show walking the backup wizard to its password step.
struct BackupCopy {
    choose_screen_id: String,
    choose_title: String,
    create_action: String,
    restore_action: String,
    password_screen_id: String,
    password_title: String,
    password_label: String,
}

fn walk_backup(locale: Locale) -> BackupCopy {
    let mut engine = BackupRecoveryEngine::new(None, true, locale);

    let choose = engine.current_screen();
    let choose_screen_id = choose.screen_id.clone();
    let choose_title = choose.title.clone();
    let create_action = action_label(&choose, "create");
    let restore_action = action_label(&choose, "restore");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create".into(),
    });
    let password = engine.current_screen();
    let password_screen_id = password.screen_id.clone();
    let password_title = password.title.clone();
    let Component::TextInput { label, .. } = &password.components[0] else {
        panic!("password step leads with the password input");
    };

    BackupCopy {
        choose_screen_id,
        choose_title,
        create_action,
        restore_action,
        password_screen_id,
        password_title,
        password_label: label.clone(),
    }
}

// @scenario: security :: duress PIN wizard renders in the active locale
// @internal
#[test]
fn duress_wizard_renders_the_active_locale() {
    load_german();
    let de = walk_duress(Locale::German);
    let en = walk_duress(Locale::English);

    assert_translated("overview title", &de.overview_title, &en.overview_title);
    assert_translated(
        "configure action",
        &de.configure_action,
        &en.configure_action,
    );
    assert_translated("enter-step title", &de.enter_title, &en.enter_title);
    assert_translated("PIN input label", &de.pin_label, &en.pin_label);
    assert_translated("back action", &de.back_action, &en.back_action);
    assert_translated(
        "empty-PIN validation",
        &de.empty_pin_error,
        &en.empty_pin_error,
    );
    assert_translated("confirm-step title", &de.confirm_title, &en.confirm_title);
    assert_translated(
        "mismatch validation",
        &de.mismatch_error,
        &en.mismatch_error,
    );
}

// @scenario: security :: backup wizard renders in the active locale
// @internal
#[test]
fn backup_wizard_renders_the_active_locale() {
    load_german();
    let de = walk_backup(Locale::German);
    let en = walk_backup(Locale::English);

    // Screen ids are identifiers, not copy — they must NOT be translated.
    assert_eq!(de.choose_screen_id, "backup_choose");
    assert_eq!(de.password_screen_id, "backup_password");
    assert_eq!(de.choose_screen_id, en.choose_screen_id);
    assert_eq!(de.password_screen_id, en.password_screen_id);

    assert_translated("choose title", &de.choose_title, &en.choose_title);
    assert_translated("create action", &de.create_action, &en.create_action);
    assert_translated("restore action", &de.restore_action, &en.restore_action);
    assert_translated("password title", &de.password_title, &en.password_title);
    assert_translated(
        "password input label",
        &de.password_label,
        &en.password_label,
    );
}

// English is the source language and ships in this repo's bundled
// locale, so pinning it here couples nothing external.
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
