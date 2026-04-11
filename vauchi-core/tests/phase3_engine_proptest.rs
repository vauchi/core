// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based tests for Phase 3 multi-step engines (CC-13).
//!
//! Covers all 5 Phase 3 engines: ExchangeEngine, DeviceLinkingEngine,
//! BackupRecoveryEngine, DuressPinEngine, EmergencyShredEngine.

use proptest::prelude::*;
use vauchi_app::ui::*;

// ── Known screen IDs per engine ─────────────────────────────────────

const EXCHANGE_SCREENS: &[&str] = &[
    "exchange_show_qr",
    "exchange_scan_qr",
    "exchange_verifying",
    "exchange_success",
    "exchange_failed",
];

const DEVICE_LINK_SCREENS: &[&str] = &[
    "link_show_qr",
    "link_verify",
    "link_syncing",
    "link_complete",
];

const BACKUP_SCREENS: &[&str] = &[
    "backup_choose",
    "backup_password",
    "backup_confirm",
    "backup_processing",
    "backup_complete",
    "backup_failed",
];

const DURESS_SCREENS: &[&str] = &[
    "duress_overview",
    "duress_enter_pin",
    "duress_confirm_pin",
    "duress_alerts",
];

const SHRED_SCREENS: &[&str] = &[
    "shred_warning",
    "shred_confirm",
    "shred_wiping",
    "shred_complete",
];

// ── Strategies ──────────────────────────────────────────────────────

fn arb_phase3_action() -> impl Strategy<Value = UserAction> {
    prop_oneof![
        // ActionPressed with known + random IDs
        prop_oneof![
            Just("continue".to_string()),
            Just("cancel".to_string()),
            Just("back".to_string()),
            Just("done".to_string()),
            Just("wipe".to_string()),
            Just("retry".to_string()),
            Just("confirm".to_string()),
            Just("reject".to_string()),
            Just("configure".to_string()),
            Just("disable".to_string()),
            Just("save".to_string()),
            Just("create".to_string()),
            Just("restore".to_string()),
            "[a-z]{3,10}",
        ]
        .prop_map(|action_id| UserAction::ActionPressed { action_id }),
        // TextChanged
        (
            prop_oneof![
                Just("password".to_string()),
                Just("confirm_password".to_string()),
                Just("pin".to_string()),
                Just("confirm_pin".to_string()),
                Just("confirmation".to_string()),
                Just("alert_message".to_string()),
                Just("scanned_data".to_string()),
                Just("unknown".to_string()),
            ],
            prop_oneof![
                Just(String::new()),
                Just("DELETE".to_string()),
                Just("123456".to_string()),
                Just("test-password".to_string()),
                "\\PC{1,20}",
            ],
        )
            .prop_map(|(component_id, value)| UserAction::TextChanged {
                component_id,
                value,
            }),
        // ItemToggled
        (
            prop_oneof![
                Just("alerts".to_string()),
                Just("duress_toggle".to_string()),
                Just("unknown".to_string()),
            ],
            prop_oneof![
                Just("include_location".to_string()),
                Just("enabled".to_string()),
                Just("unknown".to_string()),
            ],
        )
            .prop_map(|(component_id, item_id)| UserAction::ItemToggled {
                component_id,
                item_id,
            }),
    ]
}

/// Helper to check that a screen_id belongs to a known set.
fn assert_screen_in(
    screen_id: &str,
    known: &[&str],
    engine_name: &str,
) -> Result<(), TestCaseError> {
    prop_assert!(
        known.contains(&screen_id),
        "{}: unknown screen_id: {}",
        engine_name,
        screen_id,
    );
    Ok(())
}

/// Helper to validate an ActionResult's screen_id against known screens.
fn validate_result(
    result: &ActionResult,
    known: &[&str],
    engine_name: &str,
) -> Result<(), TestCaseError> {
    match result {
        ActionResult::UpdateScreen(screen) | ActionResult::NavigateTo(screen) => {
            assert_screen_in(&screen.screen_id, known, engine_name)?;
        }
        ActionResult::Complete
        | ActionResult::WipeComplete
        | ActionResult::RequestCamera
        | ActionResult::ValidationError { .. }
        | ActionResult::StartDeviceLink
        | ActionResult::StartBackupImport
        | ActionResult::OpenContact { .. }
        | ActionResult::EditContact { .. }
        | ActionResult::OpenUrl { .. }
        | ActionResult::ShowAlert { .. }
        | ActionResult::ShowToast { .. }
        | ActionResult::OpenEntryDetail { .. }
        | ActionResult::ExchangeCommands { .. }
        | ActionResult::PreviewAs { .. }
        | ActionResult::ShowContactPicker
        | ActionResult::VerifyFingerprint { .. } => {}
        _ => {}
    }
    Ok(())
}

// ── Engine constructors ─────────────────────────────────────────────

fn make_exchange() -> ExchangeEngine {
    ExchangeEngine::new(ExchangeConfig {
        own_name: "Test User".to_string(),
        own_qr_data: "test-qr-data-12345".to_string(),
        available_groups: vec![],
        device_capabilities: Default::default(),
        mode: Some(vauchi_core::exchange::mode::ExchangeMode::Glance),
        card_snapshot: None,
    })
}

fn make_device_linking() -> DeviceLinkingEngine {
    DeviceLinkingEngine::new("device-qr-data-67890".to_string())
}

fn make_backup(mode: Option<BackupMode>) -> BackupRecoveryEngine {
    BackupRecoveryEngine::new(mode, false)
}

fn make_duress() -> DuressPinEngine {
    DuressPinEngine::new(DuressConfig {
        enabled: false,
        alert_contacts: vec![],
        alert_message: String::new(),
        include_location: false,
    })
}

fn make_shred() -> EmergencyShredEngine {
    EmergencyShredEngine::new()
}

// ── Property 1: Random actions never panic ──────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn exchange_random_actions_never_panic(
        actions in prop::collection::vec(arb_phase3_action(), 0..50),
        trigger_external in prop::collection::vec(0..10u8, 0..5),
    ) {
        let mut engine = make_exchange();
        for (i, action) in actions.iter().enumerate() {
            // Occasionally call external methods mixed in with actions
            if trigger_external.get(i % trigger_external.len().max(1)).copied().unwrap_or(0) < 3 {
                engine.mark_success();
            } else if trigger_external.get(i % trigger_external.len().max(1)).copied().unwrap_or(0) < 5 {
                engine.mark_failed();
            }
            let result = engine.handle_action(action.clone());
            validate_result(&result, EXCHANGE_SCREENS, "ExchangeEngine")?;
        }
        let screen = engine.current_screen();
        assert_screen_in(&screen.screen_id, EXCHANGE_SCREENS, "ExchangeEngine")?;
    }

    #[test]
    fn device_linking_random_actions_never_panic(
        actions in prop::collection::vec(arb_phase3_action(), 0..50),
        trigger_external in prop::collection::vec(0..10u8, 0..5),
    ) {
        let mut engine = make_device_linking();
        for (i, action) in actions.iter().enumerate() {
            if trigger_external.get(i % trigger_external.len().max(1)).copied().unwrap_or(0) < 3 {
                engine.peer_connected("ABC-123".to_string());
            } else if trigger_external.get(i % trigger_external.len().max(1)).copied().unwrap_or(0) < 5 {
                engine.sync_complete();
            }
            let result = engine.handle_action(action.clone());
            validate_result(&result, DEVICE_LINK_SCREENS, "DeviceLinkingEngine")?;
        }
        let screen = engine.current_screen();
        assert_screen_in(&screen.screen_id, DEVICE_LINK_SCREENS, "DeviceLinkingEngine")?;
    }

    #[test]
    fn backup_random_actions_never_panic(
        actions in prop::collection::vec(arb_phase3_action(), 0..50),
        trigger_external in prop::collection::vec(0..10u8, 0..5),
    ) {
        let mut engine = make_backup(None);
        for (i, action) in actions.iter().enumerate() {
            if trigger_external.get(i % trigger_external.len().max(1)).copied().unwrap_or(0) < 3 {
                engine.processing_complete();
            } else if trigger_external.get(i % trigger_external.len().max(1)).copied().unwrap_or(0) < 5 {
                engine.processing_failed();
            }
            let result = engine.handle_action(action.clone());
            validate_result(&result, BACKUP_SCREENS, "BackupRecoveryEngine")?;
        }
        let screen = engine.current_screen();
        assert_screen_in(&screen.screen_id, BACKUP_SCREENS, "BackupRecoveryEngine")?;
    }

    #[test]
    fn duress_random_actions_never_panic(
        actions in prop::collection::vec(arb_phase3_action(), 0..50),
    ) {
        let mut engine = make_duress();
        for action in actions {
            let result = engine.handle_action(action);
            validate_result(&result, DURESS_SCREENS, "DuressPinEngine")?;
        }
        let screen = engine.current_screen();
        assert_screen_in(&screen.screen_id, DURESS_SCREENS, "DuressPinEngine")?;
    }

    #[test]
    fn shred_random_actions_never_panic(
        actions in prop::collection::vec(arb_phase3_action(), 0..50),
        trigger_external in prop::collection::vec(0..10u8, 0..5),
    ) {
        let mut engine = make_shred();
        for (i, action) in actions.iter().enumerate() {
            if trigger_external.get(i % trigger_external.len().max(1)).copied().unwrap_or(0) < 3 {
                engine.wipe_complete();
            }
            let result = engine.handle_action(action.clone());
            validate_result(&result, SHRED_SCREENS, "EmergencyShredEngine")?;
        }
        let screen = engine.current_screen();
        assert_screen_in(&screen.screen_id, SHRED_SCREENS, "EmergencyShredEngine")?;
    }
}

// ── Property 2: Screen stability ────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn exchange_screen_stability(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_exchange();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let s1 = engine.current_screen();
        let s2 = engine.current_screen();
        prop_assert_eq!(s1.screen_id, s2.screen_id);
        prop_assert_eq!(
            s1.progress.as_ref().map(|p| p.current_step),
            s2.progress.as_ref().map(|p| p.current_step),
        );
    }

    #[test]
    fn device_linking_screen_stability(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_device_linking();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let s1 = engine.current_screen();
        let s2 = engine.current_screen();
        prop_assert_eq!(s1.screen_id, s2.screen_id);
        prop_assert_eq!(
            s1.progress.as_ref().map(|p| p.current_step),
            s2.progress.as_ref().map(|p| p.current_step),
        );
    }

    #[test]
    fn backup_screen_stability(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_backup(None);
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let s1 = engine.current_screen();
        let s2 = engine.current_screen();
        prop_assert_eq!(s1.screen_id, s2.screen_id);
        prop_assert_eq!(
            s1.progress.as_ref().map(|p| p.current_step),
            s2.progress.as_ref().map(|p| p.current_step),
        );
    }

    #[test]
    fn duress_screen_stability(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_duress();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let s1 = engine.current_screen();
        let s2 = engine.current_screen();
        prop_assert_eq!(s1.screen_id, s2.screen_id);
        prop_assert_eq!(
            s1.progress.as_ref().map(|p| p.current_step),
            s2.progress.as_ref().map(|p| p.current_step),
        );
    }

    #[test]
    fn shred_screen_stability(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_shred();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let s1 = engine.current_screen();
        let s2 = engine.current_screen();
        prop_assert_eq!(s1.screen_id, s2.screen_id);
        prop_assert_eq!(
            s1.progress.as_ref().map(|p| p.current_step),
            s2.progress.as_ref().map(|p| p.current_step),
        );
    }
}

// ── Property 3: Progress invariants ─────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn exchange_progress_invariants(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_exchange();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let screen = engine.current_screen();
        let progress = screen.progress.as_ref().expect("exchange screens must have progress");
        prop_assert!(progress.total_steps > 0, "total_steps must be > 0");
        prop_assert!(
            progress.current_step >= 1 && progress.current_step <= progress.total_steps,
            "current_step {} out of range [1, {}]",
            progress.current_step,
            progress.total_steps,
        );
    }

    #[test]
    fn device_linking_progress_invariants(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_device_linking();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let screen = engine.current_screen();
        let progress = screen.progress.as_ref().expect("device linking screens must have progress");
        prop_assert!(progress.total_steps > 0, "total_steps must be > 0");
        prop_assert!(
            progress.current_step >= 1 && progress.current_step <= progress.total_steps,
            "current_step {} out of range [1, {}]",
            progress.current_step,
            progress.total_steps,
        );
    }

    #[test]
    fn backup_progress_invariants(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_backup(None);
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let screen = engine.current_screen();
        // backup_choose has no progress bar
        if screen.screen_id != "backup_choose" {
            let progress = screen.progress.as_ref().expect("backup screens (except choose) must have progress");
            prop_assert!(progress.total_steps > 0, "total_steps must be > 0");
            prop_assert!(
                progress.current_step >= 1 && progress.current_step <= progress.total_steps,
                "current_step {} out of range [1, {}]",
                progress.current_step,
                progress.total_steps,
            );
        } else {
            prop_assert!(screen.progress.is_none(), "backup_choose should have no progress");
        }
    }

    #[test]
    fn duress_progress_invariants(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_duress();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let screen = engine.current_screen();
        let progress = screen.progress.as_ref().expect("duress screens must have progress");
        prop_assert!(progress.total_steps > 0, "total_steps must be > 0");
        prop_assert!(
            progress.current_step >= 1 && progress.current_step <= progress.total_steps,
            "current_step {} out of range [1, {}]",
            progress.current_step,
            progress.total_steps,
        );
    }

    #[test]
    fn shred_progress_invariants(actions in prop::collection::vec(arb_phase3_action(), 0..30)) {
        let mut engine = make_shred();
        for action in actions {
            let _ = engine.handle_action(action);
        }
        let screen = engine.current_screen();
        let progress = screen.progress.as_ref().expect("shred screens must have progress");
        prop_assert!(progress.total_steps > 0, "total_steps must be > 0");
        prop_assert!(
            progress.current_step >= 1 && progress.current_step <= progress.total_steps,
            "current_step {} out of range [1, {}]",
            progress.current_step,
            progress.total_steps,
        );
    }
}

// ── Property 4: Forward progress — known sequences reach terminal ───

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Exchange: ShowQr → scan data → mark_success → done = Complete.
    #[test]
    fn exchange_forward_progress(
        qr_data in "[a-z0-9]{5,30}",
        name in "[A-Za-z ]{1,30}",
    ) {
        let mut engine = ExchangeEngine::new(ExchangeConfig {
            own_name: name,
            own_qr_data: "my-qr".to_string(),
            available_groups: vec![],
            device_capabilities: Default::default(),
            mode: Some(vauchi_core::exchange::mode::ExchangeMode::Glance),
            card_snapshot: None,
        });

        // ShowQr → ScanQr
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "exchange_scan_qr");

        // Provide scanned data → Verifying
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "scanned_data".into(),
            value: qr_data.clone(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "exchange_verifying");
        prop_assert_eq!(engine.scanned_data(), Some(qr_data.as_str()));

        // External: mark_success → Success
        engine.mark_success();
        prop_assert_eq!(engine.current_screen().screen_id, "exchange_success");

        // Done → Complete
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "done".into(),
        });
        prop_assert!(matches!(result, ActionResult::Complete));
    }

    /// DeviceLinking: ShowQr → peer_connected → confirm → sync_complete → done = Complete.
    #[test]
    fn device_linking_forward_progress(code in "[A-Z0-9]{6}") {
        let mut engine = make_device_linking();

        // External: peer connects
        engine.peer_connected(code.clone());
        prop_assert_eq!(engine.current_screen().screen_id, "link_verify");

        // Confirm → Syncing
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "link_syncing");

        // External: sync done
        engine.sync_complete();
        prop_assert_eq!(engine.current_screen().screen_id, "link_complete");

        // Done → Complete
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "done".into(),
        });
        prop_assert!(matches!(result, ActionResult::Complete));
    }

    /// BackupRecovery (Create): choose → password → confirm → processing_complete → done.
    #[test]
    fn backup_create_forward_progress(password in "[a-zA-Z0-9]{4,20}") {
        let mut engine = make_backup(None);

        // Choose Create
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "create".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "backup_password");

        // Enter password
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "password".into(),
            value: password.clone(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "backup_confirm");

        // Confirm password
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "confirm_password".into(),
            value: password,
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "backup_processing");

        // External: complete
        engine.processing_complete();
        prop_assert_eq!(engine.current_screen().screen_id, "backup_complete");

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "done".into(),
        });
        prop_assert!(matches!(result, ActionResult::Complete));
    }

    /// BackupRecovery (Restore): choose → password → processing_complete → done.
    #[test]
    fn backup_restore_forward_progress(password in "[a-zA-Z0-9]{4,20}") {
        let mut engine = make_backup(None);

        // Choose Restore
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "restore".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "backup_password");

        // Enter password
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "password".into(),
            value: password,
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "backup_processing");

        // External: complete
        engine.processing_complete();
        prop_assert_eq!(engine.current_screen().screen_id, "backup_complete");

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "done".into(),
        });
        prop_assert!(matches!(result, ActionResult::Complete));
    }

    /// DuressPin: overview → configure → enter pin → confirm pin → alerts → save = Complete.
    #[test]
    fn duress_forward_progress(pin in "[0-9]{4,8}") {
        let mut engine = make_duress();

        // Overview → EnterPin
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "configure".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "duress_enter_pin");

        // Enter PIN
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: pin.clone(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "duress_confirm_pin");

        // Confirm PIN
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "confirm_pin".into(),
            value: pin,
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "duress_alerts");

        // Save → Complete
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "save".into(),
        });
        prop_assert!(matches!(result, ActionResult::Complete));
    }

    /// EmergencyShred: warning → confirm → type DELETE → wipe → wipe_complete → done = WipeComplete.
    #[test]
    fn shred_forward_progress(_dummy in 0..1u8) {
        let mut engine = make_shred();

        // Warning → Confirm
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "shred_confirm");

        // Type DELETE
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "confirmation".into(),
            value: "DELETE".into(),
        });

        // Wipe → Wiping
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "wipe".into(),
        });
        prop_assert_eq!(engine.current_screen().screen_id, "shred_wiping");

        // External: wipe done
        engine.wipe_complete();
        prop_assert_eq!(engine.current_screen().screen_id, "shred_complete");

        // Done → WipeComplete
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "done".into(),
        });
        prop_assert!(matches!(result, ActionResult::WipeComplete));
    }
}
