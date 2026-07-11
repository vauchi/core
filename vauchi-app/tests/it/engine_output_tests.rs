// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `WorkflowEngine::engine_output` — exact-value tests per variant.
//!
//! The typed engine→hub channel of
//! `2026-06-10-appengine-typed-engine-channel`. Each test drives the
//! engine to a state and asserts the precise `EngineOutput` snapshot
//! the hub will read (CC-03).

use vauchi_app::ui::{
    BackupFormSnapshot, Component, EngineOutput, InputType, OnboardingEngine, UserAction,
    WorkflowEngine,
};

// @scenario: onboarding.feature - Completing onboarding creates identity
#[test]
fn onboarding_output_carries_typed_display_name() {
    let mut engine = OnboardingEngine::new();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Ada".into(),
    });

    let Some(EngineOutput::Onboarding(snap)) = engine.engine_output() else {
        panic!("onboarding engine must expose Onboarding output");
    };
    assert_eq!(snap.data.display_name, "Ada");
    assert!(
        snap.data.selected_groups.iter().all(|g| !g.selected),
        "no group is selected before the user toggles one"
    );
    assert_eq!(snap.data.fields, vec![]);
    assert_eq!(snap.pending_backup, None);
}

// @scenario: backup.feature - Restoring from a backup blob
#[test]
fn backup_output_redacts_password_in_debug() {
    use vauchi_app::ui::{BackupMode, BackupRecoveryEngine};
    let engine = BackupRecoveryEngine::new(
        Some(BackupMode::Restore),
        true,
        vauchi_app::i18n::Locale::English,
    );

    let output = engine.engine_output().expect("backup engine output");
    let EngineOutput::Backup(ref snap) = output else {
        panic!("backup engine must expose Backup output");
    };
    assert_eq!(
        *snap,
        BackupFormSnapshot {
            restore_mode: true,
            restore_data: String::new(),
            password: String::new(),
            full_level: true,
        }
    );
    let debug = format!("{output:?}");
    assert!(
        debug.contains("<redacted>"),
        "Debug must redact the password field: {debug}"
    );
    assert!(
        !debug.contains("restore_data:"),
        "Debug must print restore_data length only: {debug}"
    );
}

// @scenario: lock.feature - Unlocking with the app password
#[test]
fn lock_output_carries_credential_and_redacts_debug() {
    use vauchi_app::ui::LockScreenEngine;
    let mut engine = LockScreenEngine::new(4);
    assert_eq!(
        engine.engine_output(),
        None,
        "empty entry must expose no output"
    );
    // A masked TextInput emits the full current value on each change
    // (standard text-field semantics), not one char per keystroke.
    for value in ["1", "12", "123", "1234"] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: value.into(),
        });
    }
    let output = engine.engine_output().expect("credential output");
    assert_eq!(output, EngineOutput::Lock { pin: "1234".into() });
    let debug = format!("{output:?}");
    assert!(
        !debug.contains("1234") && debug.contains("<redacted>"),
        "Debug must redact the credential: {debug}"
    );
}

// Regression for the P0 lockout bug (2026-07-03 GUI audit, ranked #1):
// the lock screen rendered a 6-slot numeric PinInput while ChangePassword
// accepts free-text up to 128 chars, so any non-6-digit password could
// never reach authenticate(). The unlock surface must be a masked
// free-text TextInput that accepts the whole credential unchanged.
// @scenario: lock.feature - Unlocking with a long alphanumeric password
#[test]
fn lock_screen_accepts_long_alphanumeric_password() {
    use vauchi_app::ui::LockScreenEngine;
    let mut engine = LockScreenEngine::new(5);

    // The rendered entry surface must be a masked password TextInput,
    // NOT a fixed-length PinInput (which caps + forces a numeric keypad).
    let screen = engine.current_screen();
    match screen.components.first().expect("an input component") {
        Component::TextInput { input_type, .. } => assert_eq!(
            *input_type,
            InputType::Password,
            "lock input must be a masked password field"
        ),
        other => panic!("lock screen must render a masked TextInput, got {other:?}"),
    }

    let password = "Tr0ub4dour&3!longphrase";
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: password.into(),
    });
    assert_eq!(
        engine.engine_output().expect("credential output"),
        EngineOutput::Lock {
            pin: password.into()
        },
        "the full free-text password must reach authenticate() unchanged"
    );
}

// @scenario: visibility.feature - Toggling per-contact field visibility
#[test]
fn contact_visibility_output_carries_typed_toggles() {
    use vauchi_app::ui::{ContactVisibilityEngine, ToggleItem};
    let toggle = |id: &str, label: &str, selected: bool| ToggleItem {
        id: id.into(),
        label: label.into(),
        selected,
        subtitle: None,
        a11y: None,
        info_key: None,
    };
    let engine = ContactVisibilityEngine::new(
        "Ada".into(),
        vec![toggle("f1", "Phone", true), toggle("f2", "Email", false)],
    );
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::ContactVisibility {
            toggles: vec![("f1".into(), true), ("f2".into(), false)],
        })
    );
}

// @scenario: my_info.feature - Editing the display name
#[test]
fn form_dialog_output_is_typed_per_dialog_kind() {
    use vauchi_app::ui::{FormDialogEngine, FormDialogType, FormInput};
    let mut engine = FormDialogEngine::new(FormDialogType::EditName {
        current_name: "Ada".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Grace".into(),
    });
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::Form(FormInput::EditName {
            name: "Grace".into()
        }))
    );
}

// @scenario: settings.feature - Changing the app password
#[test]
fn change_password_output_redacts_both_credentials() {
    use vauchi_app::ui::ChangePasswordEngine;
    let mut engine = ChangePasswordEngine::new(true);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "current_password".into(),
        value: "old-secret".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "new_password".into(),
        value: "new-secret".into(),
    });

    let output = engine.engine_output().expect("change-password output");
    assert_eq!(
        output,
        EngineOutput::ChangePassword {
            current: "old-secret".into(),
            new: "new-secret".into(),
        }
    );
    let debug = format!("{output:?}");
    assert!(
        !debug.contains("secret") && debug.contains("<redacted>"),
        "Debug must redact both credentials: {debug}"
    );
}

// @scenario: duress.feature - Setting up a duress PIN
#[test]
fn duress_pin_output_redacts_pin_in_debug() {
    use vauchi_app::ui::{DuressConfig, DuressPinEngine};
    let engine = DuressPinEngine::new(
        DuressConfig {
            enabled: true,
            available_contacts: vec![],
            selected_contact_ids: vec![],
            alert_message: "help".into(),
            include_location: true,
        },
        vauchi_app::i18n::Locale::English,
    );

    let output = engine.engine_output().expect("duress output");
    let EngineOutput::DuressPin(ref setup) = output else {
        panic!("duress engine must expose DuressPin output");
    };
    assert!(setup.enabled);
    assert_eq!(setup.alert_message, "help");
    assert!(setup.include_location);
    assert_eq!(setup.alert_contact_ids, Vec::<String>::new());
    let debug = format!("{output:?}");
    assert!(
        debug.contains("<redacted>"),
        "Debug must redact the PIN: {debug}"
    );
}
