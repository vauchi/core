// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a direction writes when its control slots are scarce: every record
//! waits a finite time (protocol section 8; gate review 12).

use crate::common::notice_pair::*;

// Gate review 12 (Codex): a never-written opening ranked ahead of every view
// that had gone out once, and each refresh brought a fresh opening, so with
// one control turn a refresh cycle a view lost once never went again.
// @internal
#[test]
fn a_fresh_opening_each_refresh_does_not_starve_a_lost_view() {
    let mut p = pair();
    let (key, commitment) = update(80);
    p.sender
        .queue_group_update(2, &key, &commitment)
        .expect("within 32 groups");
    assert_eq!(
        p.sender.write_within(START, 0, 1).len(),
        1,
        "the notice the gateway drops"
    );
    p.sender.acknowledge();
    let mut now = START;
    let mut to_receiver = Vec::new();
    let mut read = false;
    for _ in 0..6 {
        now += WEEK;
        read |= p
            .receiver
            .read(&to_receiver, now)
            .iter()
            .any(|r| *r.update_key == key);
        let refresh = p.receiver.write(now);
        p.receiver.acknowledge();
        p.sender.read(&refresh, now + 60);
        to_receiver = p.sender.write_within(now + 120, 1, 0);
        p.sender.acknowledge();
    }
    read |= p
        .receiver
        .read(&to_receiver, now + DAY)
        .iter()
        .any(|r| *r.update_key == key);
    assert!(read, "each control turn went to a fresh opening");
}

// Gate review 12 (Codex, Claude): with one position left, the request
// counted the update that takes it and no view, but the write let a view
// compete for the control slot; the view ranked before the opening, the
// update took the position, and the slot granted for the opening went to a
// dummy.
// @internal
#[test]
fn the_views_that_compete_for_control_are_the_ones_the_request_counted() {
    let mut p = pair();
    let refresh = p.receiver_session(&[], START);
    for (group, byte, at) in [(1, 81u8, 10u64), (2, 82, 20)] {
        let (key, commitment) = update(byte);
        p.sender
            .queue_group_update(group, &key, &commitment)
            .expect("within 32 groups");
        assert_eq!(p.sender.write_within(START + at, 0, 1).len(), 1);
        p.sender.acknowledge();
    }
    p.sender.read(&refresh, START + 25);
    assert_eq!(
        p.sender.write_within(START + 26, 3, 0).len(),
        3,
        "the opening, which the gateway drops, and both views, not acknowledged"
    );
    p.sender.acknowledge();
    for j in 0..123u64 {
        let (key, commitment) = update(100 + (j % 100) as u8);
        p.sender
            .queue_group_update(3, &key, &commitment)
            .expect("within 32 groups");
        assert_eq!(p.sender.write_within(START + 30 + j * 60, 0, 1).len(), 1);
        p.sender.acknowledge();
    }
    let (key, commitment) = update(90);
    p.sender
        .queue_group_update(1, &key, &commitment)
        .expect("within 32 groups");
    let now = START + 30 + 124 * 60;
    let wants = p.sender.wants(now);
    assert_eq!(
        (wants.update, wants.control),
        (1, 1),
        "the last position, and the opening"
    );
    assert_eq!(
        p.sender
            .write_within(now, wants.control, wants.update)
            .len(),
        2,
        "the update and the opening"
    );
}

// Gate review 12 (Codex, Claude): the times the queues are ordered by were
// not pulled back to the clock, as every stored time is (protocol section
// 1), so a group written while the clock was ahead stayed behind every
// group written after the clock was set back, for up to three days.
// @internal
#[test]
fn a_group_written_with_the_clock_ahead_is_not_kept_behind_once_it_is_set_back() {
    let mut p = pair();
    let refresh = p.receiver_session(&[], START);
    let (a, commitment_a) = update(91);
    p.sender
        .queue_group_update(1, &a, &commitment_a)
        .expect("within 32 groups");
    assert_eq!(p.sender.write_within(START + 3 * DAY, 0, 1).len(), 1);
    p.sender.acknowledge();
    let (b, commitment_b) = update(92);
    p.sender
        .queue_group_update(2, &b, &commitment_b)
        .expect("within 32 groups");
    assert_eq!(p.sender.write_within(START + 60, 0, 1).len(), 1);
    p.sender.acknowledge();
    p.sender.read(&refresh, START + 90);
    // Two control slots: the opening and one view (owner, 2026-11-23).
    let view = p.sender.write_within(START + 120, 2, 0);
    let read: Vec<[u8; 32]> = p
        .receiver
        .read(&view, START + 180)
        .iter()
        .map(|r| *r.update_key)
        .collect();
    assert_eq!(read, vec![a], "group 1's view, pulled back to the clock");
}

// @internal
#[test]
fn a_turn_recorded_with_the_clock_ahead_is_pulled_back() {
    use vauchi_core::notice::direction::Wants;
    use vauchi_core::notice::session::{Grant, Session};
    let control = Grant {
        update: 0,
        control: 1,
    };
    let mut session = Session::default();
    session.record_turns(&[control, Grant::default()], START + 3 * DAY);
    let wants = [Wants {
        update: 0,
        control: 1,
    }; 2];
    session.plan_turns(0, &wants, START);
    session.record_turns(&[Grant::default(), control], START + 60);
    let grants = session.plan_turns(1, &wants, START + 120);
    assert_eq!(
        (grants[0].control, grants[1].control),
        (1, 0),
        "direction 0's turn is pulled back to the set-back clock"
    );
}

// Gate review 12 (Claude): a new edit's notice was eligible once any
// publisher of its group had written record 0, so with the old edit's
// publisher still held the notice went before its own record 0.
// @internal
#[test]
fn a_new_edits_notice_waits_for_its_own_record_zero_not_the_old_edits() {
    use vauchi_core::notice::publish::Publisher;
    use vauchi_core::notice::session::{FAST, Session};
    let mut p = pair();
    let (old, commitment_old) = update(93);
    p.sender
        .queue_group_update(3, &old, &commitment_old)
        .expect("within 32 groups");
    let mut publishers = vec![Publisher::for_group(3, old, &[3; 100])];
    let mut session = Session::default();
    session.write(&mut [&mut p.sender], &mut [], &mut publishers, FAST, START);
    Session::acknowledge(&mut [&mut p.sender], &mut [], &mut publishers, START);
    assert!(publishers[0].first_record_written());
    let (new, commitment_new) = update(94);
    p.sender
        .queue_group_update(3, &new, &commitment_new)
        .expect("within 32 groups");
    let mut busy = Publisher::for_group(4, [95; 32], &[1; 2_000]);
    busy.queue_due(START);
    publishers.insert(0, busy);
    publishers.push(Publisher::for_group(3, new, &[4; 100]));
    let written = session.write(
        &mut [&mut p.sender],
        &mut [],
        &mut publishers,
        FAST,
        START + 60,
    );
    assert!(
        !publishers[2].first_record_written(),
        "both content slots went to group 4"
    );
    let read: Vec<[u8; 32]> = p
        .receiver
        .read(&written.notices, START + 120)
        .iter()
        .map(|r| *r.update_key)
        .collect();
    assert!(!read.contains(&new), "the new key went before its record 0");
}

// Owner, 2026-11-23, after gates 10 to 12 each found a stall in how
// openings and views shared a direction's control slots: a direction holds
// at most one opening, the one for the highest refresh number, so openings
// cannot outnumber the turns that serve them.
// @internal
#[test]
fn a_direction_holds_one_opening_for_its_highest_refresh() {
    for later_first in [false, true] {
        let mut p = pair();
        let seed = p.receiver.current_epoch();
        let first = p.receiver_session(&[], START);
        let second = p.receiver_session(&[], START + WEEK);
        let (early, late) = if later_first {
            (second, first)
        } else {
            (first, second)
        };
        p.sender.read(&early, START + WEEK + 60);
        p.sender.read(&late, START + WEEK + 120);
        let now = START + WEEK + 180;
        assert_eq!(p.sender.wants(now).control, 1, "later first: {later_first}");
        let opening = p.sender.write_within(now, 1, 0);
        p.receiver.read(&opening, now + 60);
        assert_ne!(
            p.receiver.current_epoch(),
            seed,
            "later first: {later_first}"
        );
        // R accepts either; the one held is refresh 2's (gate review 14).
        assert_eq!(p.receiver.current_number(), 2, "later first: {later_first}");
    }
}

// Owner, 2026-11-23: a direction's control turns alternate between its
// opening and its views, so neither kind waits more than one turn for the
// other; before, six views written earlier went before the opening.
// @internal
#[test]
fn views_do_not_hold_an_opening_back_more_than_one_turn() {
    let mut p = pair();
    let seed = p.receiver.current_epoch();
    let refresh = p.receiver_session(&[], START);
    for group in 1..=6u32 {
        let (key, commitment) = update(100 + group as u8);
        p.sender
            .queue_group_update(group, &key, &commitment)
            .expect("within 32 groups");
        p.sender.write_within(START + u64::from(group), 0, 1);
        p.sender.acknowledge();
    }
    p.sender.read(&refresh, START + 100);
    for turn in 0..2u64 {
        let written = p.sender.write_within(START + 200 + turn * DAY, 1, 0);
        p.sender.acknowledge();
        p.receiver.read(&written, START + 300 + turn * DAY);
    }
    assert_ne!(
        p.receiver.current_epoch(),
        seed,
        "the opening had one of two turns"
    );
}

// Gate review 13 (Claude): after the clock was set back, the request
// counted the period's positions by age but the write by how many were
// still held, so positions assigned after the set-back and already out of
// the period still blocked the write, and the control slots the request
// asked for went to dummies, for up to the set-back.
// @internal
#[test]
fn the_write_counts_the_periods_positions_as_the_request_does_after_a_set_back() {
    let mut p = pair();
    let ahead = START + 3 * DAY;
    let set_back = START + 3_600;
    for (base, count) in [(ahead, 64u64), (set_back, 64)] {
        for j in 0..count {
            let group = 1 + u32::try_from(j % 2).expect("small");
            let (key, commitment) = update(1 + (j % 200) as u8);
            p.sender
                .queue_group_update(group, &key, &commitment)
                .expect("within 32 groups");
            assert_eq!(p.sender.write_within(base + j, 0, 1).len(), 1);
            p.sender.acknowledge();
        }
    }
    let now = set_back + WEEK + 100;
    let refresh = p.receiver_session(&[], now);
    p.sender.read(&refresh, now);
    let wants = p.sender.wants(now);
    assert_eq!(
        (wants.update, wants.control),
        (0, 3),
        "the opening and both views, 63 positions in the period"
    );
    assert_eq!(
        p.sender
            .write_within(now, wants.control, wants.update)
            .len(),
        3,
        "the write takes what the request counted"
    );
}

// Gate review 14 (Claude): the highest refresh number read only ever rose,
// so one refresh a copy forged with the highest number made every later
// refresh of R's open nothing: healing ended for good, with no alarm.
// A lower number may wait only while the higher number's opening is queued.
// @internal
#[test]
fn a_forged_refresh_with_the_highest_number_does_not_end_healing() {
    let mut p = pair();
    let seed = p.receiver.seed_epoch();
    let forged = forged_refresh(&p.copied_seed, START, 3, u32::MAX, [7; 32], seed);
    p.sender_session(&[forged], START);
    let mut to_receiver = Vec::new();
    for week in 1..=4u64 {
        let now = START + week * WEEK;
        let refresh = p.receiver_session(&to_receiver, now);
        to_receiver = p.sender_session(&refresh, now + DAY);
    }
    p.receiver_session(&to_receiver, START + 5 * WEEK);
    assert_ne!(
        p.receiver.current_epoch(),
        seed,
        "R moved on its own refresh"
    );
    p.assert_no_alarm();
}
