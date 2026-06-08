// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level regression: the multi-stage exchange poll must
//! advance display frames on a **millisecond** cadence.
//!
//! Device bug 2026-06-03 (`2026-06-03-multistage-qr-exchange-stalls-init-on-device`,
//! reproduced Pixel 3a + Samsung S7): `advance_multi_stage_session`
//! drove `MultiStageMachine::advance(now)` with `clock.unix_seconds()`,
//! but the machine's per-frame gate compares `now` against
//! `display_duration_ms` (e.g. INIT = 400 **ms**). With `now` in
//! seconds the gate `now - started < 400` held the frame for ~400
//! *seconds*, freezing `own_qr` on the INIT frame so the bilateral QR
//! exchange never progressed past stage 1 — even though every scan was
//! decoded and delivered. The existing wrapper proptests passed because
//! they drive `now` in milliseconds (`tick_ms`), matching the gate's
//! unit; only the production caller used the wrong unit.
//!
//! This pins the caller's cadence: after a peer INIT scan moves us to
//! `Discovered`, a poll one frame-window later (in wall-clock) must emit
//! the next frame (a DATA chunk), so `own_qr` changes off the INIT
//! payload.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::exchange::MultiStageSession;

/// Extract the `own_qr` display component's payload from the engine's
/// current screen, if present.
fn own_qr_data(engine: &AppEngine) -> Option<String> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::QrCode { id, data, .. } if id == "own_qr" => Some(data.clone()),
            _ => None,
        })
}

// @internal
#[test]
fn multi_stage_poll_advances_frame_after_peer_scan_and_frame_window() {
    // FakeClock anchored at a real epoch so `unix_seconds` / `unix_millis`
    // both return sane values; the test only relies on relative advances.
    let fake = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let clock: Arc<dyn Clock> = fake.clone();
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);

    // Enter the exchange flow and pick Glance → core hands off to the
    // multi-stage screen (no groups → direct handoff).
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::MultiStageExchange { .. }
        ),
        "picking Hover should navigate to MultiStageExchange, got {:?}",
        engine.current_app_screen()
    );

    // First poll emits our INIT frame (Idle → Advertising).
    engine.poll_notifications();
    let init_qr = own_qr_data(&engine).expect("own_qr INIT frame present after first poll");

    // A peer (Bob) advertises; we scan his INIT. This moves our session
    // Advertising → Discovered, so the next display frame is a DATA chunk
    // (a different payload than the INIT frame).
    let mut bob = MultiStageSession::new(b"name:Bob\nemail:bob@example.com".to_vec());
    let bob_init = bob.get_display_qr().expect("bob INIT frame").data;
    let scan_event =
        engine.forward_multi_stage_hardware_event(&Event::QrScanned { data: bob_init });
    engine.apply_multi_stage_event(scan_event);

    // Advance wall-clock by 2 s — well past the ~400 ms INIT frame window
    // (even with jitter) — and poll again. The next frame must emit, so
    // `own_qr` advances off the INIT payload.
    //
    // Regression guard: with the seconds/millis unit bug the poll gate
    // (400 ms treated as 400 s) keeps the frame frozen and `own_qr`
    // stays the INIT payload, deadlocking the exchange.
    fake.advance(Duration::from_secs(2));
    engine.poll_notifications();
    let next_qr = own_qr_data(&engine).expect("own_qr present after the frame window");

    assert_ne!(
        next_qr, init_qr,
        "multi-stage display frame must advance past INIT after a peer scan + frame window; \
         a frozen frame means the poll cadence regressed to a seconds/millis unit mismatch"
    );
}
