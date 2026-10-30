// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Spaces a client's requests to one relay hop (vauchi/private#912).
//!
//! The outer OHTTP relay limits each address (50 requests a second by
//! default), and since #890 a catch-up costs a padded batch and its
//! fetches per owed day. A token bucket lets a normal sync go out at once
//! and paces the rest, so in any `t` seconds a client sends at most
//! `burst + rate·t` requests.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::monotonic::{MonotonicClock, SystemMonotonicClock};
use crate::sleeper::{Sleeper, SystemSleeper};

/// A daily sync (about 20 requests for two day epochs, measured end to
/// end) goes out unpaced.
pub const OHTTP_BURST: u32 = 25;
/// The outer relay's bucket holds and refills 50 a second by default, so
/// two clients behind one address (2 × (25 + 20·t)) still fit.
pub const OHTTP_REQUESTS_PER_SECOND: u32 = 20;

pub struct RequestPacer {
    burst: f64,
    per_second: f64,
    clock: Arc<dyn MonotonicClock>,
    sleeper: Arc<dyn Sleeper>,
    bucket: Mutex<Bucket>,
}

struct Bucket {
    /// Negative while requests are queued behind the rate.
    tokens: f64,
    refilled_at: Instant,
}

impl RequestPacer {
    pub fn new(
        burst: u32,
        per_second: u32,
        clock: Arc<dyn MonotonicClock>,
        sleeper: Arc<dyn Sleeper>,
    ) -> Self {
        let refilled_at = clock.now();
        Self {
            burst: f64::from(burst.max(1)),
            per_second: f64::from(per_second.max(1)),
            clock,
            sleeper,
            bucket: Mutex::new(Bucket {
                tokens: f64::from(burst.max(1)),
                refilled_at,
            }),
        }
    }

    /// The OHTTP pacing with the system clock and real sleeps.
    pub fn ohttp() -> Self {
        Self::ohttp_with(SystemMonotonicClock::shared(), SystemSleeper::shared())
    }

    /// The OHTTP pacing on an injected clock and sleeper.
    pub fn ohttp_with(clock: Arc<dyn MonotonicClock>, sleeper: Arc<dyn Sleeper>) -> Self {
        Self::new(OHTTP_BURST, OHTTP_REQUESTS_PER_SECOND, clock, sleeper)
    }

    /// Returns once the next request may go. The slot is reserved under
    /// the lock and waited for outside it, so concurrent callers queue in
    /// order without holding each other up.
    pub fn pace(&self) {
        let wait = {
            let mut bucket = self.bucket.lock().unwrap_or_else(PoisonError::into_inner);
            let now = self.clock.now();
            let elapsed = now.saturating_duration_since(bucket.refilled_at);
            bucket.tokens =
                (bucket.tokens + elapsed.as_secs_f64() * self.per_second).min(self.burst);
            bucket.refilled_at = now;
            bucket.tokens -= 1.0;
            if bucket.tokens < 0.0 {
                Duration::from_secs_f64(-bucket.tokens / self.per_second)
            } else {
                Duration::ZERO
            }
        };
        if !wait.is_zero() {
            self.sleeper.sleep(wait);
        }
    }
}
