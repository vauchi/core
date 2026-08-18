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
