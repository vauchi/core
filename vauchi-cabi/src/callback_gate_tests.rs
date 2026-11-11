// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A shell frees its callback's `user_data` once `vauchi_app_set_event_callback`
//! or `vauchi_app_destroy` returns (the Qt window does, in its destructor), so
//! neither may return while the old callback still runs on another thread
//! (vauchi/private#1218). Barriers order the threads; no test sleeps.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use crate::app::EventCallbackHandler;
use crate::callback_gate::CallbackGate;

/// A callback that stops in the middle until the test lets it finish.
struct Blocking {
    entered: Barrier,
    release: Barrier,
    finished: AtomicBool,
}

impl Blocking {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Barrier::new(2),
            release: Barrier::new(2),
            finished: AtomicBool::new(false),
        })
    }

    fn handler(self: &Arc<Self>) -> EventCallbackHandler {
        EventCallbackHandler::new(blocking_cb, Arc::as_ptr(self) as *mut c_void)
    }
}

unsafe extern "C" fn blocking_cb(_: *const c_char, user_data: *mut c_void) {
    // SAFETY: user_data is the `Blocking` the test keeps alive.
    let probe = unsafe { &*(user_data as *const Blocking) };
    probe.entered.wait();
    probe.release.wait();
    probe.finished.store(true, Ordering::SeqCst);
}

struct Counting(AtomicUsize);

unsafe extern "C" fn counting_cb(_: *const c_char, user_data: *mut c_void) {
    // SAFETY: user_data is the `Counting` the test keeps alive.
    let probe = unsafe { &*(user_data as *const Counting) };
    probe.0.fetch_add(1, Ordering::SeqCst);
}

fn counting(probe: &Arc<Counting>) -> EventCallbackHandler {
    EventCallbackHandler::new(counting_cb, Arc::as_ptr(probe) as *mut c_void)
}

/// Yields the processor at most this often while waiting for another thread;
/// it only bounds a failing run (Core code reads no clock, tests included).
const SPIN_LIMIT: u64 = 50_000_000;

/// Spins until `done` or the gate has a thread waiting to replace.
fn until_waiting_or_done(gate: &CallbackGate, done: &thread::JoinHandle<bool>) {
    let mut spins = 0u64;
    while !done.is_finished() && gate.replacers_waiting() == 0 {
        spins += 1;
        assert!(spins < SPIN_LIMIT, "neither returned nor waited");
        thread::yield_now();
    }
}

// @internal
#[test]
fn clearing_waits_for_a_callback_running_on_another_thread() {
    let gate = Arc::new(CallbackGate::default());
    let probe = Blocking::new();
    gate.replace(Some(probe.handler()));
    let dispatcher = {
        let gate = Arc::clone(&gate);
        thread::spawn(move || gate.dispatch(&[]))
    };
    probe.entered.wait();

    let clearer = {
        let (gate, probe) = (Arc::clone(&gate), Arc::clone(&probe));
        thread::spawn(move || {
            gate.replace(None);
            probe.finished.load(Ordering::SeqCst)
        })
    };
    until_waiting_or_done(&gate, &clearer);
    probe.release.wait();

    assert!(
        clearer.join().unwrap(),
        "replace(None) returned while the old callback was still running"
    );
    dispatcher.join().unwrap();
}

struct SelfClearing {
    gate: Arc<CallbackGate>,
    cleared: AtomicBool,
}

unsafe extern "C" fn self_clearing_cb(_: *const c_char, user_data: *mut c_void) {
    // SAFETY: user_data is the `SelfClearing` the test keeps alive.
    let probe = unsafe { &*(user_data as *const SelfClearing) };
    probe.gate.replace(None);
    probe.cleared.store(true, Ordering::SeqCst);
}

// @internal
#[test]
fn a_callback_may_clear_itself_without_waiting_for_itself() {
    let gate = Arc::new(CallbackGate::default());
    let probe = Arc::new(SelfClearing {
        gate: Arc::clone(&gate),
        cleared: AtomicBool::new(false),
    });
    gate.replace(Some(EventCallbackHandler::new(
        self_clearing_cb,
        Arc::as_ptr(&probe) as *mut c_void,
    )));

    gate.dispatch(&[]);

    assert!(probe.cleared.load(Ordering::SeqCst));
    assert!(!gate.is_set());
}

// @internal
#[test]
fn no_callback_runs_after_it_was_cleared() {
    let gate = CallbackGate::default();
    let probe = Arc::new(Counting(AtomicUsize::new(0)));
    gate.replace(Some(counting(&probe)));
    gate.dispatch(&["contacts".to_string()]);
    gate.replace(None);
    gate.dispatch(&["contacts".to_string()]);
    assert_eq!(probe.0.load(Ordering::SeqCst), 1);
}

// @internal
#[test]
fn a_closed_gate_takes_no_new_callback() {
    let gate = CallbackGate::default();
    let probe = Arc::new(Counting(AtomicUsize::new(0)));
    gate.close();
    gate.replace(Some(counting(&probe)));
    gate.dispatch(&[]);
    assert!(!gate.is_set());
    assert_eq!(probe.0.load(Ordering::SeqCst), 0);
}

// @internal
#[test]
fn destroy_waits_for_a_callback_running_on_another_thread() {
    let dir = tempfile::tempdir().unwrap();
    let path = std::ffi::CString::new(dir.path().to_str().unwrap()).unwrap();
    // SAFETY: `path` is a valid C string; a null relay URL means the default.
    let app = unsafe { crate::app::vauchi_app_create_with_config(path.as_ptr(), std::ptr::null()) };
    assert!(!app.is_null());
    // SAFETY: `app` is live until the destroy below.
    let gate = Arc::clone(unsafe { &(*app).event_callback });
    let probe = Blocking::new();
    gate.replace(Some(probe.handler()));
    let dispatcher = {
        let gate = Arc::clone(&gate);
        thread::spawn(move || gate.dispatch(&[]))
    };
    probe.entered.wait();

    let app_addr = app as usize;
    let destroyer = {
        let probe = Arc::clone(&probe);
        thread::spawn(move || {
            // SAFETY: the address is the live app; nothing else uses it after.
            unsafe { crate::app::vauchi_app_destroy(app_addr as *mut crate::VauchiApp) };
            probe.finished.load(Ordering::SeqCst)
        })
    };
    until_waiting_or_done(&gate, &destroyer);
    probe.release.wait();

    assert!(
        destroyer.join().unwrap(),
        "vauchi_app_destroy returned while the callback was still running"
    );
    dispatcher.join().unwrap();
    assert!(!gate.is_set(), "a destroyed app's gate holds no callback");
}
