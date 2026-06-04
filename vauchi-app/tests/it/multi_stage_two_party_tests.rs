// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Two-party end-to-end test of the `MultiStageMachine` wrapper — the
//! exact poll-driven path the mobile frontends use (advance() to emit a
//! frame, handle_hardware_event(QrScanned) to ingest the peer's frame).
//!
//! `vauchi-core`'s `multistage_e2e_tests::run_full_exchange` already
//! proves the **raw** `MultiStageSession` reaches `Finalized`. This was
//! the missing coverage: the device drives the *wrapper* (cycle thread
//! retired in slice-32m), and on-device the Glance exchange ran the full
//! pipeline but stalled at the final "Almost done" (`Complete`) without
//! creating a contact — "worked with legacy [cycle-thread] code"
//! (2026-06-03 device session, Pixel 3a + Samsung S7).
//! See `2026-06-03-multistage-qr-exchange-stalls-init-on-device`.

use vauchi_app::orchestrator::multi_stage_machine::{
    MultiStageEvent, MultiStageMachine, MultiStagePhase,
};
use vauchi_core::Event;

/// Drive two Glance machines against each other through the wrapper API
/// and return their final phases. Each "tick" advances a wall-clock-ms
/// `now` past the longest frame window so every `advance` emits the next
/// frame, then cross-feeds each emitted frame to the peer as a
/// `QrScanned` hardware event — mirroring the device's poll + camera
/// loop. Breaks as soon as both reach a terminal phase.
fn drive_two_party_glance(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
    max_ticks: usize,
) -> (MultiStagePhase, MultiStagePhase, usize) {
    let mut alice = MultiStageMachine::new_glance(alice_card, 0);
    let mut bob = MultiStageMachine::new_glance(bob_card, 0);

    let mut now: u64 = 0;
    let mut alice_frame: Option<String> = None;
    let mut bob_frame: Option<String> = None;

    for tick in 0..max_ticks {
        // 500 ms/tick > the longest jittered frame window (~440 ms) so
        // each advance emits a fresh frame, cycling INIT → DATA → … →
        // COMBO via the session's display_cycle.
        now += 500;

        if let MultiStageEvent::QrFrameReady(p) = alice.advance(now) {
            alice_frame = Some(p.data);
        }
        if let MultiStageEvent::QrFrameReady(p) = bob.advance(now) {
            bob_frame = Some(p.data);
        }

        // Cross-feed: each side scans the other's most recent frame.
        if let Some(data) = bob_frame.clone() {
            let _ = alice.handle_hardware_event(&Event::QrScanned { data }, now);
        }
        if let Some(data) = alice_frame.clone() {
            let _ = bob.handle_hardware_event(&Event::QrScanned { data }, now);
        }

        if alice.is_terminal() && bob.is_terminal() {
            return (alice.phase(), bob.phase(), tick + 1);
        }
    }
    (alice.phase(), bob.phase(), max_ticks)
}

// @internal
#[test]
fn two_party_glance_reaches_completed_through_the_wrapper() {
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();

    let (alice_phase, bob_phase, ticks) = drive_two_party_glance(alice_card, bob_card, 4000);

    // Success is `Finalized` — the contact-creating transition. (The
    // later `Finalized → Completed` hop fires only after a wall-clock
    // grace period this fast in-memory loop never advances, so we do not
    // require it here.) Reaching `Finalized` through the wrapper proves
    // the COMBO/RDYY finalization the device stalled at is sound with
    // clean frame delivery — i.e. the on-device "Almost done" stall is a
    // delivery-rate issue, not a core-logic bug.
    assert!(
        matches!(alice_phase, MultiStagePhase::Finalized { .. }),
        "Alice must finalize through the wrapper; stuck at {alice_phase:?} after {ticks} ticks",
    );
    assert!(
        matches!(bob_phase, MultiStagePhase::Finalized { .. }),
        "Bob must finalize through the wrapper; stuck at {bob_phase:?} after {ticks} ticks",
    );
}

/// Like [`drive_two_party_glance`] but Bob's camera is `slow_factor`×
/// lossier — Bob only ingests every `slow_factor`-th frame Alice shows,
/// while Alice ingests every frame Bob shows. This models the device
/// asymmetry (Pixel races ahead, Samsung S7 lags) that drives the two
/// sessions to very different stages — exercising `handle_combo`'s
/// "still Transferring → stash the reveal key" path that the lockstep
/// test never hits.
fn drive_asymmetric_glance(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
    slow_factor: usize,
    max_ticks: usize,
) -> (MultiStagePhase, MultiStagePhase, usize) {
    let mut alice = MultiStageMachine::new_glance(alice_card, 0);
    let mut bob = MultiStageMachine::new_glance(bob_card, 0);

    let mut now: u64 = 0;
    let mut alice_frame: Option<String> = None;
    let mut bob_frame: Option<String> = None;

    for tick in 0..max_ticks {
        now += 500;
        if let MultiStageEvent::QrFrameReady(p) = alice.advance(now) {
            alice_frame = Some(p.data);
        }
        if let MultiStageEvent::QrFrameReady(p) = bob.advance(now) {
            bob_frame = Some(p.data);
        }
        // Alice (fast camera) ingests every Bob frame.
        if let Some(data) = bob_frame.clone() {
            let _ = alice.handle_hardware_event(&Event::QrScanned { data }, now);
        }
        // Bob (slow camera) ingests only every slow_factor-th Alice frame.
        if tick % slow_factor == 0
            && let Some(data) = alice_frame.clone()
        {
            let _ = bob.handle_hardware_event(&Event::QrScanned { data }, now);
        }
        if alice.is_terminal() && bob.is_terminal() {
            return (alice.phase(), bob.phase(), tick + 1);
        }
    }
    (alice.phase(), bob.phase(), max_ticks)
}

// @internal
#[test]
fn asymmetric_camera_glance_still_finalizes_both_sides() {
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();

    // Bob 5× lossier — Pixel-vs-S7-class asymmetry.
    let (alice_phase, bob_phase, ticks) = drive_asymmetric_glance(alice_card, bob_card, 5, 8000);

    assert!(
        matches!(alice_phase, MultiStagePhase::Finalized { .. }),
        "Alice (fast) must finalize despite the slow peer; stuck at {alice_phase:?} after {ticks} ticks",
    );
    assert!(
        matches!(bob_phase, MultiStagePhase::Finalized { .. }),
        "Bob (slow) must finalize; stuck at {bob_phase:?} after {ticks} ticks — \
         the device 'Almost done' stall (COMBO decoded but never finalizes)",
    );
}
