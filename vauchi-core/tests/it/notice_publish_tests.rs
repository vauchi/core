// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Publishing a group's body in content records, and reading it from
//! pending update keys (protocol section 4; vauchi/private#1281).

use vauchi_core::notice::chain::PERIOD_SECONDS;
use vauchi_core::notice::content::{RECORD_BYTES, commitment};
use vauchi_core::notice::direction::week;
use vauchi_core::notice::publish::{Pending, Publisher};

const DAY: u64 = 86_400;
const WEEK: u64 = PERIOD_SECONDS;
/// The start of a week.
const W0: u64 = 2_976 * WEEK;

type Record = [u8; RECORD_BYTES];

fn body(length: usize, seed: u8) -> Vec<u8> {
    (0..length).map(|i| seed.wrapping_add(i as u8)).collect()
}

fn accept_all(_: &[u8]) -> bool {
    true
}

/// A session of the sender with room for `slots` content records,
/// acknowledged.
fn session(publisher: &mut Publisher, now: u64, slots: usize) -> Vec<Record> {
    publisher.queue_due(now);
    let written = publisher.write(now, slots);
    publisher.acknowledge(now);
    written
}

// @internal
#[test]
fn an_edit_is_published_in_the_current_week_and_read_whole() {
    let key = [7; 32];
    let text = body(1_500, 1);
    let mut publisher = Publisher::new(key, &text);
    let written = session(&mut publisher, W0 + DAY, 8);
    assert_eq!(written.len(), 3, "1,500 bytes take three records");
    assert_eq!(publisher.last_week(), Some(week(W0 + DAY)));

    let mut pending = Pending::default();
    assert!(pending.note(&key, &commitment(&key, &text), W0 + DAY));
    let bodies = pending.read(&written, W0 + 2 * DAY, accept_all);
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].as_slice(), text.as_slice());
    assert_eq!(
        pending.read(&written, W0 + 2 * DAY, accept_all).len(),
        0,
        "installed once"
    );
}

// Gate review 4 (Codex): a clock set back and corrected brings the weekly
// publication forward; it must wait for a later week instead of taking a
// week number ahead of the clock.
// @internal
#[test]
fn a_republication_brought_forward_by_a_clock_set_back_waits_for_a_later_week() {
    let key = [8; 32];
    let mut publisher = Publisher::new(key, &body(100, 2));
    session(&mut publisher, W0 + 1, 2);
    let mut now = W0 + 1;
    for _ in 0..6 {
        // A session three days back, just after the last completion,
        // pulls the completion time with it; four days later the weekly
        // publication is due within the same calendar week.
        session(&mut publisher, now - 3 * DAY + 1, 2);
        now += 4 * DAY + 1;
        session(&mut publisher, now, 2);
        let last = publisher.last_week().expect("published");
        assert!(
            last <= week(now),
            "week {last} is ahead of the clock's {}",
            week(now)
        );
    }
    assert!(
        publisher.last_week() > Some(week(W0)),
        "republished at least once"
    );
}

// @internal
#[test]
fn a_weekly_republication_reaches_a_receiver_whose_first_copy_was_lost() {
    let key = [9; 32];
    let text = body(600, 3);
    let mut publisher = Publisher::new(key, &text);
    session(&mut publisher, W0 + DAY, 8);
    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), W0 + DAY);
    let mut installed = Vec::new();
    for day in 2..=12 {
        let written = session(&mut publisher, W0 + day * DAY, 8);
        if day >= 8 {
            installed.extend(pending.read(&written, W0 + day * DAY, accept_all));
        }
    }
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].as_slice(), text.as_slice());
}

// @internal
#[test]
fn an_unacknowledged_record_is_sent_again_under_a_new_id_at_most_four_times() {
    let mut publisher = Publisher::new([10; 32], &body(100, 4));
    publisher.queue_due(W0);
    let mut ids = std::collections::BTreeSet::new();
    let mut writes = 0;
    for hour in 0..10 {
        for record in publisher.write(W0 + hour * 3_600, 8) {
            assert!(ids.insert(record[..16].to_vec()), "an ID written twice");
            writes += 1;
        }
    }
    assert_eq!(writes, 4);
}

// @internal
#[test]
fn chunks_from_two_publications_complete_one_body() {
    let key = [11; 32];
    let text = body(1_100, 5);
    let mut publisher = Publisher::new(key, &text);
    let first = session(&mut publisher, W0 + DAY, 8);
    let second = session(&mut publisher, W0 + DAY + WEEK, 8);
    assert_eq!((first.len(), second.len()), (3, 3));
    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), W0 + DAY);
    assert_eq!(
        pending
            .read(&first[..1], W0 + 2 * DAY + WEEK, accept_all)
            .len(),
        0
    );
    let bodies = pending.read(&second[1..], W0 + 2 * DAY + WEEK, accept_all);
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].as_slice(), text.as_slice());
}

// @internal
#[test]
fn a_publication_is_found_from_two_weeks_back_to_one_ahead_and_no_further() {
    let key = [12; 32];
    let text = body(50, 6);
    for (weeks_old, found) in [(-1i64, true), (0, true), (2, true), (3, false)] {
        let mut publisher = Publisher::new(key, &text);
        let written_at = (W0 as i64 + DAY as i64 - weeks_old * WEEK as i64) as u64;
        let written = session(&mut publisher, written_at, 8);
        let mut pending = Pending::default();
        pending.note(&key, &commitment(&key, &text), W0);
        let bodies = pending.read(&written, W0 + DAY, accept_all);
        assert_eq!(bodies.len() == 1, found, "{weeks_old} weeks old");
    }
}

// @internal
#[test]
fn a_body_that_fails_its_signature_is_dropped_with_its_entry() {
    let key = [13; 32];
    let text = body(50, 7);
    let mut publisher = Publisher::new(key, &text);
    let written = session(&mut publisher, W0, 8);
    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), W0);
    assert_eq!(pending.read(&written, W0, |_| false).len(), 0);
    assert_eq!(
        pending.read(&written, W0, accept_all).len(),
        0,
        "the entry is gone"
    );
    assert!(
        pending.note(&key, &commitment(&key, &text), W0),
        "its key may come again"
    );
}

// @internal
#[test]
fn an_altered_record_or_a_wrong_commitment_installs_nothing() {
    let key = [14; 32];
    let text = body(50, 8);
    let mut publisher = Publisher::new(key, &text);
    let written = session(&mut publisher, W0, 8);
    let mut altered = written.clone();
    altered[0][60] ^= 1;
    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), W0);
    assert_eq!(pending.read(&altered, W0, accept_all).len(), 0);
    let mut other = Pending::default();
    other.note(&key, &commitment(&key, b"another body"), W0);
    assert_eq!(other.read(&written, W0, accept_all).len(), 0);
    assert_eq!(
        pending.read(&written, W0, accept_all).len(),
        1,
        "the honest record still reads"
    );
}

// @internal
#[test]
fn pending_keys_are_kept_14_days_and_32_a_direction() {
    let text = body(50, 9);
    let mut pending = Pending::default();
    assert!(pending.note(&[1; 32], &commitment(&[1; 32], &text), W0));
    assert!(
        !pending.note(&[1; 32], &commitment(&[1; 32], &text), W0),
        "already pending"
    );
    let mut publisher = Publisher::new([1; 32], &text);
    let written = session(&mut publisher, W0 + 14 * DAY, 8);
    assert_eq!(
        pending.read(&written, W0 + 14 * DAY, accept_all).len(),
        0,
        "expired"
    );

    let mut full = Pending::default();
    for seed in 0..33u8 {
        full.note(
            &[seed; 32],
            &commitment(&[seed; 32], &text),
            W0 + u64::from(seed),
        );
    }
    let mut oldest = Publisher::new([0; 32], &text);
    let written = session(&mut oldest, W0 + DAY, 8);
    assert_eq!(
        full.read(&written, W0 + DAY, accept_all).len(),
        0,
        "the oldest was dropped"
    );
    let mut newest = Publisher::new([32; 32], &text);
    let written = session(&mut newest, W0 + DAY, 8);
    assert_eq!(full.read(&written, W0 + DAY, accept_all).len(), 1);
    assert!(
        !full.note(&[32; 32], &commitment(&[32; 32], &text), W0 + DAY),
        "an installed commitment is not pending again"
    );
}
