// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `AppEngine` navigation: `navigate_to`,
//! `navigate_back`, and the bootstrap-only `set_initial_screen`.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn engine_no_identity() -> AppEngine {
    let vauchi = Vauchi::in_memory().unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn navigate_to_pushes_history() {
    let mut engine = engine_with_identity();
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);

    engine.navigate_to(AppScreen::Settings);
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);

    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "navigate_back should pop MyInfo from history"
    );
}

// @internal
#[test]
fn navigate_back_chain_returns_through_history() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::More);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::Privacy);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::More);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);
}

// Consent toggle on the Privacy screen flips the rendered toggle AND
// persists via the AppEngine intercept (`persist_consent_toggle`), so a
// fresh GdprEngine on revisit reads the granted state from storage.
// @internal
#[test]
fn consent_toggle_persists_and_reflects_on_revisit() {
    use vauchi_app::ui::{Component, SettingsItemKind};

    fn dp_enabled(engine: &AppEngine) -> bool {
        let screen = engine.current_screen();
        let items = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::SettingsGroup { id, items, .. } if id == "consent" => Some(items),
                _ => None,
            })
            .expect("consent SettingsGroup should be rendered");
        items
            .iter()
            .find(|i| i.id == "data_processing")
            .map(|i| matches!(i.kind, SettingsItemKind::Toggle { enabled } if enabled))
            .unwrap_or(false)
    }

    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "consent_actions".into(),
        item_id: "manage_consent".into(),
    });
    assert!(!dp_enabled(&engine), "data_processing starts off");

    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "consent".into(),
        item_id: "data_processing".into(),
    });
    assert!(
        dp_enabled(&engine),
        "engine reflects the toggle immediately"
    );

    // Leave and return: a fresh GdprEngine must read the persisted grant.
    engine.navigate_back();
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "consent_actions".into(),
        item_id: "manage_consent".into(),
    });
    assert!(
        dp_enabled(&engine),
        "consent grant persisted via the intercept and re-read on revisit"
    );
}

// P0 (settings-toggle-not-persisting): toggling notifications/contact_added
// must update the persisted Vauchi config — not just the cached SettingsEngine
// view that drives in-session render. `persist_settings_toggle` matched only
// `component_id == "privacy"`, so contact_added never reached `config_mut`,
// leaving the restart-seed (P1) and every `config()`-reading behaviour stale.
// @internal
#[test]
fn contact_added_toggle_updates_vauchi_config() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    assert!(
        !engine.vauchi().config().contact_added_notifications,
        "config default is false"
    );

    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy_notifications".into(),
        item_id: "contact_added".into(),
    });

    assert!(
        engine.vauchi().config().contact_added_notifications,
        "contact_added toggle must persist to vauchi config"
    );
}

// P2 (settings-toggle-not-persisting): the toggle must persist to durable
// core `SettingsFlags` storage (not just in-memory `config_mut`), so the
// choice survives restart via the P1 self-seed on the next `Vauchi::new`.
// @internal
#[test]
fn settings_toggle_persists_to_settings_flags() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);

    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy_notifications".into(),
        item_id: "suppress_presence".into(),
    });
    let flags = engine.vauchi().load_settings_flags().unwrap();
    assert!(
        flags.suppress_presence,
        "suppress_presence persisted to durable SettingsFlags"
    );

    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy_notifications".into(),
        item_id: "contact_added".into(),
    });
    let flags = engine.vauchi().load_settings_flags().unwrap();
    assert!(
        flags.contact_added_notifications,
        "contact_added persisted to durable SettingsFlags"
    );
}

// GDPR export action returns the serialized data via GdprExportComplete
// (core performs export_all_data; the frontend persists the payload).
// @internal
#[test]
fn gdpr_export_returns_serialized_data() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Privacy);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "export".into(),
    });
    match result {
        ActionResult::GdprExportComplete { json } => {
            assert!(!json.is_empty(), "export json should be non-empty");
            assert!(json.contains('{'), "payload should be JSON");
        }
        other => panic!("Expected GdprExportComplete, got {other:?}"),
    }
}

// Deletion schedule + cancel performed in core via the engine path:
// confirm_delete schedules; cancel_deletion (shown when scheduled) clears.
// @internal
#[test]
fn deletion_schedule_and_cancel_round_trip() {
    use vauchi_core::api::DeletionManager;
    use vauchi_core::storage::DeletionState;

    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete".into(),
    });
    assert!(
        matches!(
            DeletionManager::new(engine.vauchi().storage())
                .deletion_state()
                .unwrap(),
            DeletionState::Scheduled { .. }
        ),
        "confirm_delete schedules deletion in core"
    );

    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_deletion".into(),
    });
    assert!(
        matches!(
            DeletionManager::new(engine.vauchi().storage())
                .deletion_state()
                .unwrap(),
            DeletionState::None
        ),
        "cancel_deletion clears the scheduled deletion"
    );
}

// Panic shred via the engine path wipes all data and returns WipeComplete.
// @internal
#[test]
fn panic_shred_wipes_and_returns_wipe_complete() {
    let mut engine = engine_with_identity();
    assert!(
        engine.vauchi().identity().is_some(),
        "identity exists before shred"
    );
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "panic_shred".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_shred".into(),
    });
    assert!(
        matches!(result, ActionResult::WipeComplete),
        "shred returns WipeComplete"
    );
    assert!(
        engine.vauchi().identity().is_none(),
        "identity is wiped after shred"
    );
}

// Execute-after-grace via the engine path: schedule with an elapsed grace,
// then execute → WipeComplete.
// @internal
#[test]
fn execute_deletion_after_grace_returns_wipe_complete() {
    use vauchi_core::api::DeletionManager;
    let mut engine = engine_with_identity();
    DeletionManager::new(engine.vauchi().storage())
        .schedule_deletion_with_execute_at(1, 1)
        .unwrap();
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "execute_deletion".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_execute".into(),
    });
    assert!(
        matches!(result, ActionResult::WipeComplete),
        "execute after grace returns WipeComplete"
    );
}

// @internal
#[test]
fn navigate_back_with_empty_history_returns_my_info() {
    let mut engine = engine_with_identity();
    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "empty history should fall back to MyInfo"
    );
}

// @internal
#[test]
fn can_go_back_reflects_nav_history_state() {
    // The frontend's BackHandler must stop gating on
    // `coreScreenIdToVariant(...) != null` (the CoreScreenIdMap oracle)
    // and instead ask core whether a back step exists. `can_go_back`
    // is that query: it tracks `nav_history` non-emptiness.
    let mut engine = engine_with_identity();
    assert!(
        !engine.can_go_back(),
        "fresh engine on MyInfo with empty history cannot go back"
    );

    engine.navigate_to(AppScreen::Settings);
    assert!(
        engine.can_go_back(),
        "after navigating forward, a back step exists"
    );

    engine.navigate_back();
    assert!(
        !engine.can_go_back(),
        "after backing out to the root, no further back step exists"
    );
}

// @internal
#[test]
fn open_settings_chrome_action_navigates_to_settings() {
    // The native top-bar gear forwards a reserved global-chrome
    // `ActionPressed { action_id: "open_settings" }` instead of
    // constructing the "Settings" screen name (CoreScreenIdMap rework
    // Tier-0, item 2: chrome nav is core-resolved). Core intercepts it
    // before per-screen dispatch and resolves to NavigateTo(Settings).
    let mut engine = engine_with_identity();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_settings".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(
            screen.screen_id, "settings",
            "open_settings must resolve to the Settings screen"
        ),
        other => panic!("expected NavigateTo(settings), got {other:?}"),
    }
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);
}

// @internal
#[test]
fn can_go_back_false_at_root_even_with_nonempty_history() {
    // Roots (Onboarding + the five mobile bottom-nav tabs) are
    // back-stoppers: pressing back at a root exits the app rather
    // than popping `nav_history`. The bug this guards against is
    // the post-onboarding handoff — onboarding completion lands the
    // user on `MyInfo` with onboarding crumbs still in `nav_history`,
    // and a naive `nav_history`-only `can_go_back` would offer a
    // back affordance at the home tab. The rule is a screen
    // property (`AppScreen::is_root`), not a history property.
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::MyInfo);
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);
    // `navigate_to_pushes_history` verifies that the two navigates above
    // left `nav_history` non-empty (MyInfo, then Settings). Under the
    // history-only `can_go_back` this would return `true` — the new
    // root-aware rule must return `false`.
    assert!(
        !engine.can_go_back(),
        "tab root must not offer back even when nav_history is non-empty"
    );
}

// @internal
#[test]
fn can_go_back_false_at_every_root_screen() {
    // Each declared root must report `can_go_back == false` after
    // we put a non-root above it in history. Drift catcher: adding
    // a sixth bottom-nav tab without updating `AppScreen::is_root`
    // would let the contact-tap-style fall-through reappear.
    for root in [
        AppScreen::Onboarding,
        AppScreen::MyInfo,
        AppScreen::Contacts,
        AppScreen::Exchange,
        AppScreen::Groups,
        AppScreen::More,
    ] {
        let mut engine = engine_with_identity();
        engine.navigate_to(AppScreen::Settings);
        engine.navigate_to(root.clone());
        assert!(
            !engine.can_go_back(),
            "root {root:?} must not offer back with non-empty history"
        );
    }
}

// @internal
#[test]
fn can_go_back_false_after_set_initial_screen() {
    // set_initial_screen must NOT push history (bootstrap-only), so it
    // must not make `can_go_back` report a phantom back step.
    let mut engine = engine_no_identity();
    engine.set_initial_screen(AppScreen::MyInfo);
    assert!(
        !engine.can_go_back(),
        "set_initial_screen must not pollute nav_history / can_go_back"
    );

    engine.navigate_to(AppScreen::Settings);
    assert!(
        engine.can_go_back(),
        "forward nav after bootstrap enables back"
    );
}

// @internal
#[test]
fn set_initial_screen_does_not_push_history() {
    // Without identity, AppEngine::new initializes to Onboarding.
    // A frontend that detects identity at startup needs to swap to
    // MyInfo without leaving Onboarding in the history.
    let mut engine = engine_no_identity();
    assert_eq!(*engine.current_app_screen(), AppScreen::Onboarding);

    engine.set_initial_screen(AppScreen::MyInfo);
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);

    // First user navigation pushes MyInfo to history.
    engine.navigate_to(AppScreen::Settings);

    // navigate_back should land on MyInfo, NOT Onboarding.
    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "set_initial_screen must not pollute nav_history"
    );
}

// @internal
#[test]
fn set_initial_screen_overwrites_prior_initial() {
    // Multiple calls to set_initial_screen are idempotent — none push.
    let mut engine = engine_no_identity();
    engine.set_initial_screen(AppScreen::MyInfo);
    engine.set_initial_screen(AppScreen::Lock);
    assert_eq!(*engine.current_app_screen(), AppScreen::Lock);

    engine.navigate_to(AppScreen::Settings);
    engine.navigate_back();
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::Lock,
        "navigate_back lands on the most recent set_initial_screen target"
    );
}

// @internal
#[test]
fn navigate_to_after_set_initial_pushes_initial() {
    // The initial screen IS the legitimate prior screen for the first
    // user navigation — pushing it to history is correct.
    let mut engine = engine_no_identity();
    engine.set_initial_screen(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::Privacy);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);

    engine.navigate_back();
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);
}

// Regression: the Settings "Version" row was rendering with an empty
// value because `SettingsConfig::version` was hardcoded to `String::new()`
// at the engine-construction site. Captured 2026-05-08 during the device
// test campaign — every frontend (Pixel/Samsung/iOS) showed a labelled
// row with no value. Source: `_private/docs/investigations/2026-05-08-device-test-campaign-findings.md`
// F-MED-2.
// @internal
#[test]
fn settings_screen_version_row_has_non_empty_value() {
    use vauchi_app::ui::{Component, SettingsItemKind};

    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    let screen = engine.current_screen();

    let version_item = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SettingsGroup { items, .. } => items.iter().find(|i| i.id == "version"),
            _ => None,
        })
        .expect("Settings screen must contain a `version` SettingsItem");

    match &version_item.kind {
        SettingsItemKind::Value { value } => {
            assert!(
                !value.is_empty(),
                "Settings → Version value must not be empty (would render as a labelled row with no value to the user)",
            );
            // We don't pin the exact format because the value is a
            // semver and the workspace bumps regularly; just assert it
            // *looks* like a version (digit somewhere) rather than
            // matching a specific build.
            assert!(
                value.chars().any(|c| c.is_ascii_digit()),
                "Settings → Version value should contain at least one digit, got: {value:?}",
            );
        }
        other => panic!("Settings → Version must be SettingsItemKind::Value, got {other:?}"),
    }
}
