// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tier-0 (d): top-level/tab navigation is a `UserAction`, not a
//! frontend-constructed target. A tab tap forwards
//! `UserAction::NavigateToTab { action_id }` carrying an opaque token core
//! minted (the tab's `action_id`); `AppEngine::handle_action` intercepts it
//! before per-screen dispatch and resolves it to `NavigateTo(ScreenModel)`.
//! This replaces the `navigate_to_json` / `navigate_to_param` forward-navigate
//! surface (ADR-043 Amendment 4 §1/§5).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The action token for every top-level tab (`available_screens()`) resolves
/// through `NavigateToTab` to the **same** screen as canonical navigation.
/// Derived from `available_screens()` so the real tab set is covered, not a
/// hardcoded list.
// @internal
#[test]
fn navigate_to_tab_routes_each_tab_to_canonical_screen() {
    let tab_ids: Vec<String> = engine_with_identity()
        .available_screens()
        .iter()
        .map(|s| s.screen_id().to_string())
        .collect();
    assert!(
        tab_ids.len() >= 2,
        "expected several top-level tabs, got {tab_ids:?}"
    );

    for id in tab_ids {
        // Reference: what canonical navigation to this id produces.
        let mut reference = engine_with_identity();
        let target = AppScreen::from_screen_id(&id)
            .unwrap_or_else(|| panic!("tab action_id `{id}` must parse to an AppScreen"));
        let expected_screen_id = reference.navigate_to(target).screen_id;

        // Under test: the same id forwarded as a NavigateToTab action.
        let mut engine = engine_with_identity();
        let result = engine.handle_action(UserAction::NavigateToTab {
            action_id: id.clone(),
        });
        match result {
            ActionResult::NavigateTo(screen) => assert_eq!(
                screen.screen_id, expected_screen_id,
                "NavigateToTab({id}) must land on the same screen as canonical navigation"
            ),
            other => panic!("NavigateToTab({id}) must return NavigateTo, got {other:?}"),
        }
    }
}

/// An `action_id` core never minted (adversarial / stale token) must not
/// navigate to a wrong screen and must leave the engine where it was — no
/// silent wrong-nav (CC-03), mirroring the `route_result` unknown-id
/// precedent (`routing.rs`, `from_screen_id` → `None` → `UpdateScreen`).
// @internal
#[test]
fn navigate_to_tab_unknown_token_does_not_navigate() {
    let mut engine = engine_with_identity();
    // Land on a known screen first (set up without the unit under test).
    engine.navigate_to(AppScreen::Contacts);
    let before = engine.current_screen().screen_id;

    let result = engine.handle_action(UserAction::NavigateToTab {
        action_id: "definitely_not_a_screen_xyz".to_string(),
    });
    if let ActionResult::NavigateTo(screen) = &result {
        panic!(
            "unknown NavigateToTab token must not navigate, got NavigateTo({})",
            screen.screen_id
        );
    }
    assert_eq!(
        engine.current_screen().screen_id,
        before,
        "unknown NavigateToTab token must leave the current screen unchanged"
    );
}

/// Every `TabInfo` from `tab_info()` carries an `action_id` that the frontend
/// forwards verbatim; routing it through `NavigateToTab` navigates. The wire
/// field and the routing token are the same opaque value, so the frontend
/// never constructs a navigation target.
// @internal
#[test]
fn tab_info_action_id_round_trips_through_navigate_to_tab() {
    let tabs = engine_with_identity().tab_info(vauchi_app::Locale::English);
    assert!(
        !tabs.is_empty(),
        "expected top-level tabs after identity creation"
    );
    for tab in tabs {
        assert!(
            !tab.action_id.is_empty(),
            "TabInfo `{}` must carry a non-empty action_id",
            tab.id
        );
        let mut engine = engine_with_identity();
        let result = engine.handle_action(UserAction::NavigateToTab {
            action_id: tab.action_id.clone(),
        });
        assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "forwarding TabInfo.action_id `{}` must navigate",
            tab.action_id
        );
    }
}

/// Regression: only the typed `NavigateToTab` triggers tab routing. An
/// `ActionPressed` whose `action_id` happens to equal a canonical screen_id
/// (`"groups"`) must dispatch to the current screen's engine, never navigate —
/// guarding the dispatch-lane separation the interception relies on.
// @internal
#[test]
fn action_pressed_with_screen_like_id_is_not_tab_navigation() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Contacts);
    let before = engine.current_screen().screen_id;

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "groups".to_string(),
    });
    if let ActionResult::NavigateTo(screen) = &result {
        assert_ne!(
            screen.screen_id, "groups",
            "ActionPressed must not be routed as tab navigation"
        );
    }
    assert_eq!(
        engine.current_screen().screen_id,
        before,
        "ActionPressed with a screen-like id must leave the screen unchanged"
    );
}

/// Regression: `navigate_back` is unaffected by the NavigateToTab
/// interception — forward via the action, then pop returns to the prior tab.
// @internal
#[test]
fn navigate_back_after_navigate_to_tab_returns_to_prior_screen() {
    let mut engine = engine_with_identity();
    let _ = engine.handle_action(UserAction::NavigateToTab {
        action_id: "contacts".to_string(),
    });
    let contacts_id = engine.current_screen().screen_id;
    let _ = engine.handle_action(UserAction::NavigateToTab {
        action_id: "groups".to_string(),
    });
    assert_eq!(engine.current_screen().screen_id, "groups");

    engine.navigate_back();
    assert_eq!(
        engine.current_screen().screen_id,
        contacts_id,
        "navigate_back must return to the screen visited before the last NavigateToTab"
    );
}

/// The home tab is identified by a UI-shaped `is_home` flag, not by
/// hardcoding the domain screen id. Exactly one tab is home, and it is
/// the "my_info" / My Card tab.
// @internal
#[test]
fn tab_info_flags_exactly_one_home_tab() {
    let tabs = engine_with_identity().tab_info(vauchi_app::Locale::English);
    assert!(
        !tabs.is_empty(),
        "expected top-level tabs after identity creation"
    );

    let home_tabs: Vec<&vauchi_app::ui::TabInfo> = tabs.iter().filter(|t| t.is_home).collect();
    assert_eq!(
        home_tabs.len(),
        1,
        "exactly one tab must be marked as home, got {home_tabs:?}"
    );
    assert_eq!(
        home_tabs[0].id, "my_info",
        "the home tab must be the My Card / my_info tab"
    );

    for tab in &tabs {
        if tab.id != "my_info" {
            assert!(
                !tab.is_home,
                "non-home tab `{}` must not have is_home == true",
                tab.id
            );
        }
    }
}
