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
