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
    BackupFormSnapshot, EmergencyBroadcastPlan, EngineOutput, OnboardingEngine, UserAction,
    WorkflowEngine,
};

// @scenario: onboarding.feature - Completing onboarding creates identity
#[test]
fn onboarding_output_carries_typed_display_name() {
    let mut engine = OnboardingEngine::new();
    engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Ada".into(),
    });

    let Some(EngineOutput::Onboarding(data)) = engine.engine_output() else {
        panic!("onboarding engine must expose Onboarding output");
    };
    assert_eq!(data.display_name, "Ada");
    assert!(
        data.selected_groups.iter().all(|g| !g.selected),
        "no group is selected before the user toggles one"
    );
    assert_eq!(data.fields, vec![]);
}

// @scenario: emergency_broadcast.feature - Configuring an emergency broadcast
#[test]
fn emergency_broadcast_output_snapshots_empty_plan() {
    use vauchi_app::ui::EmergencyBroadcastEngine;
    let engine = EmergencyBroadcastEngine::new(None);

    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::EmergencyBroadcast(EmergencyBroadcastPlan {
            outcome: None,
            contact_ids: vec![],
            message: String::new(),
            include_location: false,
        }))
    );
}

// @scenario: backup.feature - Restoring from a backup blob
#[test]
fn backup_output_redacts_password_in_debug() {
    use vauchi_app::ui::{BackupMode, BackupRecoveryEngine};
    let engine = BackupRecoveryEngine::new(Some(BackupMode::Restore), true);

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
