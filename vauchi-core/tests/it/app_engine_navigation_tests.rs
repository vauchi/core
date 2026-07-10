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
use vauchi_core::exchange::mode::ExchangeMode;

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
    assert_eq!(screen.screen_id, "contacts");
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
    // The mode-selection root is the first screen; it reports the canonical
    // tab-root id `exchange` (so frontends render the bottom nav bar).
    assert_eq!(screen.screen_id, "exchange");
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
// "Link New Device" on the DeviceManagement screen returns a typed
// `StartDeviceLink { role: Initiator }` ActionResult. AppEngine routes
// the initiator role to the core-driven DeviceLinkingEngine, so
// frontends only need to render the resulting screen.
// @internal
#[test]
fn link_new_device_from_device_management_navigates_to_device_linking() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::DeviceManagement);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "link_show_qr",
                "tapping link_device on DeviceManagement should land on the link_show_qr screen, got {}",
                screen.screen_id
            );
        }
        other => panic!(
            "expected NavigateTo(link_show_qr), got {other:?} — \
             AppEngine routing for StartDeviceLink(Initiator) missing"
        ),
    }
}

// @internal
// Onboarding's "link_device" now navigates straight to the instruction
// screen; the scan button there emits `Command::QrRequestScan` directly.
// No `StartDeviceLink` result crosses the boundary for this path
// (`2026-07-06-mobile-domain-shell-violations` I9).
#[test]
fn onboarding_link_device_navigates_to_device_link_instructions() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Onboarding);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(
                screen.screen_id, "device_link_instructions",
                "tapping link_device on onboarding should land on instructions, got {}",
                screen.screen_id
            );
        }
        other => panic!("expected NavigateTo(device_link_instructions), got {other:?}"),
    }
}

// @internal
#[test]
fn navigate_to_backup_shows_backup() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Backup);
    assert_eq!(screen.screen_id, "backup");
}

// ── persistence tests ───────────────────────────────────────────────

// @internal
#[test]
fn app_engine_detects_persisted_identity() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

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
    // A group makes the picker advance to the in-engine GroupSelection
    // sub-step, which is what this test rides to prove engine-state caching.
    // Every transport mode now graduates to its own dedicated screen
    // (Glance/Hover/THS → MultiStageExchange, Magic/Bump/Shake → BleExchange,
    // TapTap → NfcExchange, Link → LinkExchange), so the only sub-steps left
    // *inside* the ExchangeEngine are GroupSelection / FieldPreview.
    vauchi.create_group("Family").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Navigate to Exchange — G4 group-first: with a group present the
    // engine starts directly on the in-engine group-selection sub-step.
    let first_visit = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(first_visit.screen_id, "exchange_group_selection");

    // Navigate away to Home — the Exchange engine should be cached at the
    // group-selection sub-step.
    let home = engine.navigate_to(AppScreen::MyInfo);
    assert_eq!(home.screen_id, "my_info");

    // Navigate back to Exchange — should restore the cached engine on its
    // sub-step, NOT reset.
    let restored = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(
        restored.screen_id, "exchange_group_selection",
        "cached engine should preserve internal state (group step)"
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

    let contacts = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(contacts.screen_id, "contacts");

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

    engine.invalidate_screen(&AppScreen::Contacts);

    // Navigate back — should get fresh engine (not the cached one)
    let screen = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(screen.screen_id, "contacts");
}

// @internal
#[test]
fn invalidate_all_clears_entire_cache() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::MyInfo);

    engine.invalidate_all();

    let contacts = engine.navigate_to(AppScreen::Contacts);
    assert_eq!(contacts.screen_id, "contacts");
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
        screen.screen_id, "contacts",
        "back should navigate to the canonical contacts screen"
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

// M4 S2 (2026-07-03-sync-surface-placebo): the standalone Sync screen was
// retired (the chrome sync chip is the sync surface). The Settings "Failed
// Deliveries" row — previously a dead Value counter — now links into the
// DeliveryStatus retry screen, which had been reachable by nothing.
// @internal
#[test]
fn failed_deliveries_row_navigates_to_delivery_status() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "delivery".into(),
        item_id: "failed_deliveries".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "delivery_status");
        }
        other => panic!("Expected NavigateTo(delivery_status), got {other:?}"),
    }
}

// @internal
#[test]
fn navigate_to_recovery_shows_recovery_intro() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Recovery);
    assert_eq!(screen.screen_id, "recovery_status");
    assert_eq!(screen.title, "Social Recovery");
    // Fresh identity has 0 contacts, quorum not met — Start Recovery
    // Process disabled. Action ID is `start_recovery_process` on the
    // new Intro step (was `start_recovery` on the legacy Status step,
    // which is now reached after creating a claim).
    assert!(
        screen
            .actions
            .iter()
            .any(|a| a.id == "start_recovery_process" && !a.enabled),
        "Recovery Intro must offer start_recovery_process and disable it when quorum not met, got actions: {:?}",
        screen
            .actions
            .iter()
            .map(|a| (&a.id, a.enabled))
            .collect::<Vec<_>>()
    );
}

// @internal
#[test]
fn navigate_to_recovery_claim_review_shows_review() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::RecoveryClaimReview);
    assert_eq!(screen.screen_id, "recovery_claim_review");
    assert!(
        screen.actions.iter().any(|a| a.id == "reject"),
        "Recovery claim review must have reject action"
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
    let contact = Contact::from_exchange([2u8; 32], card, shared_key, 0);
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
            vauchi_app::ui::Component::SectionedActionList { sections, .. } => sections
                .iter()
                .flat_map(|s| s.items.iter().map(|i| i.id.clone()))
                .collect(),
            _ => vec![],
        })
        .collect();

    // Expected items as of 2026-05-03 — extended for Android's
    // (`2026-05-01-more-engine-extension-android-retirement`):
    // dropped `device_linking` (the link flow) in favor of
    // `device_management` (the landing list, more sensible default
    // tap target); added the 4 entries Android's MoreScreen exposes
    // that core was missing (device_replacement, recovery,
    // archived_contacts, contact_duplicates).
    //
    // Phase 2A of `2026-05-03-core-file-picker-command` adds
    // `import_contacts` — the only entry that does not navigate to
    // a screen (selecting it returns `Commands` driving the
    // file picker per ADR-031).
    let expected: &[&str] = &[
        "activity_log",
        "device_management",
        "device_replacement",
        "recovery",
        "tags",
        "places",
        "archived_contacts",
        "contact_duplicates",
        "import_contacts",
        "settings",
        "backup",
        "privacy",
        "help",
    ];
    for id in expected {
        assert!(
            action_ids.contains(&id.to_string()),
            "More must contain {id}"
        );
    }
    assert_eq!(
        action_ids.len(),
        expected.len(),
        "More menu should have exactly {} items",
        expected.len()
    );
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
                screen.screen_id, "exchange",
                "go_exchange should navigate to the Exchange tab root (canonical id)"
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
                screen.screen_id, "exchange",
                "go_exchange should navigate to the Exchange tab root (canonical id)"
            );
        }
        other => panic!("Expected NavigateTo, got {other:?}"),
    }
}

// ── Screen-presentation lifecycle hooks (Phase 2b) ────────────────────

// @scenario: exchange.feature :: Multi-stage exchange dims screen, disables idle timer, and locks portrait on entry
#[test]
fn navigate_to_multi_stage_exchange_drains_brightness_idle_timer_and_orientation_commands() {
    use vauchi_core::{Command, Orientation};
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let _ = engine.drain_pending_commands();

    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let commands = engine.drain_pending_commands();

    assert_eq!(
        commands,
        vec![
            Command::SetScreenBrightness { level: Some(0.65) },
            Command::SetIdleTimerDisabled { disabled: true },
            Command::SetOrientationLock {
                orientation: Some(Orientation::Portrait)
            },
            // Phase 1.B of `2026-05-11-hover-graduation-plan.md` —
            // `screen_entered` now announces the engine's chosen camera
            // selector. AppEngine routes `MultiStageExchange` to
            // `MultiStageExchangeEngine::new_glance()` today (Phase 1.E
            // adds the Hover→`new_hover()` mode-dispatch branch), so
            // the announced default is back camera.
            Command::SwitchCamera { use_front: false },
        ],
        "navigate_to(MultiStageExchange) must drive its screen_entered hook"
    );
}

// @scenario: exchange.feature :: Multi-stage exchange restores defaults on exit
#[test]
fn navigate_back_from_multi_stage_exchange_drains_restore_commands() {
    use vauchi_core::Command;
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let _ = engine.drain_pending_commands();

    engine.navigate_back();
    let commands = engine.drain_pending_commands();

    assert!(
        commands.starts_with(&[
            Command::SetScreenBrightness { level: None },
            Command::SetIdleTimerDisabled { disabled: false },
            Command::SetOrientationLock { orientation: None },
        ]),
        "navigate_back from MultiStageExchange must emit screen_exited commands first; got {commands:?}"
    );
}

// @internal
#[test]
fn navigate_to_non_presentation_screen_drains_no_commands() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let _ = engine.drain_pending_commands();

    engine.navigate_to(AppScreen::Settings);
    let commands = engine.drain_pending_commands();

    assert!(
        commands.is_empty(),
        "screens with the empty-default lifecycle hooks must not emit commands; got {commands:?}"
    );
}

// @internal
#[test]
fn drain_pending_commands_returns_empty_after_drain() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let _first = engine.drain_pending_commands();

    let second = engine.drain_pending_commands();
    assert!(
        second.is_empty(),
        "drain consumes the queue; got {second:?}"
    );
}

// ── Audio bridge (Phase 1.C.3d) ───────────────────────────────────

// @internal
#[test]
fn apply_multi_stage_audio_proximity_routes_to_engine_setter() {
    // The bridge AppEngine::apply_multi_stage_audio_proximity must
    // downcast to MultiStageExchangeEngine and forward the state to
    // its set_audio_proximity. Phase 1.C.3d of
    // 2026-05-11-hover-graduation-plan.md.
    use vauchi_core::exchange::AudioProximityState;
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let _ = engine.drain_pending_commands();

    let applied = engine.apply_multi_stage_audio_proximity(AudioProximityState::Listening);
    assert!(
        applied,
        "apply_multi_stage_audio_proximity must succeed when the active engine is MultiStageExchange",
    );

    // Proximity narration was removed from the active screen: the engine
    // no longer renders a StatusIndicator while active (the own-QR label
    // carries the protocol-state caption instead), and proximity progress
    // is intentionally not surfaced. The bridge's only contract here is
    // that the setter is routed to the active engine, asserted above via
    // `applied`. Confirm the active screen exposes no StatusIndicator
    // narration for the Listening proximity state.
    let screen = engine.current_screen();
    let has_status_indicator = screen
        .components
        .iter()
        .any(|c| matches!(c, vauchi_app::ui::Component::StatusIndicator { .. }));
    assert!(
        !has_status_indicator,
        "active multi-stage screen must not render a StatusIndicator after proximity narration removal",
    );
}

// @internal
#[test]
fn apply_multi_stage_audio_proximity_failed_renders_audio_failed_screen() {
    // Audio-Failed routes build_screen to build_audio_failed_screen
    // (distinct chrome "Couldn't confirm devices are close") instead
    // of the generic Exchange-Failed panel. G1.3 of the Hover
    // graduation problem record; verified end-to-end through the
    // AppEngine bridge.
    use vauchi_core::exchange::AudioProximityState;
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let _ = engine.drain_pending_commands();

    let applied = engine.apply_multi_stage_audio_proximity(AudioProximityState::Failed);
    assert!(applied);

    let screen = engine.current_screen();
    let title = screen
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::StatusIndicator { title, .. } => Some(title.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(
        title, "Couldn't confirm devices are close",
        "audio-Failed must render the proximity-specific chrome",
    );
}

// @internal
#[test]
fn apply_multi_stage_audio_proximity_returns_false_on_wrong_engine() {
    // The bridge gracefully reports false when the active engine
    // isn't the multi-stage one — the user navigated away mid-
    // handshake. Phase 1.C.3d's bridge handler drops the callback
    // (no panic, no engine corruption).
    use vauchi_core::exchange::AudioProximityState;
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    // Don't navigate to MultiStageExchange — leave engine on the
    // default screen.
    let applied = engine.apply_multi_stage_audio_proximity(AudioProximityState::Listening);
    assert!(
        !applied,
        "apply_multi_stage_audio_proximity must return false when MultiStageExchange isn't active",
    );
}

// @internal
#[test]
fn is_active_engine_multi_stage_hover_returns_false_for_non_multistage_active() {
    // Phase 1.C polish — `PlatformAppEngine::ensure_multi_stage_session`
    // reads this to decide whether to register the cycle-thread
    // audio listener. False for every non-multi-stage engine
    // (including the default landing screen).
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    assert!(
        !engine.is_active_engine_multi_stage_hover(),
        "default landing engine is never multi-stage / Hover",
    );
}

// @internal
#[test]
fn is_active_engine_multi_stage_hover_returns_false_for_glance_multistage() {
    // Post-1.E.3 (`core!825`) the screen factory at
    // `app_engine/screens.rs:872` matches on
    // `AppScreen::MultiStageExchange { mode }` and dispatches
    // to `new_hover()` / `new_glance()` directly. Glance flows
    // construct `new_glance()`; the Phase 1.C polish gate
    // (`039effe3`) depends on this helper returning `false` for
    // those engines so `PlatformAppEngine` skips audio-listener
    // registration. The Hover-positive leg of the same gate is
    // covered by `hover_mode_routes_through_multi_stage_handoff`
    // in `vauchi-app/src/ui/exchange.rs` tests.
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let _ = engine.drain_pending_commands();
    assert!(
        !engine.is_active_engine_multi_stage_hover(),
        "screens.rs:880 currently constructs Glance via new() — helper must report false until Phase 1.E flips it",
    );
}

// @internal
#[test]
fn extend_pending_commands_appends_to_drain_queue() {
    // Phase 1.C.3e-v of 2026-05-11-hover-graduation-plan.md —
    // PlatformAppEngine's audio-listener bridge forwards
    // session-side audio commands into AppEngine via this method.
    use vauchi_core::{Command, Orientation};
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    // Push two arbitrary commands the bridge would normally forward.
    let pushed = vec![
        Command::SetScreenBrightness { level: Some(0.5) },
        Command::SetOrientationLock {
            orientation: Some(Orientation::Portrait),
        },
    ];
    engine.extend_pending_commands(pushed.clone());

    let drained = engine.drain_pending_commands();
    assert_eq!(
        drained, pushed,
        "extend_pending_commands must push verbatim; drain returns FIFO",
    );
    // Second drain is empty — drain takes-and-clears.
    assert!(engine.drain_pending_commands().is_empty());
}

// @internal
#[test]
fn extend_pending_commands_preserves_existing_queue() {
    // Bridge may extend while AppEngine already has engine-emitted
    // commands buffered (from a prior screen_entered or ActionResult).
    use vauchi_core::Command;
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    // navigate_to leaves screen_entered commands in the queue —
    // brightness, idle-timer, orientation, plus the 1.B-added
    // SwitchCamera announcement. Bridge-emitted audio commands
    // append AFTER these.
    let prior_len = {
        // Peek by draining + re-pushing (since AppEngine has no
        // peek API).
        let cmds = engine.drain_pending_commands();
        let len = cmds.len();
        engine.extend_pending_commands(cmds);
        len
    };
    assert!(prior_len >= 1, "screen_entered must have queued commands");

    engine.extend_pending_commands(vec![Command::AudioStop]);

    let drained = engine.drain_pending_commands();
    assert_eq!(
        drained.len(),
        prior_len + 1,
        "extend_pending_commands must preserve prior queue + append new",
    );
    assert!(
        matches!(drained.last(), Some(Command::AudioStop)),
        "newly-extended command must land at queue tail (FIFO)",
    );
}
