// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-step stall deadline for the multi-stage exchange machine
//! (Phase 1 of `2026-06-11-exchange-waits-forever-without-capabilities`).
//!
//! A non-terminal phase that makes no forward progress within
//! [`MULTI_STAGE_STEP_TIMEOUT_MS`] must fail to a retry/cancel screen
//! instead of waiting forever — the device-verified infinite
//! "Searching…" when no peer ever arrives. The deadline resets on every
//! phase transition, so a healthy exchange (steady progress) never trips
//! it; the two-party / proptest suites pin that no-regression property.
//!
//! CC-06: the machine takes `now` as an explicit `u64` (milliseconds),
//! so these tests drive time directly — no clock, no sleeps.

use vauchi_app::orchestrator::multi_stage_machine::{
    MULTI_STAGE_STEP_TIMEOUT_MS, MultiStageEvent, MultiStageMachine, MultiStagePhase,
};

/// Unix-millis base, mirroring the poll-cadence test anchor. A non-zero
/// base catches any `now - phase_entered` math that assumes `now > 0`.
const NOW: u64 = 1_700_000_000_000;

fn local_card() -> Vec<u8> {
    b"name:Alice\nemail:alice@example.com".to_vec()
}

/// Drive the machine to a live, peerless `Advertising` phase.
fn advertising_machine() -> MultiStageMachine {
    let mut machine = MultiStageMachine::new_glance(local_card(), NOW);
    let _ = machine.advance(NOW);
    assert!(
        matches!(machine.phase(), MultiStagePhase::Advertising),
        "first advance must enter Advertising, got {:?}",
        machine.phase()
    );
    machine
}

// @internal
#[test]
fn stalled_phase_past_step_budget_emits_failed() {
    let mut machine = advertising_machine();

    let event = machine.advance(NOW + MULTI_STAGE_STEP_TIMEOUT_MS + 1);

    assert!(
        matches!(event, MultiStageEvent::Failed { .. }),
        "a stalled phase past the step budget must emit Failed, got {event:?}"
    );
    assert!(
        matches!(machine.phase(), MultiStagePhase::Failed { .. }),
        "phase must be Failed after the deadline, got {:?}",
        machine.phase()
    );
}

// @internal
#[test]
fn phase_just_before_step_budget_does_not_fail() {
    let mut machine = advertising_machine();

    let event = machine.advance(NOW + MULTI_STAGE_STEP_TIMEOUT_MS - 1);

    assert!(
        !matches!(event, MultiStageEvent::Failed { .. }),
        "must not fail before the step budget elapses, got {event:?}"
    );
    assert!(
        !matches!(machine.phase(), MultiStagePhase::Failed { .. }),
        "phase must stay non-Failed before the budget, got {:?}",
        machine.phase()
    );
}

// @internal
#[test]
fn timed_out_machine_is_terminal_and_absorbing() {
    let mut machine = advertising_machine();
    let _ = machine.advance(NOW + MULTI_STAGE_STEP_TIMEOUT_MS + 1);
    assert!(
        matches!(machine.phase(), MultiStagePhase::Failed { .. }),
        "precondition: machine must have timed out"
    );

    // A later advance must not resurrect the machine out of Failed.
    let later = machine.advance(NOW + 2 * MULTI_STAGE_STEP_TIMEOUT_MS);

    assert!(
        matches!(later, MultiStageEvent::None),
        "advance after a terminal timeout must be inert, got {later:?}"
    );
    assert!(
        matches!(machine.phase(), MultiStagePhase::Failed { .. }),
        "Failed is absorbing, got {:?}",
        machine.phase()
    );
}
