// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the content update cycle on a thread of its own, so every shell gets
//! CDN content without scheduling it (vauchi/private#733 D10): once after the
//! app has settled, then every interval, and whenever asked. Requests that
//! arrive while a cycle runs are merged into one more cycle.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct State {
    requested: bool,
    stopped: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// A running cycle thread; dropping it stops the thread after any cycle in
/// progress.
pub struct ContentScheduler {
    shared: Arc<Shared>,
}

impl ContentScheduler {
    /// Starts the thread that calls `run_cycle` after `first_run_after`,
    /// then every `interval`, and after each [`Self::request_cycle`].
    pub fn spawn(
        first_run_after: Duration,
        interval: Duration,
        run_cycle: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("vauchi-content".into())
            .spawn(move || run(&worker, first_run_after, interval, run_cycle))?;
        Ok(Self { shared })
    }

    /// Asks for a cycle now, without waiting for the interval.
    pub fn request_cycle(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.requested = true;
            self.shared.wake.notify_one();
        }
    }
}

impl Drop for ContentScheduler {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.stopped = true;
            self.shared.wake.notify_one();
        }
    }
}

fn run(shared: &Shared, first_run_after: Duration, interval: Duration, run_cycle: impl Fn()) {
    let mut wait = first_run_after;
    loop {
        let Ok(state) = shared.state.lock() else {
            return;
        };
        let Ok((mut state, _)) = shared
            .wake
            .wait_timeout_while(state, wait, |s| !s.requested && !s.stopped)
        else {
            return;
        };
        if state.stopped {
            return;
        }
        state.requested = false;
        drop(state);
        run_cycle();
        wait = interval;
    }
}
