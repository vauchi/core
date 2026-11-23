// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device's session writes a fixed number of slots, whatever it has to
//! send (protocol section 8; ADR-081, proposed; vauchi/private#1281).
//! What does not fit stays queued and advances no counter.

use crate::common::notice_pair::*;
use vauchi_core::notice::content::RECORD_BYTES;
use vauchi_core::notice::direction::{Wants, week};
use vauchi_core::notice::publish::Publisher;
use vauchi_core::notice::seal::NOTICE_BYTES;
use vauchi_core::notice::session::{BACKGROUND, FAST, Grant, Session, plan};

// @internal
#[test]
fn a_record_not_given_a_slot_stays_queued_and_advances_nothing() {
    let mut p = pair();
    let (key, commitment) = update(1);
    p.sender.queue_update(&key, &commitment);
    assert_eq!(
        p.sender.wants(START),
        Wants {
            update: 1,
            control: 0
        }
    );
    // Were a position assigned to the notice left out, 200 such sessions
    // would pass the cap of 128 positions a period and stop the write below.
    for second in 0..200 {
        assert!(p.sender.write_within(START + second, 0, 0).is_empty());
        p.sender.acknowledge();
    }
    let written = p.sender.write_within(START + 60, 0, 1);
    assert_eq!(written.len(), 1);
    p.sender.acknowledge();
    let read = p.receiver.read(&written, START + DAY);
    assert_eq!(
        read.len(),
        1,
        "read at the first position: none was skipped"
    );
    assert_eq!(*read[0].update_key, key);
    assert_eq!(
        p.sender.wants(START + DAY),
        Wants {
            update: 0,
            control: 0
        }
    );
}

// @internal
#[test]
fn a_refresh_copy_not_given_a_slot_waits_and_spends_no_address() {
    let mut p = pair();
    assert_eq!(
        p.receiver.wants(START),
        1,
        "a refresh is due, sealed in the seed"
    );
    // Were an address spent on the copy left out, five such sessions would
    // use the week's four and stop the write below.
    for second in 0..5 {
        assert!(p.receiver.write_within(START + second, 0).is_empty());
        p.receiver.acknowledge();
    }
    let refresh = p.receiver.write_within(START + 60, 1);
    assert_eq!(refresh.len(), 1);
    p.receiver.acknowledge();
    let answer = p.sender_session(&refresh, START + DAY);
    assert_eq!(answer.len(), 1, "the refresh was read and opened");
    p.assert_no_alarm();
}

// @internal
#[test]
fn updates_come_first_and_every_eighth_slot_goes_to_control() {
    let grants = plan(
        80,
        &[Wants {
            update: 100,
            control: 100,
        }],
    );
    assert_eq!(
        grants,
        vec![Grant {
            update: 70,
            control: 10
        }]
    );
    let grants = plan(
        80,
        &[Wants {
            update: 3,
            control: 100,
        }],
    );
    assert_eq!(
        grants,
        vec![Grant {
            update: 3,
            control: 77
        }],
        "control fills the rest"
    );
    let grants = plan(
        80,
        &[Wants {
            update: 100,
            control: 0,
        }],
    );
    assert_eq!(
        grants,
        vec![Grant {
            update: 80,
            control: 0
        }]
    );
}

// @internal
#[test]
fn directions_take_turns_the_first_listed_first() {
    let wants = [
        Wants {
            update: 100,
            control: 0,
        },
        Wants {
            update: 100,
            control: 0,
        },
    ];
    assert_eq!(
        plan(9, &wants),
        vec![
            Grant {
                update: 5,
                control: 0
            },
            Grant {
                update: 4,
                control: 0
            }
        ]
    );
    let wants = [
        Wants {
            update: 1,
            control: 0,
        },
        Wants {
            update: 100,
            control: 0,
        },
    ];
    assert_eq!(
        plan(9, &wants),
        vec![
            Grant {
                update: 1,
                control: 0
            },
            Grant {
                update: 8,
                control: 0
            }
        ]
    );
}

// @internal
#[test]
fn a_session_writes_the_same_slots_whatever_the_device_has_to_send() {
    for (tier, name) in [(BACKGROUND, "background"), (FAST, "fast")] {
        for updates in [false, true] {
            let mut p = pair();
            let mut publishers = Vec::new();
            if updates {
                let (key, commitment) = update(2);
                p.sender.queue_update(&key, &commitment);
                publishers.push(Publisher::new(key, &[7; 1_500]));
            }
            let mut session = Session::default();
            let written = session.write(
                &mut [&mut p.sender],
                &mut [&mut p.receiver],
                &mut publishers,
                tier,
                START,
            );
            assert_eq!(
                written.notices.len(),
                tier.notices,
                "{name}, updates {updates}"
            );
            assert_eq!(
                written.contents.len(),
                tier.contents,
                "{name}, updates {updates}"
            );
            assert!(
                written
                    .notices
                    .iter()
                    .all(|notice| notice.len() == NOTICE_BYTES)
            );
            assert!(
                written
                    .contents
                    .iter()
                    .all(|record| record.len() == RECORD_BYTES)
            );
        }
    }
}

// @internal
#[test]
fn real_records_take_a_random_place_among_the_dummies() {
    let mut places = std::collections::BTreeSet::new();
    for _ in 0..20 {
        let mut p = pair();
        let mut session = Session::default();
        let written = session.write(&mut [], &mut [&mut p.receiver], &mut [], FAST, START);
        let answered: Vec<usize> = written
            .notices
            .iter()
            .enumerate()
            .filter(|(_, notice)| {
                let mut copy = p.sender.clone();
                copy.read(&[**notice], START + DAY);
                !copy.write(START + DAY).is_empty()
            })
            .map(|(place, _)| place)
            .collect();
        assert_eq!(answered.len(), 1, "one real refresh among the dummies");
        places.insert(answered[0]);
    }
    assert!(
        places.len() > 1,
        "the refresh sat at {places:?} in every session"
    );
}

// Gate review 6 (Claude, Codex): copies cut by a grant were dropped once
// the session was acknowledged, so the copy for the epoch left never went.
// @internal
#[test]
fn a_refresh_copy_cut_by_a_grant_goes_in_a_later_session() {
    let mut p = pair();
    p.faithful_week(START);
    assert_eq!(
        p.receiver.wants(START + WEEK),
        2,
        "the current epoch and the one left"
    );
    let first = p.receiver.write_within(START + WEEK, 1);
    p.receiver.acknowledge();
    assert_eq!(first.len(), 1);
    assert_eq!(
        p.receiver.wants(START + WEEK + 60),
        1,
        "the cut copy still waits"
    );
    let second = p.receiver.write_within(START + WEEK + 60, 1);
    p.receiver.acknowledge();
    assert_eq!(second.len(), 1);
    assert_eq!(p.receiver.wants(START + WEEK + 120), 0);
}

// Gate review 6 (Codex): the sender held one latest key, so an edit of a
// second group took the first group's key out of every later view.
// @internal
#[test]
fn a_view_carries_the_latest_key_of_every_group() {
    let mut p = pair();
    let (a, commitment_a) = update(10);
    p.sender.queue_group_update(1, &a, &commitment_a);
    let lost = p.sender_session(&[], START);
    assert_eq!(lost.len(), 1, "group 1's notice, which the gateway drops");
    let refresh = p.receiver_session(&[], START + DAY);
    let (b, commitment_b) = update(11);
    p.sender.queue_group_update(2, &b, &commitment_b);
    let answer = p.sender_session(&refresh, START + 2 * DAY);
    let keys: Vec<[u8; 32]> = p
        .receiver
        .read(&answer, START + 3 * DAY)
        .iter()
        .map(|read| *read.update_key)
        .collect();
    assert!(keys.contains(&b), "group 2's new edit");
    assert!(keys.contains(&a), "group 1's latest key, in the view");
}

// Gate review 6 (all three): protocol section 4 writes a new edit's notice
// only with record 0 of its publication, so a key never arrives with no
// content in reach.
// @internal
#[test]
fn a_new_edits_notice_waits_for_record_zero_of_its_publication() {
    let mut p = pair();
    let (key, commitment) = update(12);
    p.sender.queue_group_update(3, &key, &commitment);
    let mut busy = Publisher::for_group(4, [13; 32], &[1; 2_000]);
    busy.queue_due(START);
    let mut publishers = vec![busy, Publisher::for_group(3, key, &[2; 100])];
    let mut session = Session::default();
    let first = session.write(&mut [&mut p.sender], &mut [], &mut publishers, FAST, START);
    Session::acknowledge(&mut [&mut p.sender], &mut [], &mut publishers, START);
    assert!(
        p.receiver.read(&first.notices, START + 1).is_empty(),
        "both content slots went to group 4, so group 3's notice waits"
    );
    let mut wrote_notice_with_record = false;
    for minute in 1..6 {
        let now = START + minute * 60;
        let written = session.write(&mut [&mut p.sender], &mut [], &mut publishers, FAST, now);
        Session::acknowledge(&mut [&mut p.sender], &mut [], &mut publishers, now);
        if p.receiver
            .read(&written.notices, now)
            .iter()
            .any(|read| *read.update_key == key)
        {
            wrote_notice_with_record = true;
            break;
        }
    }
    assert!(
        wrote_notice_with_record,
        "the notice went once record 0 had a slot"
    );
}

use proptest::prelude::*;
use vauchi_core::notice::content::commitment;
use vauchi_core::notice::publish::Pending;

/// One step of the device-level property: how many days pass, whose
/// session it is, and whether the gateway drops that session's write.
#[derive(Debug, Clone)]
struct DeviceStep {
    days: u64,
    receiver_acts: bool,
    dropped: bool,
}

fn device_step() -> impl Strategy<Value = DeviceStep> {
    (1u64..4, any::<bool>(), any::<bool>()).prop_map(|(days, receiver_acts, dropped)| DeviceStep {
        days,
        receiver_acts,
        dropped,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // Gate reviews 5 to 6 (Claude, Kimi, Codex): P5 was tested on the
    // directions alone. Here both devices run whole background sessions,
    // the sender publishing and the receiver assembling, while the gateway
    // drops whole sessions; after six faithful weeks the card is installed
    // and nothing alarmed.
    // @internal
    #[test]
    fn whole_device_sessions_install_the_card_after_any_drops(
        steps in proptest::collection::vec(device_step(), 0..30),
        fast in any::<bool>(),
    ) {
        let tier = if fast { FAST } else { BACKGROUND };
        let mut p = pair();
        let key = [21u8; 32];
        let card = vec![5u8; 3_984];
        p.sender.queue_update(&key, &commitment(&key, &card));
        let mut publishers = vec![Publisher::new(key, &card)];
        let mut pending = Pending::default();
        let mut s_session = Session::default();
        let mut r_session = Session::default();
        let mut to_receiver = (Vec::new(), Vec::new());
        let mut to_sender = Vec::new();
        let mut installed = false;
        let mut now = START;
        let faithful = (0..42).map(|i| DeviceStep { days: 1, receiver_acts: i % 2 == 1, dropped: false });
        for step in steps.into_iter().chain(faithful) {
            now += step.days * DAY;
            if step.receiver_acts {
                let (notices, contents): (Vec<Notice>, Vec<_>) = std::mem::take(&mut to_receiver);
                for read in p.receiver.read(&notices, now) {
                    pending.note(&read.update_key, &read.commitment, now);
                }
                installed |= !pending.read(&contents, now, |_| true).is_empty();
                let written = r_session.write(&mut [], &mut [&mut p.receiver], &mut [], tier, now);
                Session::acknowledge(&mut [], &mut [&mut p.receiver], &mut [], now);
                if !step.dropped {
                    to_sender = written.notices;
                }
            } else {
                p.sender.read(&std::mem::take(&mut to_sender), now);
                let written = s_session.write(&mut [&mut p.sender], &mut [], &mut publishers, tier, now);
                Session::acknowledge(&mut [&mut p.sender], &mut [], &mut publishers, now);
                if !step.dropped {
                    to_receiver = (written.notices, written.contents);
                }
            }
        }
        prop_assert_eq!(p.sender.alarms(), &[] as &[vauchi_core::notice::direction::Alarm]);
        prop_assert_eq!(p.receiver.alarms(), &[] as &[vauchi_core::notice::direction::Alarm]);
        prop_assert!(installed, "the card never installed");
    }
}

// Gate review 7 (Codex): an epoch whose four addresses of the week were
// spent unacknowledged dropped out of the refresh's targets, so a copy in
// the newer epoch alone cleared the refresh, and the old epoch never got
// one once the week turned.
// @internal
#[test]
fn a_refresh_waits_for_an_epoch_whose_addresses_of_the_week_are_spent() {
    let mut p = pair();
    let mut copies = Vec::new();
    for second in 0..4 {
        copies.extend(p.receiver.write_within(START + second, 1));
    }
    assert_eq!(
        copies.len(),
        4,
        "the seed's four addresses this week, none acknowledged"
    );
    let opening = p.sender_session(&copies[..1], START + DAY);
    p.receiver.read(&opening, START + 2 * DAY);
    assert_ne!(p.receiver.current_epoch(), p.receiver.seed_epoch(), "moved");
    assert_eq!(
        p.receiver.write_within(START + 2 * DAY, 2).len(),
        1,
        "the new epoch only"
    );
    p.receiver.acknowledge();
    let next_week = (week(START) as u64 + 1) * WEEK + 60;
    assert!(
        next_week < START + WEEK,
        "the week turns before refresh 2 is due"
    );
    assert_eq!(
        p.receiver.wants(next_week),
        1,
        "the seed still lacks an acknowledged copy"
    );
    assert_eq!(p.receiver.write_within(next_week, 1).len(), 1);
}

// Gate review 7 (Codex): the sender's demand counted every eligible group
// once any position was left in the period.
// @internal
#[test]
fn the_senders_demand_never_exceeds_the_positions_left_in_the_period() {
    let mut p = pair();
    let (a, commitment) = update(30);
    p.sender.queue_group_update(1, &a, &commitment);
    for second in 0..127 {
        assert_eq!(p.sender.write_within(START + second, 0, 1).len(), 1);
    }
    let (b, commitment) = update(31);
    p.sender.queue_group_update(2, &b, &commitment);
    assert_eq!(
        p.sender.wants(START + 200).update,
        1,
        "one position is left"
    );
}
