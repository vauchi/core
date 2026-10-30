// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The client spaces its OHTTP requests so a long catch-up never bursts
//! past the outer relay's per-address limit (vauchi/private#912): a short
//! burst goes out at once, the rest at a steady rate.

#![cfg(feature = "network-http")]

use std::sync::Arc;
use std::time::Duration;

use vauchi_core::monotonic::FakeMonotonicClock;
use vauchi_core::network::request_pacer::RequestPacer;
use vauchi_core::sleeper::FakeSleeper;

const BURST: u32 = 3;
const PER_SECOND: u32 = 10;

fn pacer() -> (RequestPacer, Arc<FakeMonotonicClock>, Arc<FakeSleeper>) {
    let clock = Arc::new(FakeMonotonicClock::new());
    let sleeper = Arc::new(FakeSleeper::new());
    let pacer = RequestPacer::new(BURST, PER_SECOND, clock.clone(), sleeper.clone());
    (pacer, clock, sleeper)
}

// @internal
#[test]
fn a_burst_up_to_its_size_goes_out_without_waiting() {
    let (pacer, _clock, sleeper) = pacer();

    for _ in 0..BURST {
        pacer.pace();
    }

    assert_eq!(sleeper.calls(), Vec::<Duration>::new());
}

// @internal
#[test]
fn requests_beyond_the_burst_each_wait_their_turn_at_the_rate() {
    let (pacer, _clock, sleeper) = pacer();

    for _ in 0..BURST + 3 {
        pacer.pace();
    }

    assert_eq!(
        sleeper.calls(),
        vec![
            Duration::from_millis(100),
            Duration::from_millis(200),
            Duration::from_millis(300),
        ]
    );
}

// @internal
#[test]
fn a_quiet_spell_refills_the_burst() {
    let (pacer, clock, sleeper) = pacer();
    for _ in 0..BURST {
        pacer.pace();
    }

    clock.advance(Duration::from_secs(1));
    for _ in 0..BURST {
        pacer.pace();
    }

    assert_eq!(sleeper.calls(), Vec::<Duration>::new());
}
