// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivers engine events to a shell listener from a thread of its own.
//!
//! Engine events fire inside engine methods, usually while the engine lock
//! is held. A listener called there that calls back into the engine
//! deadlocks, so the event handler only records what changed and this
//! notifier's thread makes the call (vauchi/private#677). Events that pile
//! up before the thread runs are merged into one delivery.

use std::collections::BTreeSet;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Default)]
struct Pending {
    due: bool,
    delivering: bool,
    screens: BTreeSet<String>,
    stopped: bool,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
    idle: Condvar,
}

/// A running delivery thread; dropping it stops the thread after any
/// delivery in progress.
pub struct EventNotifier {
    shared: Arc<Shared>,
}

impl EventNotifier {
    /// Starts the thread that calls `deliver` with the affected screen ids
    /// (deduplicated, possibly empty) each time [`Self::notify`] has been
    /// called since the last delivery.
    pub fn spawn(deliver: impl Fn(Vec<String>) + Send + 'static) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("vauchi-events".into())
            .spawn(move || run(&worker, deliver))?;
        Ok(Self { shared })
    }

    /// Records an event affecting `screens` and wakes the delivery thread.
    /// Never calls the listener itself, so it is safe under the engine lock.
    pub fn notify<S: Into<String>>(&self, screens: impl IntoIterator<Item = S>) {
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.due = true;
            pending.screens.extend(screens.into_iter().map(Into::into));
            self.shared.wake.notify_one();
        }
    }
}

impl EventNotifier {
    /// Blocks until every recorded event has been delivered. For tests,
    /// which otherwise could only sleep (CC-06); a no-op once stopped.
    pub fn flush(&self) {
        let Ok(mut pending) = self.shared.pending.lock() else {
            return;
        };
        while (pending.due || pending.delivering) && !pending.stopped {
            pending = match self.shared.idle.wait(pending) {
                Ok(guard) => guard,
                Err(_) => return,
            };
        }
    }
}

impl Drop for EventNotifier {
    fn drop(&mut self) {
        // Not joined: a listener may replace itself from inside its own
        // callback, which runs on the thread being stopped.
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.stopped = true;
            self.shared.wake.notify_one();
            self.shared.idle.notify_all();
        }
    }
}

fn run(shared: &Shared, deliver: impl Fn(Vec<String>)) {
    loop {
        let screens = {
            let Ok(mut pending) = shared.pending.lock() else {
                return;
            };
            while !pending.due && !pending.stopped {
                pending = match shared.wake.wait(pending) {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
            }
            if pending.stopped {
                return;
            }
            pending.due = false;
            pending.delivering = true;
            std::mem::take(&mut pending.screens)
        };
        deliver(screens.into_iter().collect());
        if let Ok(mut pending) = shared.pending.lock() {
            pending.delivering = false;
            shared.idle.notify_all();
        }
    }
}
