// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! App lifecycle events as core-decided consequences (ADR-021 + ADR-044 Am2a
//! Family-A). The frontend forwards the raw OS foreground event and core owns
//! the on-resume consequence (relay catch-up + re-render), retiring the
//! frontend's `ON_RESUME -> sync()` / `becameActive -> re-fetch` decision.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The OS foreground event re-renders the current screen — covering state that
/// changed while backgrounded — so the frontend never decides to re-fetch or
/// sync on resume; it forwards the event and core owns the consequence.
// @internal
#[test]
fn app_foregrounded_rerenders_current_screen() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    let before = engine.current_screen().screen_id;

    let result = engine.handle_action(UserAction::AppForegrounded);

    match result {
        ActionResult::UpdateScreen(screen) => assert_eq!(
            screen.screen_id, before,
            "AppForegrounded must re-render the current screen"
        ),
        other => panic!("AppForegrounded must return UpdateScreen, got {other:?}"),
    }
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::Settings,
        "the foreground refresh must not navigate away"
    );
}
