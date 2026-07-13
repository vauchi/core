// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The neutral `Heartbeat` pulse (ADR-044 Am2a) — the requires_poll
//! retirement's core mechanism. The frontend forwards a steady tick and core
//! advances any live relay/exchange session one step and re-renders; the
//! frontend owns no poll cadence or semantics, only the pulse.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// A `Heartbeat` re-renders the current screen (after advancing any live
/// session), so the frontend forwards a neutral tick instead of driving a
/// `requires_poll` loop. With no active session the advance is a no-op and the
/// screen is unchanged — the engine must not navigate.
// @internal
#[test]
fn heartbeat_advances_and_rerenders_current_screen() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Settings);
    let before = engine.current_screen().screen_id;

    let result = engine.handle_action(UserAction::Heartbeat);

    match result {
        ActionResult::UpdateScreen(screen) => assert_eq!(
            screen.screen_id, before,
            "Heartbeat must re-render the current screen"
        ),
        other => panic!("Heartbeat must return UpdateScreen, got {other:?}"),
    }
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::Settings,
        "an idle Heartbeat must not navigate"
    );
}
