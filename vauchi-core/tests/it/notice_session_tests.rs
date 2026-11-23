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
    p.sender
        .queue_group_update(1, &a, &commitment_a)
        .expect("within 32 groups");
    let lost = p.sender_session(&[], START);
    assert_eq!(lost.len(), 1, "group 1's notice, which the gateway drops");
    let refresh = p.receiver_session(&[], START + DAY);
    let (b, commitment_b) = update(11);
    p.sender
        .queue_group_update(2, &b, &commitment_b)
        .expect("within 32 groups");
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
    p.sender
        .queue_group_update(3, &key, &commitment)
        .expect("within 32 groups");
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
    /// Whether the relay's acknowledgement reaches the device, apart from
    /// whether the write reaches the other (gate review 8, Kimi).
    acknowledged: bool,
}

fn device_step() -> impl Strategy<Value = DeviceStep> {
    (1u64..4, any::<bool>(), any::<bool>(), any::<bool>()).prop_map(
        |(days, receiver_acts, dropped, acknowledged)| DeviceStep {
            days,
            receiver_acts,
            dropped,
            acknowledged,
        },
    )
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
        let faithful = (0..42).map(|i| DeviceStep { days: 1, receiver_acts: i % 2 == 1, dropped: false, acknowledged: true });
        for step in steps.into_iter().chain(faithful) {
            now += step.days * DAY;
            if step.receiver_acts {
                let (notices, contents): (Vec<Notice>, Vec<_>) = std::mem::take(&mut to_receiver);
                for read in p.receiver.read(&notices, now) {
                    pending.note(&read.update_key, &read.commitment, now);
                }
                installed |= !pending.read(&contents, now, |_| true).is_empty();
                let written = r_session.write(&mut [], &mut [&mut p.receiver], &mut [], tier, now);
                if step.acknowledged {
                    Session::acknowledge(&mut [], &mut [&mut p.receiver], &mut [], now);
                }
                if !step.dropped {
                    to_sender = written.notices;
                }
            } else {
                p.sender.read(&std::mem::take(&mut to_sender), now);
                let written = s_session.write(&mut [&mut p.sender], &mut [], &mut publishers, tier, now);
                if step.acknowledged {
                    Session::acknowledge(&mut [&mut p.sender], &mut [], &mut publishers, now);
                }
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
    p.sender
        .queue_group_update(1, &a, &commitment)
        .expect("within 32 groups");
    for second in 0..127 {
        assert_eq!(p.sender.write_within(START + second, 0, 1).len(), 1);
    }
    let (b, commitment) = update(31);
    p.sender
        .queue_group_update(2, &b, &commitment)
        .expect("within 32 groups");
    assert_eq!(
        p.sender.wants(START + 200).update,
        1,
        "one position is left"
    );
}

// Gate review 7 (Kimi): R5 makes the epoch of an alarm a refresh target at
// once, but an acknowledged refresh stayed dequeued until the next one was
// due a week later.
// @internal
#[test]
fn an_alarm_queues_the_refresh_again_for_the_epochs_it_now_needs() {
    let mut p = pair();
    let refresh = p.receiver_session(&[], START);
    let honest = p.sender_session(&refresh, START + DAY);
    p.receiver.read(&honest, START + 2 * DAY);
    assert_eq!(
        p.receiver.wants(START + 2 * DAY),
        0,
        "acknowledged, nothing due"
    );
    let forged = forged_opening(&p.copied_seed, 2, 1, [7; 8], [0; 16]);
    p.receiver.read(&[forged], START + 3 * DAY);
    assert_eq!(p.receiver.alarms().len(), 1);
    assert!(
        p.receiver.wants(START + 3 * DAY) > 0,
        "the alarm queues the refresh again"
    );
}

// Gate review 8 (Codex, Kimi): each followed refresh made every view due
// again and views went in group order, so when a session had room for
// fewer views than there were groups, the last group's view never went and
// a lost notice of it was never replaced. With at most 32 groups a contact
// (gate review 9), the same holds whenever a session's control slots hold
// fewer views than groups. Notices now go the group written longest ago
// first.
// @internal
#[test]
fn with_more_groups_than_slots_every_groups_view_still_goes() {
    let mut p = pair();
    for group in 0..32u32 {
        let (key, commitment) = update(group as u8);
        p.sender
            .queue_group_update(group, &key, &commitment)
            .expect("within 32 groups");
    }
    let mut now = START;
    p.sender_session(&[], now);
    now += DAY;
    let (lost, commitment) = update(250);
    p.sender
        .queue_group_update(31, &lost, &commitment)
        .expect("within 32 groups");
    assert_eq!(
        p.sender.write_within(now, 0, 1).len(),
        1,
        "group 31's edit, which the gateway drops"
    );
    p.sender.acknowledge();
    let mut read_lost = false;
    for _ in 0..6 {
        now += WEEK;
        let refresh = p.receiver_session(&[], now);
        p.sender.read(&refresh, now);
        let written = p.sender.write_within(now, 31, 0);
        p.sender.acknowledge();
        read_lost |= p
            .receiver
            .read(&written, now + 1)
            .iter()
            .any(|read| *read.update_key == lost);
        p.receiver_session(&[], now + 2);
    }
    assert!(read_lost, "group 31's view never went");
}

// Gate review 9 (Codex): updates took the period's positions before any
// view, so a group edited at the full rate starved another group's view
// for good. Updates and views now share one queue, the group written
// longest ago first.
// @internal
#[test]
fn a_group_edited_at_the_full_rate_does_not_starve_another_groups_view() {
    let step = WEEK / 128;
    let mut p = pair();
    let refresh = p.receiver_session(&[], START);
    let (b, commitment_b) = update(40);
    p.sender
        .queue_group_update(2, &b, &commitment_b)
        .expect("within 32 groups");
    let lost = p.sender.write_within(START, 0, 1);
    assert_eq!(lost.len(), 1, "B's notice, which the gateway drops");
    p.sender.acknowledge();
    for j in 1..128u64 {
        let (a, commitment_a) = update((j % 200) as u8);
        p.sender
            .queue_group_update(1, &a, &commitment_a)
            .expect("within 32 groups");
        assert_eq!(p.sender.write_within(START + j * step, 0, 1).len(), 1);
        p.sender.acknowledge();
    }
    // A week on, the refresh is followed and B's view is due; A edits again.
    let now = START + WEEK + 1;
    p.sender.read(&refresh, now);
    let (a, commitment_a) = update(250);
    p.sender
        .queue_group_update(1, &a, &commitment_a)
        .expect("within 32 groups");
    let written = p.sender.write_within(now, usize::MAX, usize::MAX);
    let keys: Vec<[u8; 32]> = p
        .receiver
        .read(&written, now + 1)
        .iter()
        .map(|read| *read.update_key)
        .collect();
    assert!(keys.contains(&b), "B's view takes the one position left");
}

// Gate review 9 (Kimi): the order counted only acknowledged writes, so a
// gateway that delivered everything but withheld every acknowledgement
// kept the same groups first in every session too small for all of them,
// and the last group's key never went.
// @internal
#[test]
fn withheld_acknowledgements_do_not_starve_a_group() {
    let mut p = pair();
    for group in 0..32u32 {
        let (key, commitment) = update(group as u8);
        p.sender
            .queue_group_update(group, &key, &commitment)
            .expect("within 32 groups");
    }
    let last = update(31).0;
    let mut read_last = false;
    for session in 0..6u64 {
        let now = START + session * WEEK;
        let written = p.sender.write_within(now, 0, 31);
        read_last |= p
            .receiver
            .read(&written, now + 1)
            .iter()
            .any(|read| *read.update_key == last);
    }
    assert!(read_last, "group 31's key never went");
}

// Gate review 9 (Claude): a receiver holds 32 pending keys and remembers 32
// installed, so a contact in more than 32 groups saw keys push each other
// out. Owner, 2026-11-23: a contact is in at most 32 groups.
// @internal
#[test]
fn a_contact_is_in_at_most_32_groups() {
    use vauchi_core::notice::direction::{GROUPS_A_CONTACT, TooManyGroups};
    let mut p = pair();
    for group in 0..GROUPS_A_CONTACT as u32 {
        let (key, commitment) = update(group as u8);
        assert_eq!(
            p.sender.queue_group_update(group, &key, &commitment),
            Ok(())
        );
    }
    let (key, commitment) = update(99);
    assert_eq!(
        p.sender
            .queue_group_update(GROUPS_A_CONTACT as u32, &key, &commitment),
        Err(TooManyGroups)
    );
    assert_eq!(
        p.sender.queue_group_update(0, &key, &commitment),
        Ok(()),
        "an existing group takes a new edit"
    );
    assert_eq!(GROUPS_A_CONTACT, 32);
}

// Gate review 10 (Codex): a direction's one control slot always went to its
// pending opening first, so with refreshes arriving each week a lost key
// notice's view never went. Owner, 2026-11-23: every queue serves what has
// waited longest, openings and views alike.
// @internal
#[test]
fn an_opening_always_pending_does_not_starve_a_view() {
    let mut p = pair();
    let (key, commitment) = update(60);
    p.sender.queue_update(&key, &commitment);
    assert_eq!(
        p.sender.write_within(START, 0, 1).len(),
        1,
        "the notice the gateway drops"
    );
    p.sender.acknowledge();
    let mut now = START;
    let mut read = false;
    for _ in 0..6 {
        now += WEEK;
        let refresh = p.receiver_session(&[], now);
        p.sender.read(&refresh, now + DAY);
        for day in 1..4u64 {
            let written = p.sender.write_within(now + day * DAY, 1, 0);
            p.sender.acknowledge();
            read |= p
                .receiver
                .read(&written, now + day * DAY + 1)
                .iter()
                .any(|r| *r.update_key == key);
        }
    }
    assert!(read, "the view never had the control slot");
}

// Gate review 10 (Codex): new edits always took the content slots before
// another group's weekly republication, so that group's lost card body was
// never replaced while the editing went on.
// @internal
#[test]
fn constant_new_edits_do_not_starve_a_weekly_republication() {
    use vauchi_core::notice::content::commitment;
    use vauchi_core::notice::publish::Pending;
    let b_key = [61; 32];
    let b_body = vec![7u8; 400];
    let mut publishers = vec![Publisher::for_group(2, b_key, &b_body)];
    let mut session = Session::default();
    let mut now = START;
    session.write(&mut [], &mut [], &mut publishers, BACKGROUND, now);
    Session::acknowledge(&mut [], &mut [], &mut publishers, now);
    let mut pending = Pending::default();
    pending.note(&b_key, &commitment(&b_key, &b_body), now);
    let mut installed = false;
    for edit in 0..20u8 {
        now += 3 * DAY;
        publishers.retain(|publisher| publisher.group() != 1);
        publishers.insert(0, Publisher::for_group(1, [edit; 32], &vec![edit; 2_400]));
        let written = session.write(&mut [], &mut [], &mut publishers, BACKGROUND, now);
        Session::acknowledge(&mut [], &mut [], &mut publishers, now);
        pending.note(&b_key, &commitment(&b_key, &b_body), now);
        installed |= !pending
            .read(&written.contents, now + 1, |_| true)
            .is_empty();
    }
    assert!(
        installed,
        "group 2's republication never had a content slot"
    );
}

// Gate review 10 (Claude): a contact could not leave a group, so the cap
// counted every group it was ever in, and views kept sending a left
// group's key. Leaving erases that group's key from the direction.
// @internal
#[test]
fn a_contact_leaving_a_group_frees_its_place_and_its_key_goes() {
    use vauchi_core::notice::direction::GROUPS_A_CONTACT;
    let mut p = pair();
    for group in 0..GROUPS_A_CONTACT as u32 {
        let (key, commitment) = update(group as u8);
        p.sender
            .queue_group_update(group, &key, &commitment)
            .expect("within 32 groups");
    }
    p.sender_session(&[], START);
    let left = update(5).0;
    p.sender.leave_group(5);
    let (key, commitment) = update(200);
    assert_eq!(
        p.sender.queue_group_update(99, &key, &commitment),
        Ok(()),
        "its place is free"
    );
    let refresh = p.receiver_session(&[], START + DAY);
    let written = p.sender_session(&refresh, START + 2 * DAY);
    let keys: Vec<[u8; 32]> = p
        .receiver
        .read(&written, START + 3 * DAY)
        .iter()
        .map(|read| *read.update_key)
        .collect();
    assert!(
        keys.contains(&update(6).0),
        "the views of the groups it is in still go"
    );
    assert!(!keys.contains(&left), "the left group's key goes no more");
}

// Gate review 11 (Codex): one "last served" time per direction counted an
// update as service of its control queue, so with acknowledgements withheld
// the same directions got the control slots every session and the others'
// openings and views never went. Each queue now keeps its own turn order.
// @internal
#[test]
fn updates_do_not_count_as_a_directions_control_turn() {
    let wants = |update, control| Wants { update, control };
    // 16 directions, each with five updates and one control record: 70
    // update slots and 10 control slots a session.
    let all: Vec<Wants> = (0..16).map(|_| wants(5, 1)).collect();
    let mut served = vec![0usize; 16];
    let mut session = Session::default();
    for _ in 0..4 {
        let grants = session.plan_turns(80, &all, START);
        for (i, grant) in grants.iter().enumerate() {
            served[i] += grant.control;
        }
        session.record_turns(&grants, START);
    }
    assert!(
        served.iter().all(|count| *count >= 1),
        "control turns {served:?}"
    );
}

// Gate review 11 (Claude): with the period's positions spent, a view the
// cap refuses still counted against an opening's control slot, so a grant
// sized for the opening wrote nothing.
// @internal
#[test]
fn a_view_the_cap_refuses_does_not_take_an_openings_slot() {
    let step = WEEK / 128;
    let mut p = pair();
    let refresh = p.receiver_session(&[], START);
    let (b, commitment_b) = update(70);
    p.sender
        .queue_group_update(2, &b, &commitment_b)
        .expect("within 32 groups");
    p.sender.write_within(START, 0, 1);
    p.sender.acknowledge();
    for j in 1..128u64 {
        let (a, commitment_a) = update((j % 200) as u8);
        p.sender
            .queue_group_update(1, &a, &commitment_a)
            .expect("within 32 groups");
        p.sender.write_within(START + j * step, 0, 1);
        p.sender.acknowledge();
    }
    let now = START + 127 * step + 60;
    p.sender.read(&refresh, now);
    let first = p.sender.write_within(now, p.sender.wants(now).control, 0);
    p.sender.acknowledge();
    assert_eq!(first.len(), 1, "the opening, which the gateway drops");
    let later = now + 60;
    let control = p.sender.wants(later).control;
    assert!(control >= 1);
    assert!(
        !p.sender.write_within(later, control, 0).is_empty(),
        "the grant sized for the opening writes it"
    );
}

// Gate review 11 (Codex, Kimi): the number of openings followed waiting
// time, but the ones written were always the newest, so an older opening
// waited through all four attempts of a newer one.
// @internal
#[test]
fn the_opening_that_waited_longest_is_the_one_written() {
    use vauchi_core::notice::direction::Receiver;
    let mut p = pair();
    let first = p.receiver_session(&[], START);
    p.sender.read(&first, START + 60);
    let older = p.sender.write_within(START + 120, 1, 0);
    p.sender.acknowledge();
    assert_eq!(older.len(), 1, "the first opening, which the gateway drops");
    let second = p.receiver_session(&[], START + WEEK);
    p.sender.read(&second, START + WEEK + 60);
    let newer = p.sender.write_within(START + WEEK + 120, 1, 0);
    p.sender.acknowledge();
    let again = p.sender.write_within(START + WEEK + DAY, 1, 0);
    p.sender.acknowledge();
    assert_eq!((newer.len(), again.len()), (1, 1));
    let now = START + WEEK + 2 * DAY;
    let saved = p.receiver.save().expect("saves");
    let mut opened_by_newer = Receiver::load(&saved, now).expect("loads");
    let mut opened_by_again = Receiver::load(&saved, now).expect("loads");
    opened_by_newer.read(&newer, now);
    opened_by_again.read(&again, now);
    assert_ne!(opened_by_newer.current_epoch(), p.receiver.current_epoch());
    assert_ne!(opened_by_again.current_epoch(), p.receiver.current_epoch());
    assert_ne!(
        opened_by_newer.current_epoch(),
        opened_by_again.current_epoch(),
        "the second session wrote the older opening, not the newer one again"
    );
}
