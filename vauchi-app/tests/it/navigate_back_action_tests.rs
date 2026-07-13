// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Back navigation as a typed `UserAction` + the Back affordance as a
//! reserved `nav_actions` item (allowlist-reduction #2, core half).
//!
//! Symmetric with `navigate_to_tab_tests.rs`: the OS back gesture forwards
//! `UserAction::NavigateBack` through `handle_action` instead of the binding
//! calling `navigate_back_json`, and the back *affordance* state rides on
//! `ScreenModel.nav_actions` instead of a separate `can_go_back()` query.
//! Together these let the `navigate_back_json` + `can_go_back` binding surface
//! retire once both frontends migrate (ADR-043 Amendment 4 / ADR-044 Am2a).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// `handle_action(NavigateBack)` lands exactly where the engine's own
/// `navigate_back()` does — the action is a thin typed wrapper, so the
/// frontend's system-back handler can forward it instead of `navigate_back_json`.
// @internal
#[test]
fn navigate_back_action_matches_navigate_back() {
    // Reference: forward to a sub-screen, then pop directly.
    let mut reference = engine_with_identity();
    reference.navigate_to(AppScreen::Settings);
    let expected = reference.navigate_back().screen_id;

    // Under test: same forward nav, then the typed action.
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    let result = engine.handle_action(UserAction::NavigateBack);

    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(
            screen.screen_id, expected,
            "NavigateBack action must land where navigate_back() does"
        ),
        other => panic!("NavigateBack must return NavigateTo, got {other:?}"),
    }
    assert_eq!(
        engine.current_screen().screen_id,
        expected,
        "the engine must actually be on the popped screen afterwards"
    );
}

/// At a back-stopping root (no back step), the OS back gesture is *not* a
/// phantom re-nav to MyInfo — core returns `PerformNativeBack` so the frontend
/// performs the platform's native default (Android minimize / iOS suspend /
/// desktop no-op). ADR-044 Am2a: back is forwarded unconditionally and core
/// owns the empty-history decision, so the frontend never gates on
/// `can_go_back`.
// @internal
#[test]
fn navigate_back_at_root_returns_perform_native_back() {
    let mut engine = engine_with_identity();
    // A fresh identity lands on MyInfo — a declared root with no back step.
    assert!(!engine.can_go_back(), "MyInfo root offers no back step");

    let result = engine.handle_action(UserAction::NavigateBack);

    assert!(
        matches!(result, ActionResult::PerformNativeBack),
        "NavigateBack at a back-stopping root must return PerformNativeBack, \
         got {result:?}"
    );
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::MyInfo,
        "the engine must stay put — no phantom navigation on native back"
    );
}

/// The visible Back affordance rides on `nav_actions` as a reserved `go_back`
/// action at index 0 — so frontends render Back from data, not from the
/// `can_go_back` bool (ADR-044 Am2a, boolean-family retirement). A non-root
/// sub-screen with history carries it.
// @internal
#[test]
fn sub_screen_renders_go_back_nav_action_at_front() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);

    let sub = engine.current_screen();
    let first = sub
        .nav_actions
        .first()
        .expect("a sub-screen with history must offer a Back nav_action");
    assert_eq!(
        first.id, "go_back",
        "the Back affordance must be the reserved go_back action at index 0"
    );
    assert_eq!(
        first.style,
        vauchi_app::ui::ActionStyle::Secondary,
        "Back is a secondary chrome affordance"
    );
    assert!(first.enabled, "the Back affordance is enabled when offered");
}

/// Dispatching the reserved `go_back` action pops exactly where the engine's
/// own `navigate_back()` does — it shares the `NavigateBack` back logic, so the
/// visible affordance and the OS gesture are one code path.
// @internal
#[test]
fn go_back_action_matches_navigate_back() {
    let mut reference = engine_with_identity();
    reference.navigate_to(AppScreen::Settings);
    let expected = reference.navigate_back().screen_id;

    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "go_back".to_string(),
    });

    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(
            screen.screen_id, expected,
            "go_back action must land where navigate_back() does"
        ),
        other => panic!("go_back must return NavigateTo at a sub-screen, got {other:?}"),
    }
    assert_eq!(
        engine.current_screen().screen_id,
        expected,
        "the engine must actually be on the popped screen afterwards"
    );
}

/// A bottom-nav tab root is a back-stopper, so it carries no `go_back`
/// nav_action — the affordance appears only where there is a back step.
// @internal
#[test]
fn tab_root_has_no_go_back_nav_action() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Contacts);

    let tab = engine.current_screen();
    assert!(
        !tab.nav_actions.iter().any(|a| a.id == "go_back"),
        "a bottom-nav tab root must not render a Back affordance"
    );
}
