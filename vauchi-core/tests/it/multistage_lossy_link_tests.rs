// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-stage exchange over a *lossy* frame link.
//!
//! The lossless harness in `multistage_e2e_tests` delivers every displayed
//! frame to the peer, which no camera does. A real scanner samples the
//! peer's screen on its own clock, so each side sees roughly one frame in
//! `every`, offset by `phase`. Whether the protocol converges under that
//! sampling is what device runs actually exercise
//! (`2026-08-18-hover-transfer-stalls-on-the-last-chunk`).

use vauchi_core::exchange::multistage::qr_codec::{StageQr, parse_qr};
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;

/// How a camera samples the peer's display: one frame in `every`, at `phase`.
#[derive(Clone, Copy, Debug)]
struct Sampling {
    every: usize,
    phase: usize,
}

impl Sampling {
    fn catches(self, tick: usize) -> bool {
        tick % self.every == self.phase
    }
}

/// Drive two sessions across a sampled link. Returns the tick both reached
/// `Finalized`, or `None` if they were still unfinished after `ticks`.
fn exchange_over_sampled_link(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
    alice_sees: Sampling,
    bob_sees: Sampling,
    ticks: usize,
) -> Option<usize> {
    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    for tick in 0..ticks {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq
            && bob_sees.catches(tick)
        {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq
            && alice_sees.catches(tick)
        {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            return Some(tick);
        }
    }
    None
}

fn three_chunk_card(tag: u8) -> Vec<u8> {
    vec![tag; 150]
}

// @internal
#[test]
fn exchange_completes_across_every_camera_sampling_offset() {
    let mut stalled = Vec::new();

    for alice_every in 1..=5 {
        for alice_phase in 0..alice_every {
            for bob_every in 1..=5 {
                for bob_phase in 0..bob_every {
                    let alice_sees = Sampling {
                        every: alice_every,
                        phase: alice_phase,
                    };
                    let bob_sees = Sampling {
                        every: bob_every,
                        phase: bob_phase,
                    };
                    if exchange_over_sampled_link(
                        three_chunk_card(0xA1),
                        three_chunk_card(0xB2),
                        alice_sees,
                        bob_sees,
                        4000,
                    )
                    .is_none()
                    {
                        stalled.push((alice_sees, bob_sees));
                    }
                }
            }
        }
    }

    assert!(
        stalled.is_empty(),
        "exchange never finalized for {} sampling combination(s): {:?}",
        stalled.len(),
        stalled
    );
}

/// Every chunk index a transferring session still owes, as seen by a camera
/// that samples one frame in `every` starting at `phase`.
fn chunks_seen_by_sampler(every: usize, phase: usize, ticks: usize) -> Vec<u16> {
    let mut alice = MultiStageSession::new(three_chunk_card(0xA1));
    let mut bob = MultiStageSession::new(three_chunk_card(0xB2));
    let bi = bob.get_display_qr().expect("bob advertises");
    alice.get_display_qr().expect("alice advertises");
    alice.process_scanned_qr(&bi.data);

    let mut seen = Vec::new();
    for tick in 0..ticks {
        let Some(frame) = alice.get_display_qr() else {
            continue;
        };
        if tick % every != phase {
            continue;
        }
        if let Ok(StageQr::Data { chunk_idx, .. }) = parse_qr(&frame.data)
            && !seen.contains(&chunk_idx)
        {
            seen.push(chunk_idx);
        }
    }
    seen.sort_unstable();
    seen
}

// @internal
#[test]
fn every_outstanding_chunk_reaches_a_periodically_sampling_scanner() {
    let mut starved = Vec::new();

    for every in 1..=6 {
        for phase in 0..every {
            let seen = chunks_seen_by_sampler(every, phase, 600);
            if seen != vec![0, 1, 2] {
                starved.push((every, phase, seen));
            }
        }
    }

    assert!(
        starved.is_empty(),
        "a scanner sampling at a fixed period never saw every outstanding chunk \
         (every, phase, chunks seen): {starved:?}"
    );
}

/// The device stall of 2026-08-19 left the Pixel holding the peer's chunks
/// 0 and 2 while the peer kept re-showing chunk 2 — the behaviour of a peer
/// that believes we hold 0 and 1. If a `{0, 2}` ACK survived the wire as
/// `{0, 1}`, that would explain it exactly.
// @internal
#[test]
fn a_sparse_ack_bitmap_survives_the_data_frame_round_trip() {
    let session_id = [7u8; 16];
    let mut held = vauchi_core::exchange::multistage::types::ChunkBitmap::new(3);
    held.mark_received(0);
    held.mark_received(2);

    let qr = vauchi_core::exchange::multistage::qr_codec::format_data_qr(
        &session_id,
        1,
        3,
        &held.to_bytes(),
        b"ciphertext",
    );

    let StageQr::Data { ack_bitmap, .. } = parse_qr(&qr).expect("DATA parses") else {
        panic!("expected a DATA frame");
    };
    let decoded = vauchi_core::exchange::multistage::types::ChunkBitmap::from_bytes(&ack_bitmap, 3);

    assert_eq!(
        (decoded.has(0), decoded.has(1), decoded.has(2)),
        (true, false, true),
        "an ACK for chunks 0 and 2 must arrive as 0 and 2"
    );
}

// ── ADR-071: a session re-handshakes when its peer restarts ──────────────
//
// Two people never tap Hover on the same second. Whichever side scans first
// advances alone, and from `Verifying` on it can no longer accept the other's
// INIT — device-observed as 19 decodes per second in which not one frame was
// usable by either side (2026-08-19 Pixel ↔ iPhone).

/// Drive `session` to `Verifying` against a throwaway peer, so it is bound to
/// a peer that will never be heard from again — the state a side lands in when
/// its partner restarts.
fn session_stranded_in_verifying() -> (MultiStageSession, MultiStageSession, StrandedClock) {
    // Both sides share one clock so that advancing it models real elapsed
    // time for the pair. Leaving a session on the system clock, or freezing
    // the fake one after setup, means a stall can never be detected twice —
    // an artifact that made recovery look intermittent when it was the test
    // standing still.
    let clock = std::sync::Arc::new(vauchi_core::monotonic::FakeMonotonicClock::new());
    let mut ahead = MultiStageSession::new(three_chunk_card(0xA1)).with_monotonic(clock.clone());
    let mut gone = MultiStageSession::new(three_chunk_card(0xB2)).with_monotonic(clock.clone());
    let ai = ahead.get_display_qr().expect("advertises");
    let bi = gone.get_display_qr().expect("advertises");
    ahead.process_scanned_qr(&bi.data);
    gone.process_scanned_qr(&ai.data);
    for _ in 0..400 {
        if matches!(ahead.get_state(), ProtocolState::Verifying) {
            break;
        }
        let aq = ahead.get_display_qr();
        let bq = gone.get_display_qr();
        if let Some(bq) = &bq {
            ahead.process_scanned_qr(&bq.data);
        }
        if let Some(aq) = &aq {
            gone.process_scanned_qr(&aq.data);
        }
    }
    (ahead, gone, clock)
}

/// The stranded side's clock, so a stall can be reached without waiting for
/// one (CC-06: no real-time sleeps in tests).
type StrandedClock = std::sync::Arc<vauchi_core::monotonic::FakeMonotonicClock>;

/// A single stray INIT decode must never disturb a session. The guard exists
/// because a bystander's QR reaching the camera once is not evidence of
/// anything.
// @internal
#[test]
fn one_foreign_init_does_not_reset_a_session() {
    let (mut ahead, _gone, clock) = session_stranded_in_verifying();
    // Even long past the stall threshold, one sighting is not evidence.
    clock.advance(std::time::Duration::from_secs(60));
    assert!(
        matches!(ahead.get_state(), ProtocolState::Verifying),
        "precondition: stranded past Advertising, got {:?}",
        ahead.get_state()
    );

    let mut stranger = MultiStageSession::new(three_chunk_card(0xCC));
    let init = stranger.get_display_qr().expect("advertises");
    ahead.process_scanned_qr(&init.data);

    assert!(
        matches!(ahead.get_state(), ProtocolState::Verifying),
        "one foreign INIT must not reset, got {:?}",
        ahead.get_state()
    );
}

/// A peer that restarted shows its new INIT continuously. Once that has gone
/// on long enough with no progress, the stranded side must stand down and
/// re-advertise, or the pair is deadlocked until the 120 s step timeout.
// @internal
#[test]
fn a_restarted_peer_pulls_a_stranded_session_back_to_advertising() {
    let (mut ahead, _gone, clock) = session_stranded_in_verifying();
    let mut restarted =
        MultiStageSession::new(three_chunk_card(0xCC)).with_monotonic(clock.clone());
    let init = restarted.get_display_qr().expect("advertises");

    clock.advance(std::time::Duration::from_secs(6));
    for _ in 0..40 {
        ahead.process_scanned_qr(&init.data);
        let _ = ahead.get_display_qr();
    }

    assert!(
        !matches!(ahead.get_state(), ProtocolState::Verifying),
        "a peer advertising a different session must break the stranded \
         binding, got {:?}",
        ahead.get_state()
    );
    assert_eq!(
        ahead.peer_session_id(),
        Some(restarted.session_id()),
        "after standing down we must be bound to the peer that is actually \
         there, not the one that went away"
    );
}

/// After the reset the pair must actually complete — a reset that only
/// re-advertises without re-handshaking would trade one deadlock for another.
// @internal
#[test]
fn a_pair_recovers_and_completes_after_a_staggered_start() {
    let (mut ahead, _gone, clock) = session_stranded_in_verifying();
    let mut late = MultiStageSession::new(three_chunk_card(0xCC)).with_monotonic(clock.clone());
    clock.advance(std::time::Duration::from_secs(6));

    let mut finished = false;
    for _ in 0..4000 {
        clock.advance(std::time::Duration::from_millis(300));
        let aq = ahead.get_display_qr();
        let lq = late.get_display_qr();
        if let Some(lq) = &lq {
            ahead.process_scanned_qr(&lq.data);
        }
        if let Some(aq) = &aq {
            late.process_scanned_qr(&aq.data);
        }
        if matches!(ahead.get_state(), ProtocolState::Finalized)
            && matches!(late.get_state(), ProtocolState::Finalized)
        {
            finished = true;
            break;
        }
    }

    assert!(
        finished,
        "a staggered start must recover and complete; ahead={:?} late={:?}",
        ahead.get_state(),
        late.get_state()
    );
}

/// A DATA frame carries an empty ACK until its sender has received anything,
/// which is every frame at the start of an exchange. On device 87 such frames
/// were decoded and none reached the machine, while the 7 carrying a one-byte
/// ACK all did (2026-08-19 Hover run, `decrypt_fail=0` throughout — they never
/// arrived, rather than arriving broken).
// @internal
#[test]
fn a_data_frame_with_an_empty_ack_still_parses() {
    let session_id = [9u8; 16];
    let qr = vauchi_core::exchange::multistage::qr_codec::format_data_qr(
        &session_id,
        2,
        3,
        &[],
        b"short-final-chunk",
    );

    let parsed = parse_qr(&qr).expect("a DATA frame with no ACK yet must parse");
    let StageQr::Data {
        chunk_idx,
        ack_bitmap,
        ..
    } = parsed
    else {
        panic!("expected a DATA frame");
    };
    assert_eq!(chunk_idx, 2);
    assert!(ack_bitmap.is_empty(), "an empty ACK stays empty");
}

/// Both sides bound to a peer that is gone — the device case. Each one's
/// reset mints a new ephemeral, invalidating the key the other just derived,
/// so a reset that waits to be advertised at again can race forever: three
/// re-handshakes on device each ended with decrypt_fail still climbing
/// (2026-08-19). They must converge.
// @internal
#[test]
fn two_mutually_stranded_sessions_converge_instead_of_racing() {
    let (mut left, _gone_l, clock_l) = session_stranded_in_verifying();
    let (mut right, _gone_r, clock_r) = session_stranded_in_verifying();
    clock_l.advance(std::time::Duration::from_secs(6));
    clock_r.advance(std::time::Duration::from_secs(6));

    let mut finished = false;
    for _ in 0..4000 {
        clock_l.advance(std::time::Duration::from_millis(300));
        clock_r.advance(std::time::Duration::from_millis(300));
        let lq = left.get_display_qr();
        let rq = right.get_display_qr();
        if let Some(rq) = &rq {
            left.process_scanned_qr(&rq.data);
        }
        if let Some(lq) = &lq {
            right.process_scanned_qr(&lq.data);
        }
        if matches!(left.get_state(), ProtocolState::Finalized)
            && matches!(right.get_state(), ProtocolState::Finalized)
        {
            finished = true;
            break;
        }
    }

    assert!(
        finished,
        "two stranded sessions must re-handshake with each other rather than \
         resetting past one another; left={:?} right={:?}",
        left.get_state(),
        right.get_state()
    );
}

/// A peer that finishes first sends VRFY while we are still collecting its
/// chunks. That proves it has all of *ours*; it says nothing about whether we
/// have all of *its*. Fast-tracking to Verifying regardless meant the next
/// VRFY hit an incomplete reassembly and killed an exchange that was
/// progressing — the mechanism behind ADR-071 recovery converging only six
/// runs in ten (2026-08-19).
// @internal
#[test]
fn an_early_peer_vrfy_does_not_kill_an_incomplete_transfer() {
    let mut ours = MultiStageSession::new(three_chunk_card(0xA1));
    let mut peer = MultiStageSession::new(three_chunk_card(0xB2));
    let oi = ours.get_display_qr().expect("advertises");
    let pi = peer.get_display_qr().expect("advertises");
    ours.process_scanned_qr(&pi.data);
    peer.process_scanned_qr(&oi.data);
    assert!(
        matches!(ours.get_state(), ProtocolState::Transferring { .. }),
        "precondition: mid-transfer, got {:?}",
        ours.get_state()
    );

    // The peer reaches Verifying and repeats its VRFY, as it would while
    // waiting for us.
    let vrfy = vauchi_core::exchange::multistage::qr_codec::format_verify_qr(
        &peer.session_id(),
        &[7u8; 32],
    );
    for _ in 0..5 {
        ours.process_scanned_qr(&vrfy);
    }

    assert!(
        !matches!(ours.get_state(), ProtocolState::Failed(_)),
        "an early VRFY must not fail a transfer that can still complete, got {:?}",
        ours.get_state()
    );
}
