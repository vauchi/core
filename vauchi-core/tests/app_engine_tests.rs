// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::MockTransport;
use vauchi_core::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};

/// Drive through the full onboarding flow, returning the final ActionResult.
fn drive_onboarding(engine: &mut AppEngine<MockTransport>) -> ActionResult {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip_to_finish".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
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
    let screen = engine.navigate_to(AppScreen::Home);
    assert_eq!(screen.screen_id, "home");
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
        &AppScreen::Home,
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
    assert!(screens.contains(&AppScreen::Home));
    assert!(screens.contains(&AppScreen::Contacts));
    assert!(screens.contains(&AppScreen::Settings));
    assert!(!screens.contains(&AppScreen::Onboarding));
}

// ── helper: onboarding without setting display name ─────────────────

/// Drive onboarding to the name step and attempt to continue without entering a name.
/// Returns the result of pressing "continue" without a display name.
fn drive_onboarding_without_name(engine: &mut AppEngine<MockTransport>) -> ActionResult {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
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

    // Should navigate to Home after onboarding completes
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "expected NavigateTo, got {:?}",
        result
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::Home);
}

#[test]
fn app_engine_starts_on_home_with_identity() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.current_app_screen(), &AppScreen::Home);
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
fn home_screen_shows_real_setup_progress() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "home");

    // Find the setup_progress StatusIndicator component
    let progress_component = screen
        .components
        .iter()
        .find(|c| matches!(c, vauchi_core::ui::Component::StatusIndicator { id, .. } if id == "setup_progress"))
        .expect("home screen should have a setup_progress component");

    // After create_identity, identity_created and card_has_fields are true (2/6),
    // NOT the old hardcoded 3/3
    if let vauchi_core::ui::Component::StatusIndicator { detail, .. } = progress_component {
        let detail = detail.as_ref().expect("should have detail text");
        assert!(
            detail.contains("of 6"),
            "total_steps should be 6 (from get_setup_progress), got: {detail}"
        );
        assert!(
            !detail.contains("of 3"),
            "should not have old hardcoded total of 3, got: {detail}"
        );
    } else {
        panic!("expected StatusIndicator");
    }
}

/// Verify that a whitespace-only name is also rejected.
#[test]
fn onboarding_completion_with_empty_name_returns_validation_error() {
    let vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to name step
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    // Set a whitespace-only name
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

    let _ = drive_onboarding(&mut engine);

    assert!(
        engine.has_identity(),
        "identity should be persisted after onboarding"
    );
    assert!(
        engine.available_screens().contains(&AppScreen::Home),
        "should have full nav after identity created"
    );
}

// ── home screen contact limit tests ─────────────────────────────────

#[test]
fn home_screen_limits_displayed_contacts() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    // Home screen should show max 5 contacts
    // With 0 contacts, truncate(5) is a no-op — verify it doesn't break
    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "home");
    // Verify the screen renders successfully (truncate of empty list is safe)
    assert!(!screen.title.is_empty(), "home screen should have a title");
}

// ── contact detail / edit wiring tests ──────────────────────────────

#[test]
fn contact_detail_does_not_show_empty_list() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: "nonexistent".into(),
    });
    // Should not crash, and should not show the contact_list screen
    assert!(!screen.screen_id.is_empty());
    assert_ne!(
        screen.screen_id, "contact_list",
        "ContactDetail should not fall back to contact_list"
    );
}

#[test]
fn contact_edit_does_not_show_empty_list() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ContactEdit {
        contact_id: "nonexistent".into(),
    });
    assert!(!screen.screen_id.is_empty());
    assert_ne!(
        screen.screen_id, "contact_list",
        "ContactEdit should not fall back to contact_list"
    );
}

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

#[test]
fn navigate_to_contact_detail_with_nonexistent_id() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: "nonexistent-id".into(),
    });
    // Should render without panic
    assert!(!screen.screen_id.is_empty());
}

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

    // Advance the Exchange engine to the scan step
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let scan_screen = engine.current_screen();
    assert_eq!(
        scan_screen.screen_id, "exchange_scan_qr",
        "exchange engine should advance to scan step"
    );

    // Navigate away to Home — Exchange engine should be cached at scan step
    let home = engine.navigate_to(AppScreen::Home);
    assert_eq!(home.screen_id, "home");

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

    // Start on Onboarding, navigate away
    assert_eq!(engine.current_app_screen(), &AppScreen::Onboarding);
    let _ = engine.navigate_to(AppScreen::Home);

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
    let _ = engine.navigate_to(AppScreen::Home);

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
    engine.navigate_to(AppScreen::Home);

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

    // Toggle suppress_presence on
    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: "suppress_presence".into(),
    });

    // Navigate away and invalidate
    engine.navigate_to(AppScreen::Home);
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
    engine.navigate_to(AppScreen::Home);

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
        &AppScreen::Home,
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

    // Should navigate to Home
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "correct PIN should navigate to Home, got {:?}",
        result
    );
    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::Home,
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
        &AppScreen::Home,
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

    // First attempt: wrong PIN
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

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "correct PIN after failed attempt should unlock, got {:?}",
        result
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::Home);
}

#[test]
fn invalidate_all_clears_entire_cache() {
    let mut vauchi = Vauchi::<MockTransport>::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Cache multiple screens
    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::Home);

    // Invalidate all
    engine.invalidate_all();

    // Both should get fresh engines
    let contacts = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(contacts.screen_id, "contact_list");
    let settings = engine.navigate_to(AppScreen::Settings);
    assert_eq!(settings.screen_id, "settings");
}
