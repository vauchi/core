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
//!
//! `..._builds_matching_ratchet_pair` extend this past *finalization* to
//! the **ratchet** the device's `persist_exchanged_contact` builds at
//! completion (`build_exchange_ratchet`). Reaching `Finalized` is not
//! enough: if the wrapper finalizes but `build_exchange_ratchet` returns
//! `None`, the AppEngine saves the contact WITHOUT a ratchet, so the
//! mailbox token still resolves (`unresolved=0`) but every later card
//! update fails to decrypt (`rejected=N`) — the exact on-device symptom
//! measured 2026-06-28 (`2026-06-28-sync-delivery-sent-not-received`).
//! The asymmetric variant models the lossy-camera reality the lockstep
//! and raw perfect-delivery tests never exercise.

use vauchi_app::orchestrator::multi_stage_machine::{
    MultiStageEvent, MultiStageMachine, MultiStagePhase,
};
use vauchi_core::Event;

/// Drive two Glance machines against each other through the wrapper API
/// and return the finalized machines. Each "tick" advances a wall-clock-ms
/// `now` past the longest frame window so every `advance` emits the next
/// frame, then cross-feeds each emitted frame to the peer as a
/// `QrScanned` hardware event — mirroring the device's poll + camera
/// loop. Breaks as soon as both reach a terminal phase.
fn drive_two_party_glance(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
    max_ticks: usize,
) -> (MultiStageMachine, MultiStageMachine, usize) {
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
            return (alice, bob, tick + 1);
        }
    }
    (alice, bob, max_ticks)
}

// @internal
#[test]
fn two_party_glance_reaches_completed_through_the_wrapper() {
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();

    let (alice, bob, ticks) = drive_two_party_glance(alice_card, bob_card, 4000);

    // Success is `Finalized` — the contact-creating transition. (The
    // later `Finalized → Completed` hop fires only after a wall-clock
    // grace period this fast in-memory loop never advances, so we do not
    // require it here.) Reaching `Finalized` through the wrapper proves
    // the COMBO/RDYY finalization the device stalled at is sound with
    // clean frame delivery — i.e. the on-device "Almost done" stall is a
    // delivery-rate issue, not a core-logic bug.
    assert!(
        matches!(alice.phase(), MultiStagePhase::Finalized { .. }),
        "Alice must finalize through the wrapper; stuck at {:?} after {ticks} ticks",
        alice.phase(),
    );
    assert!(
        matches!(bob.phase(), MultiStagePhase::Finalized { .. }),
        "Bob must finalize through the wrapper; stuck at {:?} after {ticks} ticks",
        bob.phase(),
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
) -> (MultiStageMachine, MultiStageMachine, usize) {
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
            return (alice, bob, tick + 1);
        }
    }
    (alice, bob, max_ticks)
}

// @internal
#[test]
fn asymmetric_camera_glance_still_finalizes_both_sides() {
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();

    // Bob 5× lossier — Pixel-vs-S7-class asymmetry.
    let (alice, bob, ticks) = drive_asymmetric_glance(alice_card, bob_card, 5, 8000);

    assert!(
        matches!(alice.phase(), MultiStagePhase::Finalized { .. }),
        "Alice (fast) must finalize despite the slow peer; stuck at {:?} after {ticks} ticks",
        alice.phase(),
    );
    assert!(
        matches!(bob.phase(), MultiStagePhase::Finalized { .. }),
        "Bob (slow) must finalize; stuck at {:?} after {ticks} ticks — \
         the device 'Almost done' stall (COMBO decoded but never finalizes)",
        bob.phase(),
    );
}

// @internal
#[test]
fn both_machines_emit_the_finalized_event_that_triggers_persist() {
    // `persist_exchanged_contact` fires ONLY on `MultiStageEvent::Finalized`
    // (AppEngine::apply_multi_stage_event). A machine that reaches the
    // Finalized *phase* without ever emitting the Finalized *event* shows
    // "Exchange Complete" but never saves the contact — the device-proven
    // half-exchange (2026-07-25 Samsung S7: finalized on-screen, empty
    // contact list; Pixel persisted fine). Both sides must emit it exactly
    // once. Lossy (asymmetric) delivery is used because that is when the two
    // sides reach Finalized via different paths (scan vs stashed-key advance).
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();
    let mut alice = MultiStageMachine::new_glance(alice_card, 0);
    let mut bob = MultiStageMachine::new_glance(bob_card, 0);

    let mut now: u64 = 0;
    let mut alice_frame: Option<String> = None;
    let mut bob_frame: Option<String> = None;
    let mut alice_final = 0usize;
    let mut bob_final = 0usize;
    let is_final = |e: &MultiStageEvent| matches!(e, MultiStageEvent::Finalized { .. });
    let slow_factor = 5; // Bob (S7-class) ingests every 5th Alice frame.

    for tick in 0..8000 {
        now += 500;
        let ae = alice.advance(now);
        if is_final(&ae) {
            alice_final += 1;
        }
        if let MultiStageEvent::QrFrameReady(p) = &ae {
            alice_frame = Some(p.data.clone());
        }
        let be = bob.advance(now);
        if is_final(&be) {
            bob_final += 1;
        }
        if let MultiStageEvent::QrFrameReady(p) = &be {
            bob_frame = Some(p.data.clone());
        }
        if let Some(data) = bob_frame.clone() {
            let e = alice.handle_hardware_event(&Event::QrScanned { data }, now);
            if is_final(&e) {
                alice_final += 1;
            }
        }
        if tick % slow_factor == 0
            && let Some(data) = alice_frame.clone()
        {
            let e = bob.handle_hardware_event(&Event::QrScanned { data }, now);
            if is_final(&e) {
                bob_final += 1;
            }
        }
        if alice.is_terminal() && bob.is_terminal() {
            break;
        }
    }

    assert!(
        alice_final >= 1,
        "Alice reached {:?} but never emitted the Finalized event that triggers persist",
        alice.phase(),
    );
    assert!(
        bob_final >= 1,
        "Bob reached {:?} but never emitted the Finalized event that triggers persist \
         — this is the Samsung half-exchange: finalized on-screen, contact never saved",
        bob.phase(),
    );
}

// @internal
#[test]
fn finalized_machine_exposes_combo_for_broadcast_seed() {
    // Regression: the finalized side's success-screen broadcast is a single
    // frozen frame. If it inherits a stale DATA chunk from the Complete-state
    // interleave, a still-Complete peer scans DATA forever and times out with
    // no contact — the device-proven half-exchange (2026-07-25 Pixel↔Samsung
    // Hover: Pixel finalized + persisted, Samsung read DATA for 58s then timed
    // out). The AppEngine seeds that broadcast with this COMBO so the trailing
    // peer always scans our RDYY.
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();
    let (alice, bob, ticks) = drive_two_party_glance(alice_card, bob_card, 4000);

    assert!(
        matches!(alice.phase(), MultiStagePhase::Finalized { .. }),
        "precondition: Alice must be Finalized after {ticks} ticks, got {:?}",
        alice.phase(),
    );
    let combo = alice
        .finalization_combo_qr()
        .expect("a finalized machine must expose a COMBO to seed the broadcast");
    assert!(
        combo.data.starts_with("CMBO"),
        "the broadcast seed must be a COMBO (VRFY+CONF+RDYY), got prefix {:?}",
        &combo.data[..combo.data.len().min(4)],
    );
    if matches!(bob.phase(), MultiStagePhase::Finalized { .. }) {
        assert!(
            bob.finalization_combo_qr()
                .expect("finalized slow peer must also expose a COMBO")
                .data
                .starts_with("CMBO"),
            "the slow peer's broadcast seed must also be a COMBO",
        );
    }
}

/// The device's `persist_exchanged_contact` step, distilled: build each
/// side's role-correct ratchet via the wrapper seam exactly as the
/// AppEngine does (`MultiStageMachine::build_exchange_ratchet`), then
/// round-trip the initiator's first message. A `None` build is the
/// ratchet-less save; a failed decrypt is a desynced ratchet pair — both
/// surface on-device as `unresolved=0, rejected=N`.
///
/// Identities are synthetic — `build_exchange_ratchet` only needs their
/// ordering for the deterministic initiator/responder role split (the
/// protocol layer supplies the real signing keys from the exchanged
/// payloads). Mirrors `vauchi-core`'s
/// `multistage_in_person_ratchet_round_trips`.
fn assert_finalized_pair_builds_matching_ratchet(
    alice: &MultiStageMachine,
    bob: &MultiStageMachine,
) {
    let alice_id = [1u8; 32];
    let bob_id = [2u8; 32];

    let alice_tk = alice
        .transport_key()
        .expect("alice transport key after finalize");
    let bob_tk = bob
        .transport_key()
        .expect("bob transport key after finalize");
    assert_eq!(
        alice_tk, bob_tk,
        "transport keys must match — they seed both the mailbox token and the ratchet root"
    );

    let (alice_ratchet, alice_is_initiator) = alice
        .build_exchange_ratchet(&alice_id, &bob_id)
        .expect("alice ratchet builds (None => device saves a ratchet-less contact)");
    let (bob_ratchet, bob_is_initiator) = bob
        .build_exchange_ratchet(&bob_id, &alice_id)
        .expect("bob ratchet builds (None => device saves a ratchet-less contact)");
    assert_ne!(
        alice_is_initiator, bob_is_initiator,
        "exactly one side must be the initiator"
    );

    // The responder has no sending chain until it receives the initiator's
    // first message, so the initiator speaks first (the device "edit my
    // card → sync" path).
    let (mut initiator, mut responder) = if alice_is_initiator {
        (alice_ratchet, bob_ratchet)
    } else {
        (bob_ratchet, alice_ratchet)
    };

    let msg = b"card update over multi-stage";
    let ct = initiator.encrypt(msg).expect("initiator encrypts");
    let pt = responder
        .decrypt(&ct)
        .expect("responder must decrypt the initiator's first message (else rejected=N)");
    assert_eq!(pt, msg, "the card update must survive the channel");
}

// @scenario: contact_exchange :: Multi-stage wrapper builds a working ratchet (lockstep)
#[test]
fn lockstep_glance_builds_matching_ratchet_pair() {
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();

    let (alice, bob, _ticks) = drive_two_party_glance(alice_card, bob_card, 4000);
    assert_finalized_pair_builds_matching_ratchet(&alice, &bob);
}

// @scenario: contact_exchange :: Multi-stage wrapper builds a working ratchet (asymmetric camera)
#[test]
fn asymmetric_glance_builds_matching_ratchet_pair() {
    let alice_card = b"name:Alice\nemail:alice@example.com".to_vec();
    let bob_card = b"name:Bob\nemail:bob@example.com".to_vec();

    // Bob 5× lossier — the Pixel-vs-S7 asymmetry the device exhibited.
    let (alice, bob, _ticks) = drive_asymmetric_glance(alice_card, bob_card, 5, 8000);
    assert_finalized_pair_builds_matching_ratchet(&alice, &bob);
}
