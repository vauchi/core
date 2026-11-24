// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Whole devices that crash between assigning, saving, uploading and
//! acknowledging (vauchi/private#1389, gate 16's first test).
//!
//! The notice seal has a fixed nonce under each single-use key (ADR-019
//! amendment 2026-11-24): a key that sealed two different plaintexts
//! would expose their XOR and let its authenticator be forged. Each
//! device here lives only as its saved bytes, as protocol section 7
//! keeps it, and every session may end at any cut point.

use std::collections::BTreeMap;

use proptest::prelude::*;

use crate::common::notice_pair::*;
use vauchi_core::notice::content::commitment;
use vauchi_core::notice::direction::{Alarm, Receiver, Sender};
use vauchi_core::notice::publish::{Pending, Publisher};
use vauchi_core::notice::seal::TAG_BYTES;
use vauchi_core::notice::session::{BACKGROUND, FAST, Session};

/// Where a session stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// Before the save: the next session starts from the previous one.
    BeforeSave,
    /// Saved, then lost before anything was uploaded.
    BeforeUpload,
    /// Uploaded, then lost before the acknowledgement was saved.
    BeforeAcknowledgement,
    Completed,
    /// Uploaded, then a power loss undid the save: what a save that is
    /// not durable allows (gate 16, Claude). Never drawn by the property.
    SaveUndoneAfterUpload,
}

fn cut() -> impl Strategy<Value = Cut> {
    prop_oneof![
        Just(Cut::BeforeSave),
        Just(Cut::BeforeUpload),
        Just(Cut::BeforeAcknowledgement),
        Just(Cut::Completed),
    ]
}

#[derive(Debug, Clone)]
struct Step {
    days: u64,
    device: usize,
    edit: bool,
    fast: bool,
    cut: Cut,
    dropped: bool,
    acknowledged: bool,
    /// How far this device's clock runs ahead of true time, so a device
    /// may be set back by up to three days (specification section 1).
    clock_ahead_days: u64,
}

fn step() -> impl Strategy<Value = Step> {
    (
        0u64..3,
        0usize..3,
        any::<bool>(),
        any::<bool>(),
        cut(),
        any::<bool>(),
        any::<bool>(),
        0u64..4,
    )
        .prop_map(
            |(days, device, edit, fast, cut, dropped, acknowledged, clock_ahead_days)| Step {
                days,
                device,
                edit,
                fast,
                cut,
                dropped,
                acknowledged,
                clock_ahead_days,
            },
        )
}

/// One device as its saved bytes.
#[derive(Clone)]
struct Saved {
    senders: Vec<Vec<u8>>,
    receivers: Vec<Vec<u8>>,
    publishers: Vec<Vec<u8>>,
    pending: Vec<u8>,
    session: Vec<u8>,
}

/// One device loaded for a session.
struct Loaded {
    senders: Vec<Sender>,
    receivers: Vec<Receiver>,
    publishers: Vec<Publisher>,
    pending: Pending,
    session: Session,
}

impl Saved {
    fn new(senders: Vec<Sender>, receivers: Vec<Receiver>) -> Self {
        Loaded {
            senders,
            receivers,
            publishers: Vec::new(),
            pending: Pending::default(),
            session: Session::default(),
        }
        .save()
    }

    fn load(&self, now: u64) -> Loaded {
        Loaded {
            senders: self
                .senders
                .iter()
                .map(|b| Sender::load(b, now).expect("sender"))
                .collect(),
            receivers: self
                .receivers
                .iter()
                .map(|b| Receiver::load(b, now).expect("receiver"))
                .collect(),
            publishers: self
                .publishers
                .iter()
                .map(|b| Publisher::load(b).expect("publisher"))
                .collect(),
            pending: Pending::load(&self.pending, now).expect("pending"),
            session: Session::load(&self.session).expect("session"),
        }
    }
}

impl Loaded {
    fn save(&self) -> Saved {
        Saved {
            senders: self
                .senders
                .iter()
                .map(|s| s.save().unwrap().to_vec())
                .collect(),
            receivers: self
                .receivers
                .iter()
                .map(|r| r.save().unwrap().to_vec())
                .collect(),
            publishers: self
                .publishers
                .iter()
                .map(|p| p.save().unwrap().to_vec())
                .collect(),
            pending: self.pending.save().unwrap().to_vec(),
            session: self.session.save().unwrap().to_vec(),
        }
    }

    fn alarms(&self) -> Vec<Alarm> {
        self.senders
            .iter()
            .flat_map(|s| s.alarms().to_vec())
            .chain(self.receivers.iter().flat_map(|r| r.alarms().to_vec()))
            .collect()
    }
}

/// Device 0 writes to devices 1 and 2, and both write back: device 0
/// runs two directions each way.
fn world() -> [Saved; 3] {
    let to_1 = pair();
    let to_2 = pair();
    let from_1 = pair();
    let from_2 = pair();
    [
        Saved::new(
            vec![to_1.sender, to_2.sender],
            vec![from_1.receiver, from_2.receiver],
        ),
        Saved::new(vec![from_1.sender], vec![to_1.receiver]),
        Saved::new(vec![from_2.sender], vec![to_2.receiver]),
    ]
}

struct Run {
    devices: [Saved; 3],
    inbox: [(
        Vec<Notice>,
        Vec<vauchi_core::notice::publish::ContentRecord>,
    ); 3],
    /// Every notice a saved session sealed, by tag.
    sealed: BTreeMap<[u8; TAG_BYTES], Notice>,
    latest_key: [u8; 32],
    latest_card: Vec<u8>,
    edits: u8,
    /// An edit whose session ended before its save, so it is queued again.
    edit_lost: bool,
    /// The newest update key each of devices 1 and 2 read, and whether it
    /// installed device 0's latest card.
    read_latest: [bool; 2],
    installed_latest: [bool; 2],
}

impl Run {
    fn new() -> Self {
        Self {
            devices: world(),
            inbox: Default::default(),
            sealed: BTreeMap::new(),
            latest_key: [0; 32],
            latest_card: Vec::new(),
            edits: 0,
            edit_lost: false,
            read_latest: [false; 2],
            installed_latest: [false; 2],
        }
    }

    /// One session of `step.device`; `Err` names a key sealed twice.
    fn session(&mut self, step: &Step, now: u64) -> Result<(), String> {
        let d = step.device;
        let mut device = self.devices[d].load(now);
        let (notices, contents) = std::mem::take(&mut self.inbox[d]);
        let mut read_latest = false;
        let mut installed_latest = false;
        for sender in device.senders.iter_mut() {
            sender.read(&notices, now);
        }
        for receiver in device.receivers.iter_mut() {
            for read in receiver.read(&notices, now) {
                device.pending.note(&read.update_key, &read.commitment, now);
                read_latest |= *read.update_key == self.latest_key;
            }
        }
        if d > 0 {
            let installed = device.pending.read(&contents, now, |_| true);
            installed_latest = installed.iter().any(|body| **body == self.latest_card);
        }
        // An edit is the app's card: one lost with its session is queued
        // again by the next, so only an edit that was saved counts.
        let edit = (d == 0 && (step.edit || self.edit_lost)).then(|| {
            let edits = self.edits.wrapping_add(1);
            ([edits; 32], vec![edits; 600], edits)
        });
        if let Some((key, card, _)) = &edit {
            let commitment = commitment(key, card);
            for sender in device.senders.iter_mut() {
                sender.queue_update(key, &commitment);
            }
            device.publishers = vec![Publisher::new(*key, card)];
        }
        let tier = if step.fast { FAST } else { BACKGROUND };
        let written = {
            let mut senders: Vec<&mut Sender> = device.senders.iter_mut().collect();
            let mut receivers: Vec<&mut Receiver> = device.receivers.iter_mut().collect();
            device.session.write(
                &mut senders,
                &mut receivers,
                &mut device.publishers,
                tier,
                now,
            )
        };
        if step.cut == Cut::BeforeSave {
            // Protocol section 7: the batches are read again next time.
            self.inbox[d].0.splice(0..0, notices);
            self.inbox[d].1.splice(0..0, contents);
            self.edit_lost |= edit.is_some();
            return Ok(());
        }
        let before = std::mem::replace(&mut self.devices[d], device.save());
        if let Some((key, card, edits)) = edit {
            (self.latest_key, self.latest_card, self.edits) = (key, card, edits);
            self.edit_lost = false;
            self.read_latest = [false; 2];
            self.installed_latest = [false; 2];
        } else if d > 0 {
            self.read_latest[d - 1] |= read_latest;
            self.installed_latest[d - 1] |= installed_latest;
        }
        for notice in &written.notices {
            let mut tag = [0u8; TAG_BYTES];
            tag.copy_from_slice(&notice[..TAG_BYTES]);
            if let Some(earlier) = self.sealed.insert(tag, *notice)
                && earlier != *notice
            {
                return Err(format!("device {d} sealed one key twice"));
            }
        }
        if step.cut == Cut::BeforeUpload {
            return Ok(());
        }
        if !step.dropped {
            for other in (0..3).filter(|o| *o != d) {
                self.inbox[other].0.extend(written.notices.iter().copied());
                self.inbox[other].1.extend(written.contents.iter().cloned());
            }
        }
        if step.cut == Cut::SaveUndoneAfterUpload {
            self.devices[d] = before;
            return Ok(());
        }
        if step.cut == Cut::BeforeAcknowledgement || !step.acknowledged {
            return Ok(());
        }
        {
            let mut senders: Vec<&mut Sender> = device.senders.iter_mut().collect();
            let mut receivers: Vec<&mut Receiver> = device.receivers.iter_mut().collect();
            Session::acknowledge(&mut senders, &mut receivers, &mut device.publishers, now);
        }
        self.devices[d] = device.save();
        Ok(())
    }

    fn alarms(&self, now: u64) -> Vec<Alarm> {
        self.devices
            .iter()
            .flat_map(|saved| saved.load(now).alarms())
            .collect()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // Gate 16 (Codex, Kimi, Claude): no tag a saved session sealed is ever
    // sealed again with other bytes, whatever cut each session stops at;
    // nothing alarms; and after faithful weeks the latest card arrives.
    // @internal
    #[test]
    fn no_key_seals_twice_across_crashes_and_the_latest_card_arrives(
        steps in proptest::collection::vec(step(), 1..60),
    ) {
        let mut run = Run::new();
        let mut now = START;
        let first_edit = Step {
            days: 0, device: 0, edit: true, fast: false, cut: Cut::Completed,
            dropped: false, acknowledged: true, clock_ahead_days: 0,
        };
        let faithful = (0..63).map(|i| Step {
            days: 1, device: i % 3, edit: false, fast: false, cut: Cut::Completed,
            dropped: false, acknowledged: true, clock_ahead_days: 0,
        });
        for step in std::iter::once(first_edit).chain(steps).chain(faithful) {
            now += step.days * DAY;
            let clock = now + step.clock_ahead_days * DAY;
            if let Err(fault) = run.session(&step, clock) {
                prop_assert!(false, "{}", fault);
            }
        }
        prop_assert_eq!(run.alarms(now), Vec::<Alarm>::new());
        prop_assert_eq!(run.read_latest, [true, true], "the latest update key never arrived");
        prop_assert_eq!(run.installed_latest, [true, true], "the latest card never installed");
    }
}

// The check above can fail: a save a power loss undoes after its notices
// were uploaded makes the next session seal new plaintexts at the same
// positions, which is why protocol section 7 requires a durable save.
// @internal
#[test]
fn a_save_undone_after_its_upload_seals_a_key_twice() {
    let mut run = Run::new();
    let edit = |cut| Step {
        days: 1,
        device: 0,
        edit: true,
        fast: false,
        cut,
        dropped: false,
        acknowledged: true,
        clock_ahead_days: 0,
    };

    run.session(&edit(Cut::SaveUndoneAfterUpload), START)
        .unwrap();
    let fault = run.session(&edit(Cut::Completed), START + DAY);

    assert_eq!(fault, Err("device 0 sealed one key twice".to_string()));
}
