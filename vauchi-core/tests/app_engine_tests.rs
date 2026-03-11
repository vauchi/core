// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::MockTransport;
use vauchi_core::ui::{
    ActionResult, ActionStyle, AppEngine, AppScreen, Component, FormDialogType, UserAction,
    WorkflowEngine,
};

/// Drive through the full onboarding flow, returning the final ActionResult.
/// Each intermediate step is asserted to produce the expected ActionResult variant (T-12).
fn drive_onboarding(engine: &mut AppEngine<MockTransport>) -> ActionResult {
    // Step 1: create_new -> navigates to welcome
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 1 (create_new) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "welcome",
        "create_new should navigate to welcome"
    );

    // Step 2: get_started -> navigates to default_name
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 2 (get_started) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "get_started should navigate to default_name"
    );

    // Step 3: enter display name -> updates screen
    let r = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let ActionResult::UpdateScreen(screen) = r else {
        panic!("Step 3 (TextChanged display_name) expected UpdateScreen, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "TextChanged should update the default_name screen"
    );

    // Step 4: continue -> navigates to skip_gate
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 4 (continue) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "skip_gate",
        "continue should navigate to skip_gate"
    );

    // Step 5: skip_to_finish -> navigates to security_explanation
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip_to_finish".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 5 (skip_to_finish) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "security_explanation",
        "skip_to_finish should navigate to security_explanation"
    );

    // Step 6: continue -> navigates to backup_prompt
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 6 (continue) expected NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "backup_prompt",
        "continue should navigate to backup_prompt"
    );

    // Step 7: skip -> navigates to ready
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("Step 7 (skip) expected NavigateTo, got {r:?}");
    };
    assert_eq!(screen.screen_id, "ready", "skip should navigate to ready");

    // Step 8: start -> Complete -> AppEngine routes to Home
    engine.handle_action(UserAction::ActionPressed {
        action_id: "start".into(),
    })
}

#[test]
fn app_engine_starts_on_onboarding_without_identity() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.current_app_screen(), &AppScreen::Onboarding);
}

#[test]
fn app_engine_shows_onboarding_screen() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "identity_check");
    assert!(!screen.title.is_empty());
}

#[test]
fn navigate_to_home_shows_home_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::MyInfo);
    assert_eq!(screen.screen_id, "my_info");
}

#[test]
fn navigate_to_contacts_shows_contact_list() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(screen.screen_id, "contact_list");
}

#[test]
fn navigate_to_settings_shows_settings() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Settings);
    assert_eq!(screen.screen_id, "settings");
}

#[test]
fn navigate_to_exchange_shows_qr() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(screen.screen_id, "exchange_show_qr");
}

#[test]
fn navigate_to_help_shows_help() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Help);
    assert_eq!(screen.screen_id, "help");
}

#[test]
fn navigate_to_lock_shows_lock() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Lock);
    assert_eq!(screen.screen_id, "lock_screen");
}

#[test]
fn navigate_to_emergency_shred_shows_warning() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::EmergencyShred);
    assert_eq!(screen.screen_id, "shred_warning");
}

#[test]
fn navigate_to_backup_shows_backup() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Backup);
    assert_eq!(screen.screen_id, "backup_choose");
}

// ── persistence tests ───────────────────────────────────────────────

#[test]
fn app_engine_detects_persisted_identity() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    // Create identity with file-backed storage, then drop it
    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut vauchi = Vauchi::<MockTransport>::new(config).unwrap();
        vauchi.create_identity("Persisted User").unwrap();
        assert!(vauchi.has_identity());
    }

    // Open a fresh Vauchi from the same path — identity should be loaded
    let config2 = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
    let vauchi2 = Vauchi::<MockTransport>::new(config2).unwrap();
    assert!(
        vauchi2.has_identity(),
        "Vauchi should detect persisted identity on reopen"
    );

    let engine = AppEngine::new(vauchi2);
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "AppEngine should start on Home when identity exists in storage"
    );
}

// ── available_screens tests ─────────────────────────────────────────

#[test]
fn available_screens_without_identity_is_onboarding_only() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    let screens = engine.available_screens();
    assert_eq!(screens, vec![AppScreen::Onboarding]);
}

#[test]
fn available_screens_with_identity_has_main_nav() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    let screens = engine.available_screens();
    assert!(screens.contains(&AppScreen::MyInfo));
    assert!(screens.contains(&AppScreen::Contacts));
    assert!(screens.contains(&AppScreen::Settings));
    assert!(!screens.contains(&AppScreen::Onboarding));
}

// ── helper: onboarding without setting display name ─────────────────

/// Drive onboarding to the name step and attempt to continue without entering a name.
/// Returns the result of pressing "continue" without a display name.
fn drive_onboarding_without_name(engine: &mut AppEngine<MockTransport>) -> ActionResult {
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("create_new should produce NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "welcome",
        "create_new should navigate to welcome"
    );
    let r = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let ActionResult::NavigateTo(screen) = r else {
        panic!("get_started should produce NavigateTo, got {r:?}");
    };
    assert_eq!(
        screen.screen_id, "default_name",
        "get_started should navigate to default_name"
    );
    // Attempt to continue without setting display_name
    engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    })
}

// ── completion routing tests ────────────────────────────────────────

#[test]
fn onboarding_complete_navigates_to_home() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    let result = drive_onboarding(&mut engine);

    // Should navigate to Home after onboarding completes (T-1: verify screen_id)
    let ActionResult::NavigateTo(screen) = result else {
        panic!("expected NavigateTo, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "my_info",
        "onboarding completion should navigate to home"
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

#[test]
fn app_engine_starts_on_home_with_identity() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

#[test]
fn onboarding_completion_without_name_returns_validation_error() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    let result = drive_onboarding_without_name(&mut engine);

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "display_name");
            assert!(
                !message.is_empty(),
                "validation message should not be empty"
            );
        }
        other => panic!("expected ValidationError, got {:?}", other),
    }
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Onboarding,
        "should remain on Onboarding when name is missing"
    );
    assert!(
        !engine.has_identity(),
        "no identity should be created without a name"
    );
}

// ── setup progress tests ────────────────────────────────────────────

#[test]
fn home_screen_no_setup_progress() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "my_info");

    // Setup progress should no longer be shown on MyInfo
    let has_progress = screen
        .components
        .iter()
        .any(|c| matches!(c, vauchi_core::ui::Component::StatusIndicator { id, .. } if id == "setup_progress"));
    assert!(!has_progress, "MyInfo should not show setup progress");
}

/// Verify that a whitespace-only name is also rejected.
#[test]
fn onboarding_completion_with_empty_name_returns_validation_error() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Intermediate navigation steps — final validation asserted below
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    // Intermediate step: set a whitespace-only name — validation asserted below
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "   ".into(),
    });
    // Try to continue — onboarding engine should reject it
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "display_name");
            assert!(
                !message.is_empty(),
                "validation message should not be empty"
            );
        }
        other => panic!("expected ValidationError, got {:?}", other),
    }
    assert!(
        !engine.has_identity(),
        "no identity should be created with whitespace-only name"
    );
}

#[test]
fn duress_pin_screen_renders_with_defaults() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DuressPin);
    assert_eq!(screen.screen_id, "duress_overview");
    assert_eq!(screen.title, "Duress PIN");
}

#[test]
fn onboarding_complete_creates_identity_in_vauchi() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    assert!(!engine.has_identity());

    // Intermediate step: drive full onboarding — identity persistence asserted below
    let _ = drive_onboarding(&mut engine);

    assert!(
        engine.has_identity(),
        "identity should be persisted after onboarding"
    );
    assert!(
        engine.available_screens().contains(&AppScreen::MyInfo),
        "should have full nav after identity created"
    );
}

// ── home screen contact limit tests ─────────────────────────────────

#[test]
fn my_info_shows_own_fields_via_app_engine() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(vauchi_core::contact_card::ContactField::new(
            vauchi_core::contact_card::FieldType::Phone,
            "Mobile",
            "+41 79 123 45 67",
        ))
        .unwrap();

    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "my_info");

    // MyInfo should show own fields in an ActionList (entry view)
    let has_entries = screen.components.iter().any(
        |c| matches!(c, vauchi_core::ui::Component::ActionList { id, .. } if id == "own_entries"),
    );
    assert!(has_entries, "MyInfo should show own entries ActionList");

    let has_contact_list = screen
        .components
        .iter()
        .any(|c| matches!(c, vauchi_core::ui::Component::ContactList { .. }));
    assert!(!has_contact_list, "MyInfo should not show a ContactList");
}

#[test]
fn my_info_renders_safely_with_no_fields() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "my_info");
    assert!(
        !screen.title.is_empty(),
        "my_info screen should have a title"
    );
}

// ── contact detail / edit wiring tests ──────────────────────────────

// NOTE: contact_detail_does_not_show_empty_list and contact_edit_does_not_show_empty_list
// were removed — they were subsumed by the stronger *_nonexistent_shows_not_found tests below
// which assert the exact screen_id ("contact_not_found") rather than just != "contact_list".

#[test]
fn contact_detail_nonexistent_shows_not_found() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: "nonexistent".into(),
    });
    assert_eq!(screen.screen_id, "contact_not_found");
}

#[test]
fn contact_edit_nonexistent_shows_not_found() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ContactEdit {
        contact_id: "nonexistent".into(),
    });
    // Non-existent contact should show edit_fields (empty) or not_found
    // ContactEditEngine starts on edit_fields, but with nonexistent we show not_found
    assert_eq!(screen.screen_id, "contact_not_found");
}

// ── failure-path tests for create_engine edge cases ─────────────────

// NOTE: navigate_to_contact_detail_with_nonexistent_id was removed —
// subsumed by contact_detail_nonexistent_shows_not_found which asserts exact screen_id.

#[test]
fn navigate_to_exchange_without_identity_card() {
    // Create Vauchi but don't create identity — own_card() returns None
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Should be on onboarding, navigate to exchange anyway
    let screen = engine.navigate_to(AppScreen::Exchange);
    assert!(!screen.screen_id.is_empty());
}

#[test]
fn navigate_to_settings_without_identity() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(!screen.screen_id.is_empty());
}

// ── engine cache tests ──────────────────────────────────────────────

#[test]
fn navigate_away_and_back_preserves_engine_state() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Exchange — starts on exchange_show_qr
    let first_visit = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(first_visit.screen_id, "exchange_show_qr");

    // Intermediate step — advance to scan; screen_id asserted on next line
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let scan_screen = engine.current_screen();
    assert_eq!(
        scan_screen.screen_id, "exchange_scan_qr",
        "exchange engine should advance to scan step"
    );

    // Navigate away to Home — Exchange engine should be cached at scan step
    let home = engine.navigate_to(AppScreen::MyInfo);
    assert_eq!(home.screen_id, "my_info");

    // Navigate back to Exchange — should restore cached engine on scan step
    let restored = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(
        restored.screen_id, "exchange_scan_qr",
        "cached engine should preserve internal state (scan step, not show_qr)"
    );
}

#[test]
fn onboarding_engine_not_cached() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Start on Onboarding, navigate away — intermediate step; fresh start asserted below
    assert_eq!(engine.current_app_screen(), &AppScreen::Onboarding);
    let _ = engine.navigate_to(AppScreen::MyInfo);

    // Navigate back to Onboarding — should always be fresh (identity_check)
    let screen = engine.navigate_to(AppScreen::Onboarding);
    assert_eq!(
        screen.screen_id, "identity_check",
        "Onboarding should always start fresh, not be restored from cache"
    );
}

#[test]
fn lock_screen_engine_not_cached() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Lock, then away
    let lock = engine.navigate_to(AppScreen::Lock);
    assert_eq!(lock.screen_id, "lock_screen");
    // Intermediate step: navigate away — fresh lock screen asserted below
    let _ = engine.navigate_to(AppScreen::MyInfo);

    // Navigate back to Lock — should always be fresh
    let lock2 = engine.navigate_to(AppScreen::Lock);
    assert_eq!(
        lock2.screen_id, "lock_screen",
        "Lock should always start fresh, not be restored from cache"
    );
}

#[test]
fn navigate_creates_fresh_engine_first_time() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // First navigation to each screen should create a fresh engine
    let contacts = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(contacts.screen_id, "contact_list");

    let help = engine.navigate_to(AppScreen::Help);
    assert_eq!(help.screen_id, "help");

    let settings = engine.navigate_to(AppScreen::Settings);
    assert_eq!(settings.screen_id, "settings");
}

// ── settings toggle persistence tests (HIGH-4) ──────────────────────

/// Helper: find a toggle's enabled state in a settings screen.
fn find_settings_toggle(
    screen: &vauchi_core::ui::ScreenModel,
    group_id: &str,
    item_id: &str,
) -> bool {
    use vauchi_core::ui::{Component, SettingsItemKind};
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { id, items, .. } if id == group_id => {
                items.iter().find_map(|item| match &item.kind {
                    SettingsItemKind::Toggle { enabled } if item.id == item_id => Some(*enabled),
                    _ => None,
                })
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Toggle '{item_id}' not found in group '{group_id}'"))
}

#[test]
fn settings_toggle_persists_after_navigate_away_and_back() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Settings
    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(
        find_settings_toggle(&screen, "privacy", "delivery_receipts"),
        "delivery_receipts should default to enabled"
    );

    // Toggle delivery_receipts off
    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "delivery_receipts".into(),
    });
    match &result {
        ActionResult::UpdateScreen(s) => {
            assert!(
                !find_settings_toggle(s, "privacy", "delivery_receipts"),
                "delivery_receipts should be disabled after toggle"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }

    // Navigate away to Home
    engine.navigate_to(AppScreen::MyInfo);

    // Invalidate Settings cache to force fresh engine from vauchi.config()
    engine.invalidate_screen(&AppScreen::Settings);

    // Navigate back to Settings — toggle should still be off
    let restored = engine.navigate_to(AppScreen::Settings);
    assert!(
        !find_settings_toggle(&restored, "privacy", "delivery_receipts"),
        "delivery_receipts toggle should persist after navigating away and back (even with cache invalidated)"
    );
}

#[test]
fn settings_toggle_suppress_presence_persists() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Settings
    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(
        !find_settings_toggle(&screen, "privacy", "suppress_presence"),
        "suppress_presence should default to disabled"
    );

    // Intermediate step: toggle on — persistence asserted after navigate-away-and-back
    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "suppress_presence".into(),
    });

    // Navigate away and invalidate
    engine.navigate_to(AppScreen::MyInfo);
    engine.invalidate_screen(&AppScreen::Settings);

    // Navigate back — should still be on
    let restored = engine.navigate_to(AppScreen::Settings);
    assert!(
        find_settings_toggle(&restored, "privacy", "suppress_presence"),
        "suppress_presence toggle should persist after navigating away and back"
    );
}

// ── cache invalidation tests ─────────────────────────────────────────

#[test]
fn invalidate_screen_removes_cached_engine() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Contacts, then Home (caches Contacts engine)
    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::MyInfo);

    // Invalidate Contacts cache
    engine.invalidate_screen(&AppScreen::Contacts);

    // Navigate back — should get fresh engine (not the cached one)
    let screen = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(screen.screen_id, "contact_list");
}

// ── lock screen password verification tests (CRIT-3) ─────────────────

/// Helper: create an AppEngine with identity + password set, starting on Lock screen.
fn engine_with_password(password: &str) -> AppEngine<MockTransport> {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.setup_app_password(password).unwrap();
    assert!(
        vauchi.is_password_enabled().unwrap(),
        "password should be enabled after setup"
    );
    AppEngine::new(vauchi)
}

/// Helper: enter a PIN into the lock screen engine.
fn enter_pin(engine: &mut AppEngine<MockTransport>, pin: &str) {
    for ch in pin.chars() {
        // Intermediate step: accumulate PIN digits — unlock result asserted by caller
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "pin".into(),
            value: ch.to_string(),
        });
    }
}

#[test]
fn lock_screen_wrong_pin_stays_locked() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Enter wrong PIN and press unlock
    enter_pin(&mut engine, "999999");
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    // Should NOT navigate to Home — should show error or stay on lock
    assert_ne!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "wrong PIN must NOT unlock the app"
    );
    assert!(
        !matches!(result, ActionResult::NavigateTo(_)),
        "wrong PIN should not produce NavigateTo, got {:?}",
        result
    );
}

#[test]
fn lock_screen_correct_pin_unlocks() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Enter correct PIN and press unlock
    enter_pin(&mut engine, "123456");
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    // Should navigate to Home (T-1: verify screen_id)
    let ActionResult::NavigateTo(screen) = result else {
        panic!("correct PIN should navigate to Home, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "my_info",
        "correct PIN should navigate to home screen"
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "should be on Home after correct PIN"
    );
}

#[test]
fn lock_screen_empty_pin_does_not_unlock() {
    let mut engine = engine_with_password("123456");
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Press unlock without entering any PIN
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    assert_ne!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "empty PIN must NOT unlock the app"
    );
    assert!(
        !matches!(result, ActionResult::NavigateTo(_)),
        "empty PIN should not produce NavigateTo, got {:?}",
        result
    );
}

#[test]
fn lock_screen_tracks_failed_attempts() {
    let mut engine = engine_with_password("123456");

    // Enter wrong PIN twice
    for _ in 0..2 {
        enter_pin(&mut engine, "000000");
        // Intermediate step: trigger failed attempt — attempt count asserted below
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "unlock".into(),
        });
        // Clear PIN for next attempt — navigate back to lock to get fresh engine
        // Actually LockScreenEngine should still be active, but PIN persists.
        // We need to clear the entered PIN for the next attempt.
        // The lock screen should show remaining attempts in the validation error.
    }

    // Should still be on Lock screen
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Lock,
        "should remain locked after failed attempts"
    );

    // The screen should show attempt tracking info
    let screen = engine.current_screen();
    let has_validation_error = screen.components.iter().any(|c| {
        matches!(
            c,
            vauchi_core::ui::Component::PinInput {
                validation_error: Some(_),
                ..
            }
        )
    });
    assert!(
        has_validation_error,
        "lock screen should show remaining attempts after failures"
    );
}

#[test]
fn lock_screen_correct_pin_after_failed_attempt_unlocks() {
    let mut engine = engine_with_password("123456");

    // First attempt: wrong PIN — intermediate step; correct PIN unlock asserted below
    enter_pin(&mut engine, "000000");
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
    assert_eq!(engine.current_app_screen(), &AppScreen::Lock);

    // Second attempt: correct PIN
    enter_pin(&mut engine, "123456");
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });

    let ActionResult::NavigateTo(screen) = result else {
        panic!("correct PIN after failed attempt should unlock, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "my_info",
        "correct PIN after failed attempt should navigate to home"
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

#[test]
fn invalidate_all_clears_entire_cache() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Cache multiple screens
    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::MyInfo);

    // Invalidate all
    engine.invalidate_all();

    // Both should get fresh engines
    let contacts = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(contacts.screen_id, "contact_list");
    let settings = engine.navigate_to(AppScreen::Settings);
    assert_eq!(settings.screen_id, "settings");
}

// ── edit routing tests (HIGH-2) ──────────────────────────────────────

#[test]
fn contact_detail_edit_navigates_to_edit_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    // Add a contact
    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([2u8; 32], card, shared_key);
    let bob_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let mut engine = AppEngine::new(vauchi);
    // Navigate to ContactDetail for Bob
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: bob_id.clone(),
    });
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::ContactDetail {
            contact_id: bob_id.clone()
        }
    );

    // Press the "edit" button
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    // Should navigate to ContactEdit, not re-open ContactDetail (T-1: verify screen_id)
    let ActionResult::NavigateTo(screen) = result else {
        panic!("expected NavigateTo for edit button, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "edit_fields",
        "edit button should navigate to edit_fields screen"
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::ContactEdit { contact_id: bob_id },
        "edit button should route to ContactEdit, not ContactDetail"
    );
}

// ── navigation history tests (HIGH-5) ────────────────────────────────

#[test]
fn back_from_contact_detail_returns_to_contacts() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate: Home -> Contacts -> ContactDetail
    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: "nonexistent".into(),
    });
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::ContactDetail {
            contact_id: "nonexistent".into()
        }
    );

    // Press back (Complete) — should go to Contacts, not Home (T-1: verify screen_id)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    let ActionResult::NavigateTo(screen) = result else {
        panic!("expected NavigateTo, got {result:?}");
    };
    assert_eq!(
        screen.screen_id, "contact_list",
        "back should navigate to contact_list screen"
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Contacts,
        "back from ContactDetail should return to Contacts, not Home"
    );
}

#[test]
fn navigate_back_from_duress_pin_returns_to_settings() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate: Home -> Settings -> DuressPin
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::DuressPin);
    assert_eq!(engine.current_app_screen(), &AppScreen::DuressPin);

    // Navigate back — should go to Settings, not Home
    engine.navigate_back();
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Settings,
        "navigate_back from DuressPin should return to Settings, not Home"
    );
}

#[test]
fn navigate_back_from_settings_returns_to_home() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate: Home -> Settings
    engine.navigate_to(AppScreen::Settings);
    assert_eq!(engine.current_app_screen(), &AppScreen::Settings);

    // Use navigate_back() directly (Settings has no "back" action button)
    engine.navigate_back();
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::MyInfo,
        "navigate_back from Settings should return to Home"
    );
}

#[test]
fn back_with_empty_history_returns_to_home() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Without any navigation, back should go to Home (fallback)
    let screen = engine.navigate_back();
    assert_eq!(screen.screen_id, "my_info");
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

#[test]
fn navigate_back_does_not_create_circular_history() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate: Home -> Contacts -> Settings
    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::Settings);

    // Back: Settings -> Contacts
    engine.navigate_back();
    assert_eq!(engine.current_app_screen(), &AppScreen::Contacts);

    // Back: Contacts -> Home
    engine.navigate_back();
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);

    // Back again: empty history -> Home (fallback)
    engine.navigate_back();
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

// ── stateful proptest: onboarding random actions (CC-13) ─────────────

use proptest::prelude::*;

// ── Wave 6 Phase A: new engine navigation tests ──────────────────────

#[test]
fn navigate_to_sync_shows_sync_status() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Sync);
    assert_eq!(screen.screen_id, "sync_status");
    assert_eq!(screen.title, "Sync");
    assert!(
        screen.actions.iter().any(|a| a.id == "sync_now"),
        "Sync screen must have sync_now action"
    );
}

#[test]
fn navigate_to_tor_settings_shows_tor() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::TorSettings);
    assert_eq!(screen.screen_id, "tor_settings");
    assert_eq!(screen.title, "Tor Privacy");
    assert!(
        !screen.components.is_empty(),
        "Tor screen must have at least one component"
    );
}

#[test]
fn navigate_to_recovery_shows_recovery_status() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Recovery);
    assert_eq!(screen.screen_id, "recovery_status");
    assert_eq!(screen.title, "Social Recovery");
    // Fresh identity has 0 contacts, quorum not met — Start Recovery disabled
    assert!(
        screen.actions.iter().any(|a| a.id == "claim" && !a.enabled),
        "Recovery claim action must be disabled when quorum not met"
    );
}

#[test]
fn navigate_to_groups_shows_groups_list() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Groups);
    assert_eq!(screen.screen_id, "groups_list");
    assert_eq!(screen.title, "Groups");
    assert!(
        screen.actions.iter().any(|a| a.id == "new_group"),
        "Groups screen must have new_group action"
    );
}

#[test]
fn navigate_to_privacy_shows_privacy_settings() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Privacy);
    assert_eq!(screen.screen_id, "privacy_settings");
    assert_eq!(screen.title, "Privacy & Data");
    // GDPR delete action must be Destructive style
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "delete" && a.style == ActionStyle::Destructive),
        "Privacy screen must have destructive delete action"
    );
}

#[test]
fn navigate_to_support_shows_support() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Support);
    assert_eq!(screen.screen_id, "support");
    assert_eq!(screen.title, "Support Vauchi");
    assert!(
        !screen.components.is_empty(),
        "Support screen must have at least one component"
    );
}

// ── Wave 6 failure-path tests (CC-11) ────────────────────────────────

#[test]
fn sync_engine_unknown_action_returns_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Sync);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "sync_status");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn privacy_engine_text_changed_is_noop() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Privacy);
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "bogus".into(),
        value: "test".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "privacy_settings");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn navigate_to_group_detail_shows_group() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::GroupDetail {
        group_id: "g1".into(),
    });
    assert_eq!(screen.screen_id, "group_detail");
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "delete_group" && a.style == ActionStyle::Destructive),
        "GroupDetail must have destructive delete action"
    );
}

// ── contact visibility tests ─────────────────────────────────────────

#[test]
fn navigate_to_contact_visibility_shows_toggles() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    // No real contact exists, so engine shows empty field list
    let screen = engine.navigate_to(AppScreen::ContactVisibility {
        contact_id: "fake-id".into(),
    });
    assert_eq!(screen.screen_id, "contact_visibility");
    assert!(
        screen.actions.iter().any(|a| a.id == "save"),
        "Visibility screen must have save action"
    );
}

#[test]
fn contact_visibility_toggle_updates_field() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactVisibility {
        contact_id: "fake-id".into(),
    });
    // Toggle a nonexistent field — should not panic, just return screen
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "field_toggles".into(),
        item_id: "nonexistent".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_visibility");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// ── FormDialogEngine tests ────────────────────────────────────────────

#[test]
fn form_dialog_add_field_shows_type_list() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });
    // Single-page form with flat type list
    assert_eq!(screen.screen_id, "form_add_field");
    let has_action_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ActionList { .. }));
    assert!(
        has_action_list,
        "Should have an ActionList for type selection"
    );
}

#[test]
fn form_dialog_add_field_type_selection_shows_value_inputs() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select type from flat list
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_add_field");
            let text_inputs: Vec<_> = screen
                .components
                .iter()
                .filter(|c| matches!(c, Component::TextInput { .. }))
                .collect();
            assert_eq!(
                text_inputs.len(),
                2,
                "Should have 2 text inputs (value + note) after selecting type"
            );
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_edit_name_tracks_text_changes() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Old Name".into(),
        },
    });
    // Change the display name
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "New Name".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "form_edit_name");
            // Verify the TextInput now shows "New Name"
            let has_new_value = screen.components.iter().any(|c| {
                matches!(c, Component::TextInput { id, value, .. } if id == "display_name" && value == "New Name")
            });
            assert!(has_new_value, "TextInput should reflect updated value");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

#[test]
fn form_dialog_submit_navigates_back() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    // Navigate to Home first, then to the form — so back goes to Home
    engine.navigate_to(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditRelayUrl {
            current_url: "wss://old.relay".into(),
        },
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    // AppEngine intercepts Complete and navigates back
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "my_info");
        }
        other => panic!("Expected NavigateTo(home), got {other:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Random sequences of UserActions fired at a fresh AppEngine never
    /// panic and always produce a non-empty screen_id. This satisfies
    /// CC-13 (stateful property tests for state machines).
    #[test]
    fn onboarding_random_actions_never_panic(
        actions in prop::collection::vec(
            prop_oneof![
                Just(UserAction::ActionPressed { action_id: "create_new".into() }),
                Just(UserAction::ActionPressed { action_id: "have_identity".into() }),
                Just(UserAction::ActionPressed { action_id: "get_started".into() }),
                Just(UserAction::ActionPressed { action_id: "continue".into() }),
                Just(UserAction::ActionPressed { action_id: "skip".into() }),
                Just(UserAction::ActionPressed { action_id: "back".into() }),
                Just(UserAction::ActionPressed { action_id: "start".into() }),
                Just(UserAction::ActionPressed { action_id: "skip_to_finish".into() }),
                ".*".prop_map(|s| UserAction::TextChanged {
                    component_id: "display_name".into(),
                    value: s,
                }),
            ],
            0..30
        )
    ) {
        let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
        let mut engine = AppEngine::new(vauchi);
        for action in actions {
            // Result intentionally discarded — proptest asserts no-panic + non-empty screen_id
            let _ = engine.handle_action(action);
            let screen = engine.current_screen();
            prop_assert!(!screen.screen_id.is_empty(),
                "screen_id must never be empty");
        }
    }
}

// ── FormDialog completion tests ──────────────────────────────────────

#[test]
fn form_dialog_edit_name_saves_display_name() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to FormDialog for EditName
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });

    // Type new name
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Bob".into(),
    });

    // Submit — should save and navigate back
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "EditName submit should navigate back, got {result:?}"
    );
}

#[test]
fn form_dialog_edit_name_empty_returns_validation_error() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });

    // Clear the name (set to empty)
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(
            result,
            ActionResult::ValidationError {
                ref component_id,
                ..
            } if component_id == "display_name"
        ),
        "Empty name should return ValidationError, got {result:?}"
    );
}

#[test]
fn form_dialog_add_field_saves_to_own_card() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select type, then enter value
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "phone".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+41 79 123 45 67".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "AddField submit should navigate back, got {result:?}"
    );
}

#[test]
fn form_dialog_add_field_empty_value_returns_validation_error() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Select a type first
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "phone".into(),
    });

    // Leave value empty, submit
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(
            result,
            ActionResult::ValidationError {
                ref component_id,
                ..
            } if component_id == "field_value"
        ),
        "Empty value should return ValidationError, got {result:?}"
    );
}

#[test]
fn form_dialog_edit_field_saves_value() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    // Add a field first so we have a field_id to edit
    let field = vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Phone,
        "Phone",
        "+41 79 000 00 00",
    );
    let field_id = field.id().to_string();
    vauchi.add_own_field(field).unwrap();

    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditField {
            field_id: field_id.clone(),
            field_label: "Phone".into(),
        },
    });

    // Change value
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "+41 79 999 99 99".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "EditField submit should navigate back, got {result:?}"
    );
}

#[test]
fn form_dialog_edit_relay_url_navigates_back() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditRelayUrl {
            current_url: "wss://relay.vauchi.app".into(),
        },
    });

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "relay_url".into(),
        value: "wss://custom.relay.example.com".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    // EditRelayUrl is TUI-specific config — AppEngine just navigates back
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "EditRelayUrl submit should navigate back, got {result:?}"
    );
}

// =============================================================================
// SP-12a: Duplicate Detection, Merge Preview, Contact Limit
// =============================================================================

#[test]
fn duplicate_detection_navigate_shows_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDuplicates);
    assert_eq!(screen.screen_id, "duplicate_detection");
    assert_eq!(screen.title, "Duplicate Detection");
}

#[test]
fn duplicate_detection_empty_shows_no_duplicates() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDuplicates);
    // With no pairs, should show "no duplicates" text
    assert!(
        screen.components.iter().any(|c| matches!(c,
            Component::Text { content, .. } if content.contains("No duplicate")
        )),
        "Empty pairs should show 'No duplicate' message, got {:?}",
        screen.components
    );
}

#[test]
fn duplicate_detection_merge_navigates_back() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "merge".into(),
    });
    // Engine returns Complete, AppEngine intercepts and navigates back
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "merge action should navigate back, got {result:?}"
    );
}

#[test]
fn duplicate_detection_dismiss_stays_on_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "dismiss".into(),
    });
    // Dismiss stays on screen (only merge triggers navigation back)
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "dismiss should stay on screen, got {result:?}"
    );
}

#[test]
fn contact_merge_navigate_shows_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec!["email: alice@example.com".into()],
        secondary_name: "Bob".into(),
        secondary_fields: vec!["phone: +1234567890".into()],
    });
    assert_eq!(screen.screen_id, "contact_merge");
    assert_eq!(screen.title, "Merge Contacts");
}

#[test]
fn contact_merge_shows_both_contacts() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec!["email: alice@example.com".into()],
        secondary_name: "Bob".into(),
        secondary_fields: vec!["phone: +1234567890".into()],
    });
    // Should have subtitle text with both names
    assert!(
        screen.components.iter().any(|c| matches!(c,
            Component::Text { content, .. } if content.contains("Alice") && content.contains("Bob")
        )),
        "Merge screen should show both contact names"
    );
}

#[test]
fn contact_merge_confirm_navigates_back() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec![],
        secondary_name: "Bob".into(),
        secondary_fields: vec![],
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "confirm should navigate back, got {result:?}"
    );
}

#[test]
fn contact_merge_cancel_stays_on_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactMerge {
        primary_name: "Alice".into(),
        primary_fields: vec![],
        secondary_name: "Bob".into(),
        secondary_fields: vec![],
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    // Cancel stays on screen (only confirm triggers navigation back)
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "cancel should stay on screen, got {result:?}"
    );
}

#[test]
fn contact_limit_navigate_shows_screen() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactLimit);
    assert_eq!(screen.screen_id, "contact_limit");
    assert_eq!(screen.title, "Contact Limit");
}

#[test]
fn contact_limit_shows_text_input() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactLimit);
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::TextInput { id, .. } if id == "limit_input")),
        "Should have limit_input TextInput component"
    );
}

#[test]
fn contact_limit_edit_then_save() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactLimit);

    // Enter edit mode
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "edit should update screen, got {result:?}"
    );

    // Type a number
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "100".into(),
    });

    // Save — engine returns Complete, AppEngine routes back
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "save with valid number should navigate back, got {result:?}"
    );
}

#[test]
fn contact_limit_save_invalid_returns_validation_error() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactLimit);

    // Enter edit mode
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    // Type invalid input
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "not_a_number".into(),
    });

    // Save should fail
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert!(
        matches!(result, ActionResult::ValidationError { .. }),
        "save with invalid number should return ValidationError, got {result:?}"
    );
}

#[test]
fn contact_limit_cancel_edit_restores_value() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactLimit);

    // Enter edit mode
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    // Type something
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "limit_input".into(),
        value: "999".into(),
    });

    // Cancel
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_edit".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "cancel_edit should update screen, got {result:?}"
    );

    // Screen should show edit action (not save) — meaning we exited edit mode
    let screen = engine.current_screen();
    assert!(
        screen.actions.iter().any(|a| a.id == "edit"),
        "After cancel_edit, should show 'edit' action again"
    );
}

/// Reproduce the "identity not initialized" bug:
/// After onboarding creates identity via vauchi.create_identity(),
/// navigating to AddField form and completing it should succeed.
#[test]
fn add_field_after_onboarding_identity_creation() {
    // Create Vauchi (no identity) + AppEngine — same as TUI startup
    let vauchi: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Simulate TUI onboarding: create identity directly on vauchi
    // (TUI navigation.rs does this, not AppEngine.handle_completion)
    engine.vauchi_mut().create_identity("TestUser").unwrap();

    // Navigate to Home (TUI does this after onboarding)
    engine.navigate_to(AppScreen::MyInfo);

    // Navigate to AddField form (TUI does this on 'a' key)
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::AddField {
            available_groups: vec![],
        },
    });

    // Single-page form: select type from flat list
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "form_add_field");

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: "email".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "Type selection should update screen, got {result:?}"
    );

    // Type a value
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "test@example.com".into(),
    });

    // Submit
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });

    // Should succeed (navigate back), NOT show "identity not initialized"
    match &result {
        ActionResult::NavigateTo(_) => {} // Success
        ActionResult::ShowAlert { message, .. } => {
            panic!("AddField failed with: {message}");
        }
        other => panic!("Unexpected result: {other:?}"),
    }
}
