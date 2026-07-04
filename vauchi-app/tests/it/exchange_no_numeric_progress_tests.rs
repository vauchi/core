// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M2 S2 — no numeric progress anywhere in the exchange flow (design D2.2,
//! `2026-07-03-one-tap-exchange` goal 2): a handshake is not a wizard. The
//! old numbering was incoherent (group=2, picker=none, preview=3, verifying
//! hardcoded 6, failed 8/8) and could regress or skip; the fix is removal,
//! not recomputation.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::mode::ExchangeMode;

fn engine_with_group() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let work = vauchi.create_group("Work").expect("create group");
    let work_id = work.id().to_string();
    (AppEngine::new(vauchi), work_id)
}

fn assert_no_progress(engine: &AppEngine, context: &str) {
    let screen = engine.current_screen();
    assert!(
        screen.progress.is_none(),
        "{context} (screen `{}`) must not render numeric progress, got {:?}",
        screen.screen_id,
        screen.progress
    );
}

// The legacy flow's own steps: group selection → picker → field preview.
// @scenario: exchange :: no exchange screen renders a step number
// @internal
#[test]
fn legacy_flow_steps_render_no_progress() {
    let (mut engine, work_id) = engine_with_group();
    engine.navigate_to(AppScreen::Exchange);
    assert_no_progress(&engine, "group selection");

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: work_id,
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert_no_progress(&engine, "mode picker");

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    assert_no_progress(&engine, "field preview");
}

// Every dedicated exchange engine's entry screen: BLE, NFC, Cable, Link,
// multi-stage.
// @scenario: exchange :: dedicated mode screens render no step numbers
// @internal
#[test]
fn dedicated_mode_screens_render_no_progress() {
    let (mut engine, _work) = engine_with_group();
    for (screen, context) in [
        (
            AppScreen::BleExchange {
                mode: ExchangeMode::Bump,
            },
            "BLE exchange",
        ),
        (AppScreen::NfcExchange, "NFC exchange"),
        (AppScreen::DirectTransport, "Cable exchange"),
        (AppScreen::LinkExchange, "Link exchange"),
        (
            AppScreen::MultiStageExchange {
                mode: ExchangeMode::Hover,
            },
            "multi-stage exchange",
        ),
    ] {
        engine.navigate_to(screen);
        assert_no_progress(&engine, context);
    }
}
