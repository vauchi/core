// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Saved state of the notice core (protocol section 5): what a device
//! keeps between sessions, and that loading it first erases what expired
//! (P2, owner 2026-11-23; vauchi/private#1281).

use crate::common::notice_pair::*;
use vauchi_core::notice::content::commitment;
use vauchi_core::notice::direction::{Receiver, Sender};
use vauchi_core::notice::publish::{Pending, Publisher};
use vauchi_core::notice::saved::SavedError;

fn reload_receiver(p: &mut Pair, now: u64) {
    let saved = p.receiver.save();
    p.receiver = Receiver::load(&saved, now).expect("loads");
}

fn reload_sender(p: &mut Pair, now: u64) {
    let saved = p.sender.save();
    p.sender = Sender::load(&saved, now).expect("loads");
}

// @internal
#[test]
fn both_devices_saved_and_loaded_at_every_session_heal_and_read_updates() {
    let mut p = pair();
    let mut now = START;
    for week in 0..4u8 {
        reload_receiver(&mut p, now);
        let refreshes = p.receiver_session(&[], now);
        reload_sender(&mut p, now + DAY);
        let (key, commitment) = update(week);
        p.sender.queue_update(&key, &commitment);
        let answers = p.sender_session(&refreshes, now + DAY);
        reload_receiver(&mut p, now + 2 * DAY);
        let read = p.receiver.read(&answers, now + 2 * DAY);
        assert!(
            read.iter().any(|read| *read.update_key == key),
            "week {week}"
        );
        now += WEEK;
    }
    p.assert_healed();
    assert_ne!(p.sender.writes_in(), p.receiver.seed_epoch());
    p.assert_no_alarm();
}

// @internal
#[test]
fn loading_saved_state_first_erases_what_expired() {
    let mut p = pair();
    let (lost, commitment) = update(1);
    p.sender.queue_update(&lost, &commitment);
    let dropped = p.sender_session(&[], START);
    let (newer, commitment) = update(2);
    p.sender.queue_update(&newer, &commitment);
    let delivered = p.sender_session(&[], START + DAY);
    p.receiver.read(&delivered, START + 2 * DAY);
    let saved = p.receiver.save();

    let inside = Receiver::load(&saved, START + 2 * DAY + WEEK).expect("loads");
    assert_eq!(
        inside.clone().read(&dropped, START + 2 * DAY + WEEK).len(),
        1,
        "loaded inside the window, the skipped position still reads"
    );
    // Off for four weeks: loading erases the skipped key before anything
    // reads, so even on a clock set back into the window it reads nothing.
    let after = Receiver::load(&saved, START + 2 * DAY + 4 * WEEK).expect("loads");
    assert_eq!(
        after.clone().read(&dropped, START + 2 * DAY + WEEK).len(),
        0
    );
}

// @internal
#[test]
fn a_publisher_and_pending_keys_carry_on_after_loading() {
    let key = [3; 32];
    let text = vec![9u8; 1_200];
    let mut publisher = Publisher::new(key, &text);
    publisher.queue_due(START);
    let first = publisher.write(START, 1);
    publisher.acknowledge(START);
    let mut publisher = Publisher::load(&publisher.save()).expect("loads");
    let rest = publisher.write(START + 60, 8);
    publisher.acknowledge(START + 60);
    assert_eq!(
        (first.len(), rest.len()),
        (1, 2),
        "three records, none again"
    );

    let mut pending = Pending::default();
    pending.note(&key, &commitment(&key, &text), START);
    pending.read(&first, START, |_| true);
    let mut pending = Pending::load(&pending.save(), START + DAY).expect("loads");
    let bodies = pending.read(&rest, START + DAY, |_| true);
    assert_eq!(
        bodies.len(),
        1,
        "the chunk held before saving completes the body"
    );
    assert_eq!(bodies[0].as_slice(), text.as_slice());

    let mut stale = Pending::default();
    stale.note(&key, &commitment(&key, &text), START);
    let mut stale = Pending::load(&stale.save(), START + 14 * DAY).expect("loads");
    assert!(
        stale.note(&key, &commitment(&key, &text), START + 14 * DAY),
        "the 14-day entry was erased on load"
    );
}

// @internal
#[test]
fn malformed_saved_state_is_refused() {
    let p = pair();
    let saved = p.receiver.save();
    let mut wrong_version = saved.to_vec();
    wrong_version[0] ^= 0xFF;
    let mut trailing = saved.to_vec();
    trailing.push(0);
    let cases: [(&str, &[u8]); 4] = [
        ("empty", &[]),
        ("truncated", &saved[..saved.len() / 2]),
        ("another version", &wrong_version),
        ("trailing bytes", &trailing),
    ];
    for (what, bytes) in cases {
        assert_eq!(
            Receiver::load(bytes, START).err(),
            Some(SavedError),
            "{what}"
        );
    }
    assert_eq!(
        Sender::load(&saved, START).err(),
        Some(SavedError),
        "a receiver's state is not a sender's"
    );
}
