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
fn pending_keys_are_kept_21_days_and_32_a_direction() {
    let text = body(50, 9);
    let mut pending = Pending::default();
    assert!(pending.note(&[1; 32], &commitment(&[1; 32], &text), W0));
    assert!(
        !pending.note(&[1; 32], &commitment(&[1; 32], &text), W0),
        "already pending"
    );
    let mut publisher = Publisher::new([1; 32], &text);
    let mut kept = Pending::default();
    kept.note(&[1; 32], &commitment(&[1; 32], &text), W0);
    let early = session(&mut Publisher::new([1; 32], &text), W0 + 20 * DAY, 8);
    assert_eq!(
        kept.read(&early, W0 + 20 * DAY, accept_all).len(),
        1,
        "kept 20 days"
    );
    let written = session(&mut publisher, W0 + 21 * DAY, 8);
    assert_eq!(
        pending.read(&written, W0 + 21 * DAY, accept_all).len(),
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

// Gate review 5 (Claude): under withheld acknowledgements, writing records
// in index order sent the first ones four times before the last ones
// went once.
// @internal
#[test]
fn with_too_few_slots_every_record_goes_once_before_any_goes_twice() {
    let key = [15; 32];
    let text = body(2_000, 10);
    let mut publisher = Publisher::new(key, &text);
    publisher.queue_due(W0);
    let first = publisher.write(W0, 2);
    let second = publisher.write(W0 + 3_600, 2);
    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), W0);
    let mut records = first;
    records.extend(second);
    let bodies = pending.read(&records, W0 + 7_200, accept_all);
    assert_eq!(bodies.len(), 1, "four records in two sessions of two slots");
}

// Gate review 6 (Codex): an 8-record card at 2 content slots a session
// takes about 9 days, and the next publication is due a week after it
// completes, an 18-day cycle. With the 14 days counted from the first
// notice, the first chunks always arrived in the session that expired the
// entry, and the card never installed. Each notice carrying the key now
// restarts the 14 days (owner, 2026-11-23).
// @internal
#[test]
fn a_card_lost_once_installs_when_each_view_renews_its_pending_key() {
    let key = [16; 32];
    let text = body(3_984, 11);
    let mut publisher = Publisher::new(key, &text);
    let mut pending = Pending::default();
    let mut installed_on = None;
    for day in (0..120).step_by(3) {
        let written = session(&mut publisher, W0 + day * DAY, 2);
        let read_on = W0 + (day + 1) * DAY;
        if (day + 1) % 9 == 4 {
            pending.note(&key, &commitment(&key, &text), read_on);
        }
        if day == 0 {
            continue;
        }
        if !pending.read(&written, read_on, accept_all).is_empty() {
            installed_on.get_or_insert(day + 1);
        }
    }
    let day = installed_on.expect("the card never installed");
    assert!(day <= 40, "installed only on day {day}");
}

// Gate review 6 (Kimi, Codex, Claude): a publisher given no slot fixed its
// week anyway, so a publication starved for weeks wrote with a week R no
// longer looks in.
// @internal
#[test]
fn a_publication_takes_its_week_only_when_its_first_record_is_written() {
    let mut publisher = Publisher::new([17; 32], &body(100, 12));
    publisher.queue_due(W0);
    assert!(publisher.write(W0, 0).is_empty());
    assert_eq!(publisher.last_week(), None);
    let later = W0 + 3 * WEEK;
    assert_eq!(publisher.write(later, 1).len(), 1);
    assert_eq!(publisher.last_week(), Some(week(later)));
}

// Gate review 7 (Claude): the full pending list dropped the entry noted
// first, though a view had just renewed it; and a note renewed an entry
// already 14 days old, which section 4 erases.
// @internal
#[test]
fn a_full_pending_list_drops_the_entry_renewed_longest_ago_and_an_expired_one_is_not_renewed() {
    let text = body(50, 13);
    let mut pending = Pending::default();
    for seed in 0..32u8 {
        pending.note(
            &[seed; 32],
            &commitment(&[seed; 32], &text),
            W0 + u64::from(seed),
        );
    }
    pending.note(&[0; 32], &commitment(&[0; 32], &text), W0 + 100);
    pending.note(&[99; 32], &commitment(&[99; 32], &text), W0 + 101);
    let read = |pending: &mut Pending, seed: u8| {
        let mut publisher = Publisher::new([seed; 32], &text);
        let written = session(&mut publisher, W0 + DAY, 8);
        pending.read(&written, W0 + DAY, accept_all).len()
    };
    assert_eq!(read(&mut pending, 0), 1, "renewed, so kept");
    assert_eq!(read(&mut pending, 1), 0, "renewed longest ago, so dropped");

    let mut stale = Pending::default();
    stale.note(&[40; 32], &commitment(&[40; 32], &text), W0);
    assert!(
        stale.note(&[40; 32], &commitment(&[40; 32], &text), W0 + 21 * DAY),
        "a 21-day entry is gone: the key is noted anew"
    );
}

// Gate review 7 (Claude): every group member holds the group's update key,
// so a member could write records under wanted IDs; the first chunk locked
// the body's shape and a failed commitment erased the entry, and the card
// never installed. Record 0 carries the hash of every later chunk and the
// notice's commitment binds record 0, so each record is checked alone
// (owner, 2026-11-23).
// @internal
#[test]
fn records_forged_by_a_group_member_cannot_block_the_card() {
    use vauchi_core::notice::content::seal_record;
    let key = [18; 32];
    let text = body(1_500, 14);
    let mut publisher = Publisher::new(key, &text);
    let honest = session(&mut publisher, W0 + DAY, 8);
    let w = week(W0 + DAY);
    let forged_first = seal_record(&key, b"x", w, 0, 0, &[1; 24]).expect("a member seals");
    let forged_other =
        seal_record(&key, &body(1_500, 99), w, 1, 1, &[2; 24]).expect("a member seals");
    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), W0 + DAY);
    assert!(
        pending
            .read(&[forged_first, forged_other], W0 + 2 * DAY, accept_all)
            .is_empty()
    );
    let bodies = pending.read(&honest, W0 + 2 * DAY, accept_all);
    assert_eq!(bodies.len(), 1, "the honest records still install");
    assert_eq!(bodies[0].as_slice(), text.as_slice());
}

// @internal
#[test]
fn a_later_chunk_read_before_record_zero_in_one_session_still_counts() {
    let key = [19; 32];
    let text = body(1_500, 15);
    let mut publisher = Publisher::new(key, &text);
    let mut honest = session(&mut publisher, W0 + DAY, 8);
    honest.reverse();
    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), W0 + DAY);
    assert_eq!(pending.read(&honest, W0 + 2 * DAY, accept_all).len(), 1);
}

// Gate review 7 (Kimi): a sender away 14 days at a time kept writing an
// open publication under its first week, which R stops looking for after
// two weeks, so an eight-record card never converged.
// @internal
#[test]
fn a_sender_away_two_weeks_at_a_time_still_gets_its_card_across() {
    let key = [20; 32];
    let text = body(3_984, 16);
    let mut publisher = Publisher::new(key, &text);
    let mut pending = Pending::default();
    let mut installed_on = None;
    for fortnight in 0..12u64 {
        let now = W0 + fortnight * 2 * WEEK;
        let written = session(&mut publisher, now, 2);
        // R reads the session's view and content a day later.
        pending.note(&key, &commitment(&key, &text), now + DAY);
        if !pending.read(&written, now + DAY, accept_all).is_empty() {
            installed_on.get_or_insert(fortnight);
        }
    }
    assert!(installed_on.is_some(), "the card never installed");
}
