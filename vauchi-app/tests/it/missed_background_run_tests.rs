// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A background run that found storage locked leaves a record beside the
//! database, the only place it can write; the next open moves it into
//! memory and My Info says so until a sync succeeds (vauchi/private#1093
//! slice 3).
//!
//! Traces to: features/sync_updates.feature

use std::path::Path;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{
    AppEngine, AppScreen, Component, ScreenModel, WorkflowEngine, record_missed_background_run,
};
use vauchi_core::api::Vauchi;
use vauchi_core::clock::FakeClock;

const MISSED_AT: u64 = 1_700_000_000;

fn engine_opened_at(dir: &Path, now: u64) -> AppEngine {
    let clock = FakeClock::new(SystemTime::UNIX_EPOCH + Duration::from_secs(now)).shared();
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.keep_restorable_state_in(dir);
    engine.navigate_to(AppScreen::MyInfo);
    engine
}

fn caption(screen: &ScreenModel, id: &str) -> Option<String> {
    screen.components.iter().find_map(|c| match c {
        Component::Text {
            id: cid, content, ..
        } if cid == id => Some(content.clone()),
        _ => None,
    })
}

// @scenario: sync_updates :: A background check missed while the phone was locked shows after unlock
#[test]
fn a_run_missed_while_locked_shows_on_my_info_at_the_next_open() {
    let dir = tempfile::tempdir().unwrap();
    record_missed_background_run(dir.path(), MISSED_AT).expect("record written");

    let engine = engine_opened_at(dir.path(), MISSED_AT + 5 * 60);

    assert_eq!(
        caption(&engine.current_screen(), "missed_run_caption").as_deref(),
        Some("Couldn't check for updates while your phone was locked (5 minutes ago)"),
    );
}

// @internal
#[test]
fn the_record_is_shown_by_one_open_only() {
    let dir = tempfile::tempdir().unwrap();
    record_missed_background_run(dir.path(), MISSED_AT).expect("record written");
    let _first = engine_opened_at(dir.path(), MISSED_AT + 60);

    let second = engine_opened_at(dir.path(), MISSED_AT + 120);

    assert_eq!(
        caption(&second.current_screen(), "missed_run_caption"),
        None
    );
}

// @internal
#[test]
fn no_record_shows_no_missed_run() {
    let dir = tempfile::tempdir().unwrap();

    let engine = engine_opened_at(dir.path(), MISSED_AT);

    assert_eq!(
        caption(&engine.current_screen(), "missed_run_caption"),
        None
    );
}

// @internal
#[test]
fn a_later_record_replaces_an_earlier_one() {
    let dir = tempfile::tempdir().unwrap();
    record_missed_background_run(dir.path(), MISSED_AT).expect("first record");
    record_missed_background_run(dir.path(), MISSED_AT + 2 * 3600).expect("second record");

    let engine = engine_opened_at(dir.path(), MISSED_AT + 2 * 3600 + 60);

    assert_eq!(
        caption(&engine.current_screen(), "missed_run_caption").as_deref(),
        Some("Couldn't check for updates while your phone was locked (1 minute ago)"),
    );
}
