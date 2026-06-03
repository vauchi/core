// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Back navigation as a typed `UserAction` + `can_go_back` as `ScreenModel`
//! data (allowlist-reduction #2, core half).
//!
//! Symmetric with `navigate_to_tab_tests.rs`: the OS back gesture forwards
//! `UserAction::NavigateBack` through `handle_action` instead of the binding
//! calling `navigate_back_json`, and the back *affordance* state rides on the
//! rendered `ScreenModel.can_go_back` instead of a separate `can_go_back()`
//! query. Together these let the `navigate_back_json` + `can_go_back` binding
//! surface retire once both frontends migrate (ADR-043 Amendment 4).

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

/// `ScreenModel.can_go_back` mirrors `AppEngine::can_go_back()` on every
/// rendered screen — so the frontend reads its back affordance off the screen
/// it already has, no separate query.
// @internal
#[test]
fn screen_model_can_go_back_mirrors_engine_for_root_and_sub_screen() {
    let mut engine = engine_with_identity();

    // A bottom-nav tab root is a back-stopper even with history behind it.
    engine.navigate_to(AppScreen::Contacts);
    let tab = engine.current_screen();
    assert_eq!(
        tab.can_go_back,
        engine.can_go_back(),
        "ScreenModel.can_go_back must mirror engine.can_go_back() at a tab root"
    );
    assert!(
        !tab.can_go_back,
        "a bottom-nav tab root offers no back even with nav history"
    );

    // A non-root sub-screen with history offers back.
    engine.navigate_to(AppScreen::Settings);
    let sub = engine.current_screen();
    assert_eq!(
        sub.can_go_back,
        engine.can_go_back(),
        "ScreenModel.can_go_back must mirror engine.can_go_back() at a sub-screen"
    );
    assert!(
        sub.can_go_back,
        "a non-root sub-screen with history must offer back"
    );
}
