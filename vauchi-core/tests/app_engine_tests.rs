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
