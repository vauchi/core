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
//! keeps it, and every session may end at any cut point. The gateway
//! between them replays earlier uploads and alters bytes (claim P5).

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use crate::common::notice_pair::*;
use vauchi_core::notice::content::{RECORD_BYTES, commitment};
use vauchi_core::notice::direction::{Alarm, Receiver, Sender};
use vauchi_core::notice::publish::{ContentRecord, Pending, Publisher};
use vauchi_core::notice::seal::{NOTICE_BYTES, TAG_BYTES};
use vauchi_core::notice::session::{BACKGROUND, FAST, Session, SessionWrite};

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

/// What the gateway does to an upload on its way to the other devices
/// (claim P5: it replays, alters and injects bytes, and picks the order).
#[derive(Debug, Clone, Copy)]
enum Relay {
    Faithful,
    /// Also delivers, after it, an upload of any device from earlier on.
    ReplayEarlier(usize),
    /// Flips one byte in every other record, notices and content alike.
    Alter(usize),
    /// Delivers the notices and keeps the content back, to replay later.
    WithholdContent,
}

fn relay() -> impl Strategy<Value = Relay> {
    prop_oneof![
        2 => Just(Relay::Faithful),
        1 => any::<usize>().prop_map(Relay::ReplayEarlier),
        1 => any::<usize>().prop_map(Relay::Alter),
        1 => Just(Relay::WithholdContent),
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
    relay: Relay,
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
        relay(),
    )
        .prop_map(
            |(days, device, edit, fast, cut, dropped, acknowledged, clock_ahead_days, relay)| {
                Step {
                    days,
                    device,
                    edit,
                    fast,
                    cut,
                    dropped,
                    acknowledged,
                    clock_ahead_days,
                    relay,
                }
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
    inbox: [(Vec<Notice>, Vec<ContentRecord>); 3],
    /// Every notice a saved session sealed, by tag.
    sealed: BTreeMap<[u8; TAG_BYTES], Notice>,
    latest_key: [u8; 32],
    latest_card: Vec<u8>,
    edits: u8,
    /// An edit whose session ended before its save, so it is queued again.
    edit_lost: bool,
    /// Every card a saved session of device 0 published.
    published: BTreeSet<Vec<u8>>,
    /// Every upload, which the gateway may deliver again later.
    uploads: Vec<(Vec<Notice>, Vec<ContentRecord>)>,
    /// Whether each of devices 1 and 2 read the newest update key, and
    /// the card it shows: the highest version it installed (R4).
    read_latest: [bool; 2],
    shown: [Vec<u8>; 2],
    /// The versions each of devices 1 and 2 installed, in order.
    installed_versions: [Vec<u8>; 2],
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
            published: BTreeSet::new(),
            uploads: Vec::new(),
            read_latest: [false; 2],
            shown: Default::default(),
            installed_versions: Default::default(),
        }
    }

    /// What the other devices receive of an upload.
    fn deliver(&self, relay: Relay, written: &SessionWrite) -> (Vec<Notice>, Vec<ContentRecord>) {
        let mut notices = written.notices.clone();
        let mut contents = written.contents.clone();
        match relay {
            Relay::Faithful => {}
            Relay::WithholdContent => contents.clear(),
            Relay::ReplayEarlier(pick) => {
                if let Some((old_notices, old_contents)) =
                    self.uploads.get(pick % self.uploads.len().max(1))
                {
                    notices.extend(old_notices.iter().copied());
                    contents.extend(old_contents.iter().copied());
                }
            }
            Relay::Alter(pick) => {
                for (i, notice) in notices.iter_mut().enumerate() {
                    if (i + pick) % 2 == 0 {
                        notice[(pick / 2 + 7 * i) % NOTICE_BYTES] ^= 1;
                    }
                }
                for (i, record) in contents.iter_mut().enumerate() {
                    if (i + pick) % 2 == 0 {
                        record[(pick / 2 + 7 * i) % RECORD_BYTES] ^= 1;
                    }
                }
            }
        }
        (notices, contents)
    }

    /// One session of `step.device`; `Err` names a key sealed twice.
    fn session(&mut self, step: &Step, now: u64) -> Result<(), String> {
        let d = step.device;
        let mut device = self.devices[d].load(now);
        let (notices, contents) = std::mem::take(&mut self.inbox[d]);
        let mut read_latest = false;
        let mut installed = Vec::new();
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
            installed = device.pending.read(&contents, now, |_| true);
            if installed
                .iter()
                .any(|body| !self.published.contains(&**body))
            {
                return Err(format!("device {d} installed a card never published"));
            }
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
            self.published.insert(card.clone());
            (self.latest_key, self.latest_card, self.edits) = (key, card, edits);
            self.edit_lost = false;
            self.read_latest = [false; 2];
        } else if d > 0 {
            self.read_latest[d - 1] |= read_latest;
            let shown = &mut self.shown[d - 1];
            for body in installed {
                self.installed_versions[d - 1].extend(body.first());
                if body.first() > shown.first() {
                    *shown = body.to_vec();
                }
            }
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
            let (notices, contents) = self.deliver(step.relay, &written);
            for other in (0..3).filter(|o| *o != d) {
                self.inbox[other].0.extend(notices.iter().copied());
                self.inbox[other].1.extend(contents.iter().copied());
            }
        }
        self.uploads
            .push((written.notices.clone(), written.contents.clone()));
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
    // nothing alarms, whatever the gateway replays or alters (P5); no card
    // installs that was not published; and after faithful weeks each
    // receiver shows the latest card.
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
            relay: Relay::Faithful,
        };
        let faithful = (0..63).map(|i| Step {
            days: 1, device: i % 3, edit: false, fast: false, cut: Cut::Completed,
            dropped: false, acknowledged: true, clock_ahead_days: 0,
            relay: Relay::Faithful,
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
        let latest = run.latest_card.clone();
        prop_assert_eq!(run.shown, [latest.clone(), latest], "a receiver does not show the latest card");
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
        relay: Relay::Faithful,
    };

    run.session(&edit(Cut::SaveUndoneAfterUpload), START)
        .unwrap();
    let fault = run.session(&edit(Cut::Completed), START + DAY);

    assert_eq!(fault, Err("device 0 sealed one key twice".to_string()));
}

// A gateway that delivers an edit's notice, holds its content back and
// replays it once a newer card installed makes the receiver install the
// older card last: only the highest version shown (R4) keeps the latest.
// @internal
#[test]
fn an_older_card_replayed_after_the_latest_installs_and_is_not_shown() {
    let mut run = Run::new();
    let step = |device, edit, relay| Step {
        days: 0,
        device,
        edit,
        fast: false,
        cut: Cut::Completed,
        dropped: false,
        acknowledged: true,
        clock_ahead_days: 0,
        relay,
    };
    let sessions = [
        step(0, true, Relay::WithholdContent),
        step(1, false, Relay::Faithful),
        step(0, true, Relay::Faithful),
        step(1, false, Relay::Faithful),
        step(0, false, Relay::ReplayEarlier(0)),
        step(1, false, Relay::Faithful),
    ];

    for (i, session) in sessions.iter().enumerate() {
        run.session(session, START + i as u64 * 3600).unwrap();
    }

    assert_eq!(run.installed_versions[0], vec![2, 1]);
    assert_eq!(run.shown[0], vec![2u8; 600]);
    assert_eq!(run.alarms(START + DAY), Vec::<Alarm>::new());
}
