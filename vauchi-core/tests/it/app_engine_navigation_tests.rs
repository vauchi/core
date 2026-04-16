// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine navigation tests: screen routing, cache, history, back navigation,
//! available_screens, persistence, default_screen.

use vauchi_app::ui::{ActionResult, ActionStyle, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

// @internal
#[test]
fn navigate_to_home_shows_home_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::MyInfo);
    assert_eq!(screen.screen_id, "my_info");
}

// @internal
#[test]
fn navigate_to_contacts_shows_contact_list() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(screen.screen_id, "contact_list");
}

// @internal
#[test]
fn navigate_to_settings_shows_settings() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Settings);
    assert_eq!(screen.screen_id, "settings");
}

// @internal
#[test]
fn navigate_to_exchange_shows_mode_selection() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Exchange);
    // Mode selection is the first screen when no mode is pre-set
    assert_eq!(screen.screen_id, "exchange_mode_selection");
}

// @internal
#[test]
fn navigate_to_help_shows_help() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Help);
    assert_eq!(screen.screen_id, "help");
}

// @internal
#[test]
fn navigate_to_lock_shows_lock() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Lock);
    assert_eq!(screen.screen_id, "lock_screen");
}

// @internal
#[test]
fn navigate_to_emergency_shred_shows_warning() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::EmergencyShred);
    assert_eq!(screen.screen_id, "shred_warning");
}

// @internal
#[test]
fn navigate_to_backup_shows_backup() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Backup);
    assert_eq!(screen.screen_id, "backup_choose");
}

// ── persistence tests ───────────────────────────────────────────────

// @internal
#[test]
fn app_engine_detects_persisted_identity() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    // Create identity with file-backed storage, then drop it
    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut vauchi = Vauchi::new(config).unwrap();
        vauchi.create_identity("Persisted User").unwrap();
        assert!(vauchi.has_identity());
    }

    // Open a fresh Vauchi from the same path — identity should be loaded
    let config2 = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
    let vauchi2 = Vauchi::new(config2).unwrap();
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

// @internal
#[test]
fn available_screens_without_identity_is_onboarding_only() {
    let vauchi = Vauchi::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    let screens = engine.available_screens();
    assert_eq!(screens, vec![AppScreen::Onboarding]);
}

// @internal
#[test]
fn available_screens_with_identity_has_five_tab_nav() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    let screens = engine.available_screens();
    assert_eq!(
        screens,
        vec![
            AppScreen::MyInfo,
            AppScreen::Contacts,
            AppScreen::Exchange,
            AppScreen::Groups,
            AppScreen::More,
        ],
        "L2b navigation: 5-tab model [MyInfo, Contacts, Exchange, Groups, More]"
    );
    assert!(!screens.contains(&AppScreen::Onboarding));
    assert!(!screens.contains(&AppScreen::Settings));
    assert!(!screens.contains(&AppScreen::Help));
}

// ── failure-path tests for create_engine edge cases ─────────────────

// @internal
#[test]
fn navigate_to_exchange_without_identity_card() {
    // Create Vauchi but don't create identity — own_card() returns None
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Should be on onboarding, navigate to exchange anyway
    let screen = engine.navigate_to(AppScreen::Exchange);
    assert!(!screen.screen_id.is_empty());
}

// @internal
#[test]
fn navigate_to_settings_without_identity() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::Settings);
    assert!(!screen.screen_id.is_empty());
}

// ── engine cache tests ──────────────────────────────────────────────

// @internal
#[test]
fn navigate_away_and_back_preserves_engine_state() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Exchange — starts on mode selection
    let first_visit = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(first_visit.screen_id, "exchange_mode_selection");

    // Pick Glance mode via ListItemSelected → advances to QR
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:glance".into(),
    });
    let qr_screen = engine.current_screen();
    assert_eq!(
        qr_screen.screen_id, "exchange_show_qr",
        "after mode pick, should be at show_qr"
    );

    // Advance to scan
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

// @internal
#[test]
fn onboarding_engine_not_cached() {
    let vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn lock_screen_engine_not_cached() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn navigate_creates_fresh_engine_first_time() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// ── cache invalidation tests ─────────────────────────────────────────

// @internal
#[test]
fn invalidate_screen_removes_cached_engine() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn invalidate_all_clears_entire_cache() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// ── navigation history tests (HIGH-5) ────────────────────────────────

// @internal
#[test]
fn back_from_contact_detail_returns_to_contacts() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn navigate_back_from_duress_pin_returns_to_settings() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn navigate_back_from_settings_returns_to_home() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn back_with_empty_history_returns_to_home() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Without any navigation, back should go to Home (fallback)
    let screen = engine.navigate_back();
    assert_eq!(screen.screen_id, "my_info");
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

// @internal
#[test]
fn navigate_back_does_not_create_circular_history() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// ── Wave 6 Phase A: new engine navigation tests ──────────────────────

// @internal
#[test]
fn navigate_to_sync_shows_sync_status() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn navigate_to_recovery_shows_recovery_status() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn navigate_to_privacy_shows_privacy_settings() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn navigate_to_support_shows_support() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn sync_engine_unknown_action_returns_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// @internal
#[test]
fn privacy_engine_text_changed_is_noop() {
    let mut vauchi = Vauchi::in_memory().unwrap();
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

// ── default_screen tests (Phase 1: Navigation IA) ──────────────────

// @internal
#[test]
fn default_screen_is_my_info_when_no_contacts() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.default_screen(), AppScreen::MyInfo);
}

// @internal
#[test]
fn default_screen_is_contacts_when_has_contacts() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([2u8; 32], card, shared_key);
    vauchi.add_contact(contact).unwrap();

    let engine = AppEngine::new(vauchi);
    assert_eq!(engine.default_screen(), AppScreen::Contacts);
}

// @internal
#[test]
fn default_screen_is_onboarding_without_identity() {
    let vauchi = Vauchi::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    // Without identity, default_screen returns Onboarding so frontends
    // that call navigate_to(default_screen()) don't bypass onboarding.
    assert_eq!(engine.default_screen(), AppScreen::Onboarding);
}

/// CABI completeness canary: every non-parameterized AppScreen must roundtrip
/// through `screen_id()` → `from_screen_id()`.
///
/// When a new AppScreen variant is added, this test will fail if `from_screen_id`
/// isn't updated — which means CABI consumers (Linux-Qt, Windows) can't navigate
/// to it. This prevents the "missing 6 screens" problem from recurring.
///
/// See: Gate 6 of frontend architecture audit (2026-03-18)
// @internal
#[test]
fn cabi_completeness_all_simple_screens_roundtrip_via_screen_id() {
    // All non-parameterized AppScreen variants that CABI must support.
    // Parameterized variants (ContactDetail, ContactEdit, etc.) require
    // additional data and are accessed via handle_action, not navigate_to.
    let simple_screens = vec![
        AppScreen::Onboarding,
        AppScreen::MyInfo,
        AppScreen::Contacts,
        AppScreen::Exchange,
        AppScreen::Settings,
        AppScreen::Help,
        AppScreen::Backup,
        AppScreen::Lock,
        AppScreen::DeviceLinking,
        AppScreen::DuressPin,
        AppScreen::EmergencyShred,
        AppScreen::DeliveryStatus,
        AppScreen::Sync,
        AppScreen::Recovery,
        AppScreen::Groups,
        AppScreen::Privacy,
        AppScreen::Support,
        AppScreen::ContactDuplicates,
        AppScreen::ContactLimit,
        AppScreen::More,
    ];

    let mut missing = Vec::new();
    for screen in &simple_screens {
        let id = screen.screen_id();
        if AppScreen::from_screen_id(id).is_none() {
            missing.push(id);
        }
    }

    assert!(
        missing.is_empty(),
        "AppScreen::from_screen_id() does not handle these screen IDs: {:?}\n\
         CABI consumers (Linux-Qt, Windows) cannot navigate to these screens.\n\
         Add them to from_screen_id() in app_engine/mod.rs.",
        missing
    );
}

// ── L2b MoreEngine tests ─────────────────────────────────────────────

// @internal
#[test]
fn navigate_to_more_shows_more_menu() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::More);
    assert_eq!(screen.screen_id, "more");
    assert_eq!(screen.title, "More");
    assert!(
        !screen.components.is_empty(),
        "More screen must have at least one component"
    );
}

// @internal
#[test]
fn more_engine_has_expected_navigation_targets() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::More);

    // Extract action IDs from the ActionList component
    let action_ids: Vec<String> = screen
        .components
        .iter()
        .flat_map(|c| match c {
            vauchi_app::ui::Component::ActionList { items, .. } => {
                items.iter().map(|item| item.id.clone()).collect::<Vec<_>>()
            }
            _ => vec![],
        })
        .collect();

    assert!(
        action_ids.contains(&"sync".to_string()),
        "More must contain sync"
    );
    assert!(
        action_ids.contains(&"activity_log".to_string()),
        "More must contain activity_log"
    );
    assert!(
        action_ids.contains(&"device_linking".to_string()),
        "More must contain device_linking"
    );
    assert!(
        action_ids.contains(&"settings".to_string()),
        "More must contain settings"
    );
    assert!(
        action_ids.contains(&"backup".to_string()),
        "More must contain backup"
    );
    assert!(
        action_ids.contains(&"privacy".to_string()),
        "More must contain privacy"
    );
    assert!(
        action_ids.contains(&"help".to_string()),
        "More must contain help"
    );
    assert_eq!(action_ids.len(), 7, "More menu should have exactly 7 items");
}

// @internal
#[test]
fn more_engine_routes_to_settings() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::More);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "settings".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "settings");
        }
        other => panic!("Expected NavigateTo settings, got {other:?}"),
    }
    assert_eq!(engine.current_app_screen(), &AppScreen::Settings);
}

// @internal
#[test]
fn more_engine_routes_to_help() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::More);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "help".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "help");
        }
        other => panic!("Expected NavigateTo help, got {other:?}"),
    }
    assert_eq!(engine.current_app_screen(), &AppScreen::Help);
}

// @internal
// @internal
#[test]
fn more_engine_routes_to_activity_log() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::More);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "activity_log".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "activity_log");
        }
        other => panic!("Expected NavigateTo activity_log, got {other:?}"),
    }
    assert_eq!(engine.current_app_screen(), &AppScreen::ActivityLog);
}

// @internal
#[test]
fn more_engine_unknown_action_returns_update_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::More);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent_screen".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "more");
        }
        other => panic!("Expected UpdateScreen for unknown action, got {other:?}"),
    }
}

// @internal
#[test]
fn from_screen_id_more_roundtrips() {
    let screen = AppScreen::More;
    let id = screen.screen_id();
    assert_eq!(id, "more");
    let parsed = AppScreen::from_screen_id(id).expect("from_screen_id must handle 'more'");
    assert_eq!(parsed, AppScreen::More);
}

// ============================================================================
// Empty state: go_exchange action routing
// ============================================================================

// @scenario: onboarding.feature - Empty state with guidance
// @internal
#[test]
fn go_exchange_from_contacts_navigates_to_exchange() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "go_exchange".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "exchange_mode_selection",
                "go_exchange should navigate to exchange screen"
            );
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}

// @scenario: onboarding.feature - Prompt for first exchange
// @internal
#[test]
fn go_exchange_from_my_info_navigates_to_exchange() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "go_exchange".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "exchange_mode_selection",
                "go_exchange should navigate to exchange screen"
            );
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}
