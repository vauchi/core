// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core runs the content update cycle itself, on a thread of its own: once
//! the app has settled, then every interval, and whenever asked
//! (vauchi/private#733 D10). The tests wait on the cycle reporting in,
//! never on a fixed sleep (CC-06).

use std::sync::mpsc;
use std::time::Duration;
use vauchi_app::content::ContentScheduler;

const NEVER: Duration = Duration::from_secs(3600);
const PATIENCE: Duration = Duration::from_secs(10);

fn scheduler(first: Duration, interval: Duration) -> (ContentScheduler, mpsc::Receiver<()>) {
    let (ran, runs) = mpsc::channel();
    let scheduler = ContentScheduler::spawn(first, interval, move || {
        let _ = ran.send(());
    })
    .unwrap();
    (scheduler, runs)
}

// @internal
#[test]
fn the_first_cycle_runs_once_the_app_has_settled() {
    let (_scheduler, runs) = scheduler(Duration::ZERO, NEVER);

    assert_eq!(runs.recv_timeout(PATIENCE), Ok(()));
}

// @internal
#[test]
fn a_request_runs_a_cycle_without_waiting_for_the_interval() {
    let (scheduler, runs) = scheduler(NEVER, NEVER);

    scheduler.request_cycle();

    assert_eq!(runs.recv_timeout(PATIENCE), Ok(()));
}

// @internal
#[test]
fn cycles_repeat_every_interval() {
    let (_scheduler, runs) = scheduler(Duration::ZERO, Duration::from_millis(5));

    for run in 0..3 {
        assert_eq!(runs.recv_timeout(PATIENCE), Ok(()), "run {run}");
    }
}

// @internal
#[test]
fn dropping_the_scheduler_ends_its_thread_without_a_cycle() {
    let (scheduler, runs) = scheduler(NEVER, NEVER);

    drop(scheduler);

    assert_eq!(
        runs.recv_timeout(PATIENCE),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}
