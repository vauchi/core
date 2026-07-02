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
use vauchi_core::Event;

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

    let MultiStageEvent::Failed { reason } = event else {
        panic!("a stalled phase past the step budget must emit Failed, got {event:?}");
    };
    // T1.5: the reason renders directly on the failed screen, so it must be
    // a user-readable message, not the old stable id.
    assert!(
        reason.to_lowercase().contains("timed out") && reason != "exchange_timeout",
        "timeout reason must be human-readable, got {reason:?}"
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

/// Drive a machine into a peer-engaged phase by cross-feeding one live
/// peer's frames (the two-party pattern), then return it with the time
/// cursor. Bounded — engagement must happen within a minute of ticks.
fn peer_engaged_machine() -> (MultiStageMachine, u64) {
    let mut alice = MultiStageMachine::new_glance(local_card(), NOW);
    let mut bob = MultiStageMachine::new_glance(b"name:Bob\nemail:bob@example.com".to_vec(), NOW);

    let mut now = NOW;
    loop {
        now += 500;
        let _ = alice.advance(now);
        if let MultiStageEvent::QrFrameReady(p) = bob.advance(now) {
            let _ = alice.handle_hardware_event(&Event::QrScanned { data: p.data }, now);
        }
        if !matches!(
            alice.phase(),
            MultiStagePhase::Preparing | MultiStagePhase::Advertising
        ) {
            return (alice, now);
        }
        assert!(
            now < NOW + 60_000,
            "peer engagement must happen within the drive budget, stuck in {:?}",
            alice.phase()
        );
    }
}

// Discovery-budget differentiation (Phase 1 field feedback 2026-07-02 in
// `2026-06-11-exchange-waits-forever-without-capabilities`): peerless
// `Advertising` is human-paced — two people aligning phones burned the
// flat 120 s budget on-device before any peer contact. Discovery gets a
// longer budget; every peer-engaged phase keeps the 120 s step budget.
// @internal
#[test]
fn peerless_advertising_outlives_the_step_budget() {
    let mut machine = advertising_machine();

    let event = machine.advance(NOW + MULTI_STAGE_STEP_TIMEOUT_MS + 1);

    assert!(
        !matches!(event, MultiStageEvent::Failed { .. }),
        "human-paced discovery must outlive the machine-paced step \
         budget (device regression 2026-07-02), got {event:?}"
    );
    assert!(
        !matches!(machine.phase(), MultiStagePhase::Failed { .. }),
        "Advertising must not fail at the step budget, got {:?}",
        machine.phase()
    );
}

// @internal
#[test]
fn peer_engaged_phase_still_fails_at_step_budget() {
    let (mut machine, now) = peer_engaged_machine();
    let engaged_phase = machine.phase().clone();

    let event = machine.advance(now + MULTI_STAGE_STEP_TIMEOUT_MS + 1);

    let MultiStageEvent::Failed { .. } = event else {
        panic!(
            "a stalled peer-engaged phase ({engaged_phase:?}) must keep \
             the 120s step budget, got {event:?}"
        );
    };
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
