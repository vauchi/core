// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One direction of the notice core: a sender and a receiver with a
//! gateway between them that holds no keys (vauchi/private#1281).
//!
//! Each named test is a finding of the red teams' passes on the
//! specification, as the gateway alone can cause it; the property at the
//! end lets the gateway do anything it can in any order.

use proptest::prelude::*;
use vauchi_core::crypto::X25519KeyPair;
use vauchi_core::notice::chain::PERIOD_SECONDS;
use vauchi_core::notice::direction::{Alarm, Receiver, Sender};
use vauchi_core::notice::keys::{Label, seed_epoch};
use vauchi_core::notice::seal::NOTICE_BYTES;

const START: u64 = 1_800_000_000;
const DAY: u64 = 86_400;
const WEEK: u64 = PERIOD_SECONDS;

type Notice = [u8; NOTICE_BYTES];

struct Pair {
    sender: Sender,
    receiver: Receiver,
}

fn pair() -> Pair {
    let sender_device = X25519KeyPair::generate();
    let receiver_device = X25519KeyPair::generate();
    let label = Label::new(sender_device.public_key(), receiver_device.public_key());
    let handshake = X25519KeyPair::generate()
        .diffie_hellman(X25519KeyPair::generate().public_key())
        .expect("contributory");
    let sender_public = *sender_device.public_key();
    let receiver_public = *receiver_device.public_key();
    Pair {
        sender: Sender::new(
            seed_epoch(&handshake, &label),
            label.clone(),
            sender_device,
            receiver_public,
            START,
        ),
        receiver: Receiver::new(
            seed_epoch(&handshake, &label),
            label,
            receiver_device,
            sender_public,
            START,
        ),
    }
}

impl Pair {
    /// The receiver's session at `now`: it reads what the gateway hands
    /// it and writes; the write is acknowledged.
    fn receiver_session(&mut self, deliver: &[Notice], now: u64) -> Vec<Notice> {
        self.receiver.read(deliver, now);
        let written = self.receiver.write(now);
        self.receiver.acknowledge();
        written
    }

    fn sender_session(&mut self, deliver: &[Notice], now: u64) -> Vec<Notice> {
        self.sender.read(deliver, now);
        let written = self.sender.write(now);
        self.sender.acknowledge();
        written
    }

    /// A week in which the gateway delivers everything at once.
    fn faithful_week(&mut self, now: u64) {
        let refreshes = self.receiver_session(&[], now);
        let answers = self.sender_session(&refreshes, now + DAY);
        self.receiver_session(&answers, now + 2 * DAY);
    }

    fn assert_no_alarm(&self) {
        assert_eq!(self.sender.alarms(), &[] as &[Alarm], "sender");
        assert_eq!(self.receiver.alarms(), &[] as &[Alarm], "receiver");
    }

    /// The sender writes in the epoch the receiver's newest refresh names.
    fn assert_healed(&self) {
        assert_eq!(Some(self.sender.writes_in()), self.receiver.newest_name());
    }
}

fn update(byte: u8) -> ([u8; 32], [u8; 31]) {
    ([byte; 32], [byte; 31])
}

// @internal
#[test]
fn an_update_reaches_the_receiver_in_the_seed_epoch() {
    let mut p = pair();
    let (key, commitment) = update(1);
    p.sender.queue_update(&key, &commitment);
    let written = p.sender_session(&[], START);
    assert_eq!(written.len(), 1);
    let read = p.receiver.read(&written, START + DAY);
    assert_eq!(read.len(), 1);
    assert_eq!(*read[0].update_key, key);
    assert_eq!(read[0].commitment, commitment);
    assert_eq!(p.receiver.read(&written, START + DAY).len(), 0);
    p.assert_no_alarm();
}

// @internal
#[test]
fn a_refresh_is_opened_then_named_then_followed() {
    let mut p = pair();
    let seed = p.receiver.current_epoch();

    let refresh = p.receiver_session(&[], START);
    assert_eq!(
        refresh.len(),
        1,
        "one copy: the seed has no epoch before it"
    );
    let opening = p.sender_session(&refresh, START + DAY);
    assert_eq!(opening.len(), 1);
    assert_eq!(p.sender.writes_in(), seed, "followed the name: the seed");

    p.receiver_session(&opening, START + 2 * DAY);
    let first = p.receiver.current_epoch();
    assert_ne!(first, seed, "the receiver moved");
    assert_eq!(p.sender.writes_in(), seed, "not named yet");

    let second_refresh = p.receiver_session(&[], START + WEEK);
    assert_eq!(
        second_refresh.len(),
        2,
        "sealed in the current epoch and the one left"
    );
    p.sender_session(&second_refresh, START + WEEK + DAY);
    assert_eq!(p.sender.writes_in(), first, "followed the name");
    p.assert_no_alarm();
}

// @internal
#[test]
fn the_current_view_goes_out_on_every_followed_refresh() {
    let mut p = pair();
    let (key, commitment) = update(9);
    p.sender.queue_update(&key, &commitment);
    let lost = p.sender_session(&[], START);
    assert_eq!(lost.len(), 1);

    let refresh = p.receiver_session(&[], START + DAY);
    let answer = p.sender_session(&refresh, START + 2 * DAY);
    assert_eq!(answer.len(), 2, "an opening and the view");
    let read = p.receiver.read(&answer, START + 3 * DAY);
    assert_eq!(read.len(), 1);
    assert_eq!(*read[0].update_key, key);
    p.assert_no_alarm();
}

// @internal
#[test]
fn replayed_records_change_nothing() {
    let mut p = pair();
    let refresh = p.receiver_session(&[], START);
    let opening = p.sender_session(&refresh, START + DAY);
    p.receiver_session(&opening, START + 2 * DAY);
    let epoch = p.receiver.current_epoch();
    let writes_in = p.sender.writes_in();

    for _ in 0..3 {
        let resent = p.sender_session(&refresh, START + 3 * DAY);
        p.receiver.read(&opening, START + 3 * DAY);
        p.receiver.read(&resent, START + 3 * DAY);
    }
    assert_eq!(p.receiver.current_epoch(), epoch);
    assert_eq!(p.sender.writes_in(), writes_in);
    p.assert_no_alarm();
}

// Red team, pass 1: four unanswered openings stopped healing for good.
// @internal
#[test]
fn lost_openings_never_stop_healing() {
    let mut p = pair();
    for week in 0..8 {
        let now = START + week * WEEK;
        let refresh = p.receiver_session(&[], now);
        let _lost = p.sender_session(&refresh, now + DAY);
    }
    for week in 8..11 {
        p.faithful_week(START + week * WEEK);
    }
    p.assert_healed();
    assert_ne!(p.sender.writes_in(), seed_of(&p));
    p.assert_no_alarm();
}

fn seed_of(p: &Pair) -> [u8; 16] {
    p.receiver.seed_epoch()
}

// Red team, pass 2: older refreshes delivered after a newer one made the
// sender drop the epoch the receiver then moved into, and the receiver's
// next refresh raised the sender's alarm.
// @internal
#[test]
fn older_refreshes_delivered_after_a_newer_one_raise_no_alarm() {
    let mut p = pair();
    let mut withheld = Vec::new();
    for week in 0..3 {
        withheld.push(p.receiver_session(&[], START + week * WEEK));
    }
    let newest = p.receiver_session(&[], START + 3 * WEEK);
    let opening = p.sender_session(&newest, START + 3 * WEEK + DAY);
    p.receiver_session(&opening, START + 3 * WEEK + 2 * DAY);
    let moved_into = p.receiver.current_epoch();

    let mut late_openings = Vec::new();
    for (index, refresh) in withheld.iter().enumerate() {
        let now = START + 3 * WEEK + (3 + index as u64) * DAY;
        late_openings.extend(p.sender_session(refresh, now));
    }
    p.receiver_session(&late_openings, START + 3 * WEEK + 6 * DAY);
    assert_eq!(p.receiver.current_epoch(), moved_into, "never moves back");

    p.faithful_week(START + 4 * WEEK);
    assert_eq!(p.sender.writes_in(), moved_into);
    p.assert_no_alarm();
}

// Red team, pass 3: the sender erased unnamed epochs three periods after
// opening them while the receiver was already in one and sealed its
// refreshes only there: a silent cut-off for good.
// @internal
#[test]
fn the_receiver_two_epochs_ahead_with_its_refreshes_delayed_is_not_cut_off() {
    let mut p = pair();
    let first = p.receiver_session(&[], START);
    let first_opening = p.sender_session(&first, START + DAY);
    let second = p.receiver_session(&[], START + WEEK);
    let second_opening = p.sender_session(&second, START + WEEK + DAY);

    let both: Vec<Notice> = first_opening.into_iter().chain(second_opening).collect();
    p.receiver_session(&both, START + WEEK + 2 * DAY);
    let two_ahead = p.receiver.current_epoch();

    let mut delayed = Vec::new();
    for week in 2..6 {
        delayed.push(p.receiver_session(&[], START + week * WEEK));
        p.sender_session(&[], START + week * WEEK + DAY);
    }
    for refresh in &delayed {
        p.sender_session(refresh, START + 6 * WEEK);
    }
    assert_eq!(p.sender.writes_in(), two_ahead, "the sender still held it");
    for week in 7..9 {
        p.faithful_week(START + week * WEEK);
    }
    p.assert_healed();
    p.assert_no_alarm();
}

// Red team, pass 3: a second copy of an honest refresh arriving after the
// sender erased the epoch it names compared as another name and alarmed.
// The sealing epoch must still be kept for the copy to be read at all,
// which takes an older refresh delivered late to name it again.
// @internal
#[test]
fn a_late_copy_of_a_refresh_naming_an_erased_epoch_raises_nothing() {
    let mut p = pair();
    p.faithful_week(START);
    let first = p.receiver.current_epoch();

    let second = p.receiver_session(&[], START + WEEK);
    let withheld_opening = p.sender_session(&second, START + WEEK + DAY);
    let late_third = p.receiver_session(&[], START + 2 * WEEK);
    p.receiver_session(&withheld_opening, START + 2 * WEEK + DAY);
    let second_epoch = p.receiver.current_epoch();
    assert_ne!(second_epoch, first);

    let fourth = p.receiver_session(&[], START + 3 * WEEK);
    assert_eq!(
        fourth.len(),
        2,
        "sealed in the second epoch and in the first"
    );
    let late_copy = fourth[1];
    let opening = p.sender_session(&fourth[..1], START + 3 * WEEK + DAY);
    assert_eq!(p.sender.writes_in(), second_epoch);
    p.receiver_session(&opening, START + 3 * WEEK + 2 * DAY);

    p.sender_session(&late_third, START + 4 * WEEK);
    assert_eq!(
        p.sender.writes_in(),
        first,
        "an older refresh moves it back"
    );
    p.faithful_week(START + 4 * WEEK + DAY);
    p.faithful_week(START + 5 * WEEK + DAY);
    p.sender_session(&[], START + 6 * WEEK + 3 * DAY);
    assert!(
        !p.sender.keeps(&second_epoch),
        "three periods after its naming"
    );
    assert!(p.sender.keeps(&first), "named again a week later");

    p.sender_session(&[late_copy], START + 6 * WEEK + 4 * DAY);
    p.assert_no_alarm();
}

// Red team, pass 3: withheld acknowledgements made the sender assign new
// positions every session until the chain ran past the reader's window.
// @internal
#[test]
fn withheld_acknowledgements_do_not_run_the_chain_past_the_window() {
    let mut p = pair();
    let (key, commitment) = update(4);
    p.sender.queue_update(&key, &commitment);
    let mut last = Vec::new();
    for hour in 0..(3 * 7 * 24) {
        let now = START + hour * 3_600;
        p.sender.read(&[], now);
        let written = p.sender.write(now);
        if !written.is_empty() {
            last = written;
        }
    }
    let read = p.receiver.read(&last, START + 3 * WEEK);
    assert_eq!(
        read.len(),
        1,
        "the newest notice is still inside the window"
    );
    assert_eq!(*read[0].update_key, key);
    p.assert_no_alarm();
}

// Red team, pass 3: a clock set back reused a refresh address, which
// links two records for the gateway and alarms the sender.
// @internal
#[test]
fn a_clock_set_back_reuses_no_address_and_raises_no_alarm() {
    let mut p = pair();
    let mut tags = std::collections::BTreeSet::new();
    let mut all = Vec::new();
    let times = [
        START,
        START + DAY,
        START + DAY - 3 * DAY,
        START + 5 * DAY,
        START + 8 * DAY,
        START + 6 * DAY,
        START + 15 * DAY,
    ];
    for now in times {
        p.receiver.read(&[], now);
        for notice in p.receiver.write(now) {
            assert!(
                tags.insert(notice[..16].to_vec()),
                "an address written twice"
            );
            all.push(notice);
        }
    }
    p.sender_session(&all, START + 16 * DAY);
    p.assert_healed();
    p.assert_no_alarm();
}

#[derive(Debug, Clone)]
struct Step {
    days: u64,
    receiver_acts: bool,
    deliver: Vec<(usize, bool)>,
    acknowledged: bool,
    edit: bool,
}

fn step() -> impl Strategy<Value = Step> {
    (
        0u64..5,
        any::<bool>(),
        proptest::collection::vec((0usize..64, any::<bool>()), 0..12),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(days, receiver_acts, deliver, acknowledged, edit)| Step {
            days,
            receiver_acts,
            deliver,
            acknowledged,
            edit,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    // P5: honest use and a gateway that drops, delays, reorders, replays,
    // alters and withholds acknowledgements raise no alarm, and once it
    // delivers again the direction heals and the latest update arrives.
    // @internal
    #[test]
    fn a_gateway_without_keys_raises_no_alarm_and_cannot_stop_convergence(
        steps in proptest::collection::vec(step(), 0..60),
    ) {
        let mut p = pair();
        let mut to_sender: Vec<Notice> = Vec::new();
        let mut to_receiver: Vec<Notice> = Vec::new();
        let mut now = START;
        let mut edits = 0u8;

        for s in steps {
            now += s.days * DAY;
            let (pool, outbox) = if s.receiver_acts {
                (&to_receiver, &mut to_sender)
            } else {
                (&to_sender, &mut to_receiver)
            };
            let mut delivered = Vec::new();
            for (pick, altered) in &s.deliver {
                if pool.is_empty() { break; }
                let mut notice = pool[pick % pool.len()];
                if *altered { notice[20 + pick % 70] ^= 1; }
                delivered.push(notice);
            }
            let written = if s.receiver_acts {
                p.receiver.read(&delivered, now);
                let written = p.receiver.write(now);
                if s.acknowledged { p.receiver.acknowledge(); }
                written
            } else {
                if s.edit {
                    edits = edits.wrapping_add(1);
                    let (key, commitment) = update(edits);
                    p.sender.queue_update(&key, &commitment);
                }
                p.sender.read(&delivered, now);
                let written = p.sender.write(now);
                if s.acknowledged { p.sender.acknowledge(); }
                written
            };
            outbox.extend(written);
            prop_assert_eq!(p.sender.alarms(), &[] as &[Alarm]);
            prop_assert_eq!(p.receiver.alarms(), &[] as &[Alarm]);
        }

        let (key, commitment) = update(200);
        p.sender.queue_update(&key, &commitment);
        let mut latest_arrived = false;
        for _ in 0..4 {
            now += WEEK;
            let refreshes = p.receiver_session(&[], now);
            let answers = p.sender_session(&refreshes, now + DAY);
            latest_arrived |= p
                .receiver
                .read(&answers, now + 2 * DAY)
                .iter()
                .any(|read| *read.update_key == key);
            p.receiver.write(now + 2 * DAY);
            p.receiver.acknowledge();
        }
        prop_assert_eq!(p.sender.alarms(), &[] as &[Alarm]);
        prop_assert_eq!(p.receiver.alarms(), &[] as &[Alarm]);
        prop_assert_eq!(Some(p.sender.writes_in()), p.receiver.newest_name());
        prop_assert!(latest_arrived, "the latest update key never arrived");
    }
}
