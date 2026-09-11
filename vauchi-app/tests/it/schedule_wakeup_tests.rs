// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Option C wakeup scheduling (ADR-044 Am2a): core owns *when* the app
//! heartbeat is due — `on_wakeup` runs due work and emits the next
//! `Command::ScheduleWakeup`; the humble shell owns only the platform wakeup
//! mechanism (desktop interval / iOS BGAppRefreshTask / Android WorkManager).

use vauchi_app::ui::AppEngine;
use vauchi_core::Command;
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// `on_wakeup` runs due work and emits exactly one `ScheduleWakeup` with a sane
/// window, so the shell can re-arm. The frontend bootstraps the loop by calling
/// it once at launch.
// @internal
#[test]
fn on_wakeup_emits_a_schedule_wakeup_command() {
    let mut engine = engine_with_identity();
    let _notifications = engine.on_wakeup();

    let cmds = engine.drain_pending_commands();
    let scheduled: Vec<_> = cmds
        .iter()
        .filter(|c| matches!(c, Command::ScheduleWakeup { .. }))
        .collect();
    assert_eq!(
        scheduled.len(),
        1,
        "on_wakeup must emit exactly one ScheduleWakeup, got {cmds:?}"
    );
    if let Command::ScheduleWakeup {
        earliest_secs,
        deadline_secs,
        min_interval_secs,
        ..
    } = scheduled[0]
    {
        assert!(
            earliest_secs <= deadline_secs,
            "earliest ({earliest_secs}) must not exceed deadline ({deadline_secs})"
        );
        assert!(*min_interval_secs > 0, "min_interval must be positive");
    }
}

/// Idempotent: a second wake emits its own single reschedule (no accumulation
/// beyond one-per-call), so delayed / coalesced / repeated wakes stay safe.
// @internal
#[test]
fn on_wakeup_reschedules_once_per_call() {
    let mut engine = engine_with_identity();
    engine.on_wakeup();
    let _ = engine.drain_pending_commands();

    engine.on_wakeup();
    let cmds = engine.drain_pending_commands();
    let count = cmds
        .iter()
        .filter(|c| matches!(c, Command::ScheduleWakeup { .. }))
        .count();
    assert_eq!(count, 1, "each on_wakeup emits exactly one reschedule");
}

/// A live Hover/Glance exchange is driven entirely by this heartbeat:
/// `advance_multi_stage_session` runs inside `poll_notifications`, which only
/// runs on wakeup. At the idle 30 s cadence the QR advances once every 30 s
/// while the protocol is built for a ~300 ms frame — device-observed as one
/// `[MSX] tx` against 110 camera decodes, 40 peer INITs dropped before our own
/// QR existed, and 43 s from "Exchange started" to any state change
/// (2026-08-19 Hover run).
// @internal
#[test]
fn an_active_exchange_schedules_a_far_shorter_wakeup_than_the_idle_heartbeat() {
    let mut idle = engine_with_identity();
    let _ = idle.on_wakeup();
    let idle_secs = first_wakeup_earliest_secs(&mut idle);

    let mut exchanging = engine_with_identity();
    exchanging.ensure_multi_stage_session(vauchi_core::exchange::mode::ExchangeMode::Hover);
    assert!(
        exchanging.multi_stage_session_active(),
        "precondition: a multi-stage session is live"
    );
    let _ = exchanging.on_wakeup();
    let active_secs = first_wakeup_earliest_secs(&mut exchanging);

    assert_eq!(idle_secs, 30, "the idle heartbeat is unchanged");
    assert!(
        active_secs <= 1,
        "a live exchange must be driven at least once a second, got {active_secs}s \
         (the QR frame it advances is designed to show for ~300ms)"
    );

    // Whole seconds cannot express the frame dwell, so the sub-second field is
    // what actually fixes the cadence; a shell reading only `earliest_secs`
    // still gets a working, if coarser, exchange.
    let idle_millis = first_wakeup_earliest_millis(&mut engine_with_identity());
    assert_eq!(
        idle_millis, None,
        "the idle heartbeat needs no sub-second precision"
    );

    let mut exchanging2 = engine_with_identity();
    exchanging2.ensure_multi_stage_session(vauchi_core::exchange::mode::ExchangeMode::Hover);
    let _ = exchanging2.on_wakeup();
    let active_millis =
        first_wakeup_earliest_millis_from(&mut exchanging2).expect("a live exchange names its ms");
    assert!(
        (100..=500).contains(&active_millis),
        "a live exchange must be driven at its frame dwell (~300ms), got {active_millis}ms"
    );
}

fn first_wakeup_earliest_millis(engine: &mut AppEngine) -> Option<u32> {
    let _ = engine.on_wakeup();
    first_wakeup_earliest_millis_from(engine)
}

fn first_wakeup_earliest_millis_from(engine: &mut AppEngine) -> Option<u32> {
    engine
        .drain_pending_commands()
        .into_iter()
        .find_map(|c| match c {
            Command::ScheduleWakeup {
                earliest_millis, ..
            } => Some(earliest_millis),
            _ => None,
        })
        .expect("a ScheduleWakeup is emitted")
}

fn first_wakeup_earliest_secs(engine: &mut AppEngine) -> u32 {
    engine
        .drain_pending_commands()
        .into_iter()
        .find_map(|c| match c {
            Command::ScheduleWakeup { earliest_secs, .. } => Some(earliest_secs),
            _ => None,
        })
        .expect("a ScheduleWakeup is emitted")
}

/// Opening the exchange screen must produce our QR immediately, not on the
/// next heartbeat. Until it exists we are still `Idle`, and `handle_init`
/// accepts a peer INIT only while `Advertising` — so every peer frame decoded
/// in the meantime is discarded. Device-measured at 29.1 s from
/// "Exchange started" to the first QR, during which 40 peer INITs were thrown
/// away (2026-08-19 Hover run).
// @internal
#[test]
fn starting_an_exchange_builds_our_qr_without_waiting_for_a_heartbeat() {
    let mut engine = engine_with_identity();
    engine.ensure_multi_stage_session(vauchi_core::exchange::mode::ExchangeMode::Hover);

    assert_eq!(
        engine.multi_stage_phase(),
        Some(vauchi_app::orchestrator::multi_stage_machine::MultiStagePhase::Advertising),
        "a session must be advertising as soon as it is created — anything \
         earlier means peer INITs are being dropped"
    );
}

/// The sub-second interval has to survive serialisation to reach a shell.
/// On device the Pixel advanced its display every 2–4 seconds while core was
/// asking for ~300 ms, and the field being absent or renamed on the wire is
/// the first thing that could explain it
/// (2026-08-19, `2026-08-18-hover-transfer-stalls-on-the-last-chunk`).
// @internal
#[test]
fn the_sub_second_wakeup_survives_json() {
    let mut engine = engine_with_identity();
    engine.ensure_multi_stage_session(vauchi_core::exchange::mode::ExchangeMode::Hover);
    let _ = engine.on_wakeup();
    let scheduled = engine
        .drain_pending_commands()
        .into_iter()
        .find(|c| matches!(c, Command::ScheduleWakeup { .. }))
        .expect("a ScheduleWakeup is emitted");

    let json = serde_json::to_string(&scheduled).expect("serialises");
    assert!(
        json.contains("earliest_millis"),
        "the shell keys on `earliest_millis`; got {json}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parses");
    let millis = parsed
        .pointer("/ScheduleWakeup/earliest_millis")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("no sub-second interval in {json}"));
    assert!(
        (100..=500).contains(&millis),
        "a live exchange must ask for its frame dwell, got {millis}ms"
    );
}

/// A live Link session is a relay rendezvous of several dependent round
/// trips (presence deposit → peer epk → card deposit → peer card), and each
/// leg waits for a heartbeat on one side or the other. At the idle 30 s
/// cadence two terminals needed ~90 s to converge — past every 60 s wait in
/// `integration_tui_to_tui_link_exchange` — while the escrow phase is
/// designed to poll about once a second.
// @internal
#[test]
fn a_live_link_session_schedules_a_far_shorter_wakeup_than_the_idle_heartbeat() {
    let mut initiating = engine_with_identity();
    let entry = initiating.navigate_to(vauchi_app::ui::AppScreen::LinkExchange);
    assert_eq!(
        entry.screen_id, "exchange_share_url",
        "precondition: on the Link share screen"
    );
    assert!(
        initiating.link_session_active(),
        "precondition: the initiator machine is live"
    );
    let _ = initiating.on_wakeup();
    let initiator_secs = first_wakeup_earliest_secs(&mut initiating);
    assert!(
        initiator_secs <= 1,
        "a live Link initiator must be driven at least once a second, got {initiator_secs}s"
    );

    let (initiation, _presence) = vauchi_core::exchange::link_mode::initiator_generate();
    let payload = vauchi_core::exchange::link_mode::parse_exchange_deep_link(&initiation.url)
        .expect("a generated link parses");
    let mut responding = engine_with_identity();
    responding.navigate_to(vauchi_app::ui::AppScreen::DeepLinkResponder { payload });
    assert!(
        responding.link_session_active(),
        "precondition: the responder machine is live"
    );
    let _ = responding.on_wakeup();
    let responder_secs = first_wakeup_earliest_secs(&mut responding);
    assert!(
        responder_secs <= 1,
        "a live Link responder must be driven at least once a second, got {responder_secs}s"
    );

    let mut idle = engine_with_identity();
    let _ = idle.on_wakeup();
    assert_eq!(
        first_wakeup_earliest_secs(&mut idle),
        30,
        "the idle heartbeat is unchanged"
    );
}

// `FakeClock` is `#[cfg(any(test, feature = "testing"))]`; the no-feature
// compile check excludes the clock-driven tests below with it.
#[cfg(feature = "testing")]
mod link_heartbeat {
    use super::*;
    use vauchi_app::ui::WorkflowEngine;

    fn engine_on_link_share_screen_with_clock()
    -> (std::sync::Arc<vauchi_core::clock::FakeClock>, AppEngine) {
        let clock = std::sync::Arc::new(vauchi_core::clock::FakeClock::new(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        ));
        let mut vauchi = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
        vauchi.create_identity("Alice").unwrap();
        let mut engine = AppEngine::new(vauchi);
        let entry = engine.navigate_to(vauchi_app::ui::AppScreen::LinkExchange);
        assert_eq!(
            entry.screen_id, "exchange_share_url",
            "precondition: on the Link share screen"
        );
        (clock, engine)
    }

    /// The polling deadline is the one Link transition no relay event
    /// announces: `tick` fails the machine, but nothing rendered that, so a
    /// share screen whose session had silently died kept inviting the peer.
    // @internal
    #[test]
    fn a_link_session_past_its_deadline_shows_the_failure_on_the_next_heartbeat() {
        let (clock, mut engine) = engine_on_link_share_screen_with_clock();
        let before = engine.current_screen();

        clock.advance(std::time::Duration::from_secs(301));
        let _ = engine.on_wakeup();

        assert!(
            !engine.link_session_active(),
            "the session is over once its deadline passed"
        );
        assert_ne!(
            before,
            engine.current_screen(),
            "the heartbeat must render the timed-out session"
        );
    }

    /// A heartbeat can replace what is on screen — a Link responder's waiting
    /// screen becomes the completion summary when the relay hands over the
    /// peer's card; a timed-out session becomes its failure. Shells render only
    /// what core hands them, so a wakeup that changed the screen must
    /// re-present it; otherwise the terminal keeps showing "Waiting..." after
    /// the contact was saved (`integration_tui_to_tui_link_exchange`,
    /// 2026-09-11).
    // @internal
    #[test]
    fn a_wakeup_that_changes_the_screen_re_presents_it() {
        let (clock, mut engine) = engine_on_link_share_screen_with_clock();
        let _ = engine.on_wakeup();
        let _ = engine.drain_pending_commands();

        let before = engine.current_screen();
        clock.advance(std::time::Duration::from_secs(301));
        let _ = engine.on_wakeup();
        assert_ne!(
            before,
            engine.current_screen(),
            "precondition: the heartbeat changed the screen"
        );

        let cmds = engine.drain_pending_commands();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, Command::ReplaceSurface { .. })),
            "a wakeup that changed the screen must re-present it, got {cmds:?}"
        );
    }
}

/// The converse: an idle heartbeat that changed nothing must not flood the
/// shell with a surface replacement every tick.
// @internal
#[test]
fn an_idle_wakeup_does_not_re_present_the_screen() {
    let mut engine = engine_with_identity();
    let _ = engine.on_wakeup();
    let _ = engine.drain_pending_commands();
    let _ = engine.on_wakeup();
    let cmds = engine.drain_pending_commands();
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, Command::ReplaceSurface { .. })),
        "an idle heartbeat must not re-present, got {cmds:?}"
    );
}
