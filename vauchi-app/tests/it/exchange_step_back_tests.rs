// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-internal BACK for the exchange flow.
//!
//! Regression for the 2026-06-01 device "Exchange Mode" back-trap: the
//! exchange sub-screens are internal `ExchangeStep` states under the
//! single `AppScreen::Exchange`, so a BACK press must rewind one step
//! (`WorkflowEngine::navigate_back_within`) instead of tearing down the
//! whole Exchange screen. See `ui/exchange/back_nav.rs`.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn back_from_exchange_subflow_rewinds_to_mode_selection() {
    let mut engine = engine_with_identity();

    // Entering Exchange lands on the mode-selection root step, stamped with
    // the canonical tab-root id so the bottom nav bar renders.
    let screen = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(
        screen.screen_id, "exchange",
        "Exchange entry should show the mode picker under the canonical id"
    );
    // The root step offers no engine-internal back yet (BACK here exits
    // the Exchange screen via the AppScreen path, not handled by this hook).
    assert!(
        !engine.can_go_back(),
        "no in-engine back at the mode-selection root"
    );

    // Pick a QR-family mode (TapHoverShake → Qr::ShowQr sub-flow entry). This
    // records ModeSelection on the engine's step history.
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:fun".into(),
        item_id: "mode:tap_hover_shake".into(),
    });
    assert!(
        engine.can_go_back(),
        "a sub-flow entry step must offer engine-internal BACK"
    );
    // Narrowness guard: only the mode-selection ROOT is stamped `exchange`.
    // A sub-flow state under the same `AppScreen::Exchange` must keep its
    // distinct engine id so the nav bar hides mid-flow and native wrappers
    // still dispatch — a blanket Exchange stamp would regress this to
    // `exchange`.
    assert_ne!(
        engine.current_screen().screen_id,
        "exchange",
        "an exchange sub-flow state must NOT carry the canonical root id"
    );

    // BACK rewinds the step rather than leaving Exchange.
    let back = engine.navigate_back();
    assert_eq!(
        back.screen_id, "exchange",
        "BACK must rewind to the mode picker (canonical id), not exit Exchange"
    );
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::Exchange,
        "BACK should stay on the Exchange AppScreen"
    );
    // And mode selection is interactive again (engine was not torn down).
    assert!(
        !engine.can_go_back(),
        "after rewinding to the root step, in-engine back is exhausted"
    );
}
