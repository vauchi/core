// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Multi-stage exchange where one camera reads the peer's code at one
//! layout only, and only on some of the frames drawn there.
//!
//! Two phones face to face see part of each other's screen, which is why the
//! link trainer moves the code between 14 layouts until the peer echoes
//! reads. A camera that reads every frame, or reads frames at random
//! regardless of where they are drawn, never stalled in simulation; one that
//! reads a single layout did, with the faster side finalized and the slower
//! one left in `Verifying` (vauchi/private#952).

use std::sync::Arc;
use std::time::Duration;

use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::monotonic::FakeMonotonicClock;

/// A seeded xorshift, so every run is reproducible by its seed.
struct Draws(u64);

impl Draws {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % 100 < percent
    }
}

/// Alice reads every frame Bob draws. Bob reads Alice's frames drawn at
/// `bob_reads_layout`, each with `percent` chance. Time advances by the dwell
/// of each frame Alice shows. Whether both finalized within `frames`.
fn both_finalize(seed: u64, bob_reads_layout: u8, percent: u64, frames: usize) -> bool {
    let clock = Arc::new(FakeMonotonicClock::new());
    let mut alice = MultiStageSession::new(vec![0xA1; 62]).with_monotonic(clock.clone());
    let mut bob = MultiStageSession::new(vec![0xB2; 62]).with_monotonic(clock.clone());
    let mut draws = Draws::new(seed);

    for _ in 0..frames {
        let alice_frame = alice.get_display_qr();
        let bob_frame = bob.get_display_qr();
        if let Some(frame) = &alice_frame {
            clock.advance(Duration::from_millis(u64::from(frame.display_duration_ms)));
            if frame.layout == bob_reads_layout && draws.chance(percent) {
                bob.process_scanned_qr(&frame.data);
            }
        }
        if let Some(frame) = &bob_frame {
            alice.process_scanned_qr(&frame.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            return true;
        }
    }
    false
}

// @internal
#[test]
fn a_peer_that_reads_one_layout_finishes_once_the_other_side_has_saved() {
    let stuck: Vec<u64> = (1..=200)
        .filter(|&seed| !both_finalize(seed, 3, 20, 3000))
        .collect();

    assert_eq!(stuck, Vec::<u64>::new(), "seeds that never finalized");
}
