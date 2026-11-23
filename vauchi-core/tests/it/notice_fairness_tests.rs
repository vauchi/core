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
        p.sender.write_within(START + 26, 2, 0).len(),
        2,
        "both views, not acknowledged"
    );
    assert_eq!(
        p.sender.write_within(START + 27, 1, 0).len(),
        1,
        "the opening, which the gateway drops"
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
    let view = p.sender.write_within(START + 120, 1, 0);
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
