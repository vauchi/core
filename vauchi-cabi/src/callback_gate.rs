// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one place a shell's C event callback lives, and the only way it is
//! called.
//!
//! A shell frees its callback's `user_data` as soon as
//! `vauchi_app_set_event_callback` or `vauchi_app_destroy` returns, so
//! neither may return while the old callback still runs on another thread
//! (vauchi/private#1218). Callbacks run one at a time: two callbacks that
//! each replaced the handler would otherwise wait for each other forever.
//! A callback that replaces the handler from inside itself does not wait
//! for itself, and a replacement waits only for callbacks of earlier
//! registrations, so a stream of new events cannot hold it forever.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

use crate::app::EventCallbackHandler;

#[derive(Default)]
struct State {
    handler: Option<EventCallbackHandler>,
    /// Bumped by every replacement; a running callback carries the
    /// registration it was started under.
    registration: u64,
    running: Option<(ThreadId, u64)>,
    closed: bool,
    replacers_waiting: usize,
}

#[derive(Default)]
pub(crate) struct CallbackGate {
    state: Mutex<State>,
    changed: Condvar,
}

impl CallbackGate {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A callback cannot unwind into Rust and nothing else here panics,
        // so a poisoned lock still holds a consistent state.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Calls the registered callback, if any, with the affected screens.
    pub(crate) fn dispatch(&self, screens: &[String]) {
        let me = thread::current().id();
        let mut state = self.lock();
        while state.running.is_some_and(|(owner, _)| owner != me) {
            state = self.wait(state);
        }
        if state.running.is_some() {
            // A callback on this thread is already running; calling it again
            // from inside itself is never what a shell expects.
            return;
        }
        let Some(handler) = state.handler else {
            return;
        };
        state.running = Some((me, state.registration));
        drop(state);

        let _done = Running(self);
        handler.dispatch(screens);
    }

    /// Installs `handler` (or none) and returns once no callback of an
    /// earlier registration is running on another thread. Ignored after
    /// [`Self::close`].
    pub(crate) fn replace(&self, handler: Option<EventCallbackHandler>) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        state.handler = handler;
        self.drain(state);
    }

    /// Removes the callback for good: nothing can be registered again, and
    /// it returns once no callback runs on another thread.
    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.handler = None;
        self.drain(state);
    }

    fn drain(&self, mut state: MutexGuard<'_, State>) {
        state.registration += 1;
        let registration = state.registration;
        let me = thread::current().id();
        state.replacers_waiting += 1;
        while state
            .running
            .is_some_and(|(owner, started)| owner != me && started < registration)
        {
            state = self.wait(state);
        }
        state.replacers_waiting -= 1;
    }

    pub(crate) fn is_set(&self) -> bool {
        self.lock().handler.is_some()
    }

    /// Threads inside [`Self::replace`] or [`Self::close`] right now; lets a
    /// test order its threads without sleeping.
    // INLINE_TEST_REQUIRED: the only way a test can see that a replacement is
    // waiting, which it needs to prove the wait without a timed sleep (CC-06).
    #[cfg(test)]
    pub(crate) fn replacers_waiting(&self) -> usize {
        self.lock().replacers_waiting
    }
}

/// Marks the end of a running callback even if the dispatch unwinds.
struct Running<'a>(&'a CallbackGate);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.running = None;
        drop(state);
        self.0.changed.notify_all();
    }
}
