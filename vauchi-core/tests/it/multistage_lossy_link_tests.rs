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
