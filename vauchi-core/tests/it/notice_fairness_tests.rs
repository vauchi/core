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
