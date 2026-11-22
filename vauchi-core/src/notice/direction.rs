// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One direction of the notice core as its two devices hold it: the
//! receiver, which refreshes and moves, and the sender, which follows,
//! opens and writes.
//!
//! Without content and without the slot budget: every record a device
//! wants to write in a session is written. A device's sessions are
//! `read`, then `write`, then `acknowledge` when the relay took the write.

use std::collections::{BTreeMap, VecDeque};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::chain::{ChainReader, ChainWriter, PERIOD_SECONDS};
use super::keys::{
    Epoch, Keyed, Label, key_id, open_as_receiver, open_as_sender, opening_address, proof_tag,
    refresh_address,
};
use super::record::Record;
use super::seal::{NOTICE_BYTES, TAG_BYTES, open, seal};
use crate::crypto::X25519KeyPair;

/// How many addresses a refresh has in one epoch and week, and an
/// opening under one number in one base.
pub const ATTEMPTS: u8 = 4;
const RETENTION_SECONDS: u64 = 3 * PERIOD_SECONDS;
const REFRESH_EVERY_SECONDS: u64 = PERIOD_SECONDS;
/// The relays keep records 120 days; eighteen weeks cover that.
const WEEKS_WATCHED_BACK: u32 = 18;
/// As long as a refresh's addresses are watched, so that a replay inside
/// the watch is always known.
const REFRESH_MEMORY_SECONDS: u64 = (WEEKS_WATCHED_BACK as u64 + 2) * PERIOD_SECONDS;
const NUMBERS_WATCHED_AHEAD: u32 = 2;
const REFRESH_KEYS_HELD: usize = 3;
const NAMED_EPOCHS_KEPT: usize = 24;
const UNNAMED_EPOCHS_KEPT: usize = 12;
const ERASED_IDS_KEPT: usize = 64;
const REFRESHES_REMEMBERED: usize = 64;
const REFRESHES_A_SESSION: usize = 8;
/// A quarter of the reader's window, so withheld acknowledgements take
/// four periods to run a chain past it.
const POSITIONS_A_PERIOD: usize = 128;

pub type EpochId = [u8; 16];
type Notice = [u8; NOTICE_BYTES];

pub fn week(now: u64) -> u32 {
    u32::try_from(now / PERIOD_SECONDS).unwrap_or(u32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alarm {
    OpeningOnNumberNotIssued,
    OpeningKeyIdNotIssued,
    OpeningConfirmationFailed,
    TwoOpeningsOnOneNumber,
    RefreshConflict,
    ForeignName,
    TwoRecordsAtOneAddress,
}

/// An update key the receiver read, to be matched with its content.
pub struct UpdateKeyRead {
    pub update_key: Zeroizing<[u8; 32]>,
    pub commitment: [u8; 31],
}

enum Seen {
    First,
    Again,
    Different,
}

/// What a device first read at each address: a replay is dropped, and a
/// second, different record there can only come from a holder of the
/// epoch's keys. An entry is kept while its address is watched.
#[derive(Default)]
struct FirstRecords(BTreeMap<[u8; TAG_BYTES], FirstRecord>);

struct FirstRecord {
    hash: [u8; 8],
    read_at: u64,
    /// The week in a refresh address, which says when it stops being
    /// watched; `None` for an opening address, watched by age.
    address_week: Option<u32>,
}

fn short_hash(notice: &Notice) -> [u8; 8] {
    let digest = Sha256::digest(notice);
    let mut hash = [0u8; 8];
    for (to, from) in hash.iter_mut().zip(digest.iter()) {
        *to = *from;
    }
    hash
}

impl FirstRecords {
    fn seen(&self, tag: &[u8; TAG_BYTES], notice: &Notice) -> Seen {
        match self.0.get(tag) {
            None => Seen::First,
            Some(first) if first.hash == short_hash(notice) => Seen::Again,
            Some(_) => Seen::Different,
        }
    }

    fn remember(
        &mut self,
        tag: [u8; TAG_BYTES],
        notice: &Notice,
        now: u64,
        address_week: Option<u32>,
    ) {
        self.0.entry(tag).or_insert(FirstRecord {
            hash: short_hash(notice),
            read_at: now,
            address_week,
        });
    }

    fn forget_unwatched(&mut self, now: u64) {
        let this_week = week(now);
        self.0.retain(|_, first| match first.address_week {
            Some(address_week) => this_week <= address_week.saturating_add(WEEKS_WATCHED_BACK),
            // An opening's number is watched for three periods from its
            // refresh, which was made before the opening was read.
            None => age(now, &mut first.read_at) < RETENTION_SECONDS,
        });
    }
}

fn age(now: u64, stamp: &mut u64) -> u64 {
    if *stamp > now {
        *stamp = now;
    }
    now.saturating_sub(*stamp)
}

fn tag_of(notice: &Notice) -> [u8; TAG_BYTES] {
    let mut tag = [0u8; TAG_BYTES];
    for (to, from) in tag.iter_mut().zip(notice.iter()) {
        *to = *from;
    }
    tag
}

struct Issued {
    number: u32,
    public: [u8; 32],
    private: Option<X25519KeyPair>,
    key_id: [u8; 8],
    name: EpochId,
    made_at: u64,
    accepted: Option<[u8; 32]>,
}

struct ReceiverEpoch {
    keys: Epoch,
    reader: Option<ChainReader>,
    left_at: Option<u64>,
    last_sealed_at: Option<u64>,
    refresh_attempts: BTreeMap<u32, u8>,
}

impl ReceiverEpoch {
    fn new(mut keys: Epoch) -> Self {
        let reader = keys
            .take_notice_chain()
            .map(|chain_key| ChainReader::new(&chain_key));
        Self {
            keys,
            reader,
            left_at: None,
            last_sealed_at: None,
            refresh_attempts: BTreeMap::new(),
        }
    }
}

struct OpeningRead {
    number: u32,
    attempt: u8,
    base: EpochId,
    ephemeral_public: [u8; 32],
    key_id: [u8; 8],
    confirmation: [u8; 16],
}

pub struct Receiver {
    label: Label,
    device: X25519KeyPair,
    sender_device_public: [u8; 32],
    seed: EpochId,
    /// Oldest first; the last is the current epoch.
    epochs: Vec<ReceiverEpoch>,
    issued: Vec<Issued>,
    refresh_queued: bool,
    refresh_written: bool,
    alarm_epoch: Option<(EpochId, u64)>,
    first_records: FirstRecords,
    alarms: Vec<Alarm>,
}

impl Receiver {
    pub fn new(
        seed: Epoch,
        label: Label,
        device: X25519KeyPair,
        sender_device_public: [u8; 32],
        _now: u64,
    ) -> Self {
        Self {
            label,
            device,
            sender_device_public,
            seed: seed.id(),
            epochs: vec![ReceiverEpoch::new(seed)],
            issued: Vec::new(),
            refresh_queued: false,
            refresh_written: false,
            alarm_epoch: None,
            first_records: FirstRecords::default(),
            alarms: Vec::new(),
        }
    }

    pub fn alarms(&self) -> &[Alarm] {
        &self.alarms
    }

    pub fn seed_epoch(&self) -> EpochId {
        self.seed
    }

    pub fn current_epoch(&self) -> EpochId {
        self.epochs
            .last()
            .map_or(self.seed, |epoch| epoch.keys.id())
    }

    /// The epoch the newest refresh names.
    pub fn newest_name(&self) -> Option<EpochId> {
        self.issued.last().map(|issued| issued.name)
    }

    fn current_number(&self) -> u32 {
        self.epochs.last().map_or(0, |epoch| epoch.keys.number())
    }

    fn newest_number(&self) -> u32 {
        self.issued.last().map_or(0, |issued| issued.number)
    }

    fn raise(&mut self, alarm: Alarm, read_in: EpochId, now: u64) {
        self.alarms.push(alarm);
        self.alarm_epoch = Some((read_in, now));
    }

    /// Reads one session's notices: update keys on the chains it keeps,
    /// then openings in rising order of number, reading the chains again
    /// after each move so that notices the sender wrote in the new epoch
    /// in the same batches are not missed.
    pub fn read(&mut self, notices: &[Notice], now: u64) -> Vec<UpdateKeyRead> {
        self.expire(now);
        let mut updates = Vec::new();
        self.read_chains(notices, now, &mut updates);
        let mut openings = self.find_openings(notices, now);
        openings.sort_by_key(|opening| (opening.number, opening.attempt));
        for opening in openings {
            if self.accept(&opening, now) {
                self.read_chains(notices, now, &mut updates);
            }
        }
        updates
    }

    fn read_chains(&mut self, notices: &[Notice], now: u64, updates: &mut Vec<UpdateKeyRead>) {
        for notice in notices {
            for epoch in &mut self.epochs {
                let Some(reader) = epoch.reader.as_mut() else {
                    continue;
                };
                let Some((_, plaintext)) = reader.read(notice, now) else {
                    continue;
                };
                if let Ok(Record::UpdateKey {
                    update_key,
                    commitment,
                }) = Record::from_plaintext(&plaintext)
                {
                    updates.push(UpdateKeyRead {
                        update_key,
                        commitment,
                    });
                }
                break;
            }
        }
    }

    fn watched_numbers(&mut self, now: u64) -> Vec<u32> {
        let mut numbers: Vec<u32> = self
            .issued
            .iter_mut()
            .filter_map(|issued| {
                (age(now, &mut issued.made_at) < RETENTION_SECONDS).then_some(issued)
            })
            .map(|issued| issued.number)
            .collect();
        let newest = self.newest_number();
        for ahead in 1..=NUMBERS_WATCHED_AHEAD {
            if let Some(number) = newest.checked_add(ahead) {
                numbers.push(number);
            }
        }
        numbers
    }

    fn find_openings(&mut self, notices: &[Notice], now: u64) -> Vec<OpeningRead> {
        let numbers = self.watched_numbers(now);
        let mut watched: BTreeMap<[u8; TAG_BYTES], (Keyed, EpochId, u32, u8)> = BTreeMap::new();
        for epoch in &mut self.epochs {
            let Some(sealed_at) = epoch.last_sealed_at.as_mut() else {
                continue;
            };
            if age(now, sealed_at) >= RETENTION_SECONDS {
                continue;
            }
            for number in &numbers {
                for attempt in 0..ATTEMPTS {
                    let at = opening_address(epoch.keys.opening_key(), *number, attempt);
                    watched.insert(at.tag, (at, epoch.keys.id(), *number, attempt));
                }
            }
        }

        let mut found = Vec::new();
        for notice in notices {
            let tag = tag_of(notice);
            let Some((at, base, number, attempt)) = watched.get(&tag) else {
                continue;
            };
            let (base, number, attempt) = (*base, *number, *attempt);
            let Some(plaintext) = open(at, notice) else {
                continue;
            };
            match self.first_records.seen(&tag, notice) {
                Seen::Again => continue,
                Seen::Different => self.raise(Alarm::TwoRecordsAtOneAddress, base, now),
                Seen::First => self.first_records.remember(tag, notice, now, None),
            }
            let Ok(Record::Opening {
                ephemeral_public,
                number: said,
                key_id,
                confirmation,
            }) = Record::from_plaintext(&plaintext)
            else {
                continue;
            };
            if said != number {
                continue;
            }
            found.push(OpeningRead {
                number,
                attempt,
                base,
                ephemeral_public,
                key_id,
                confirmation,
            });
        }
        found
    }

    /// Whether the receiver moved.
    fn accept(&mut self, opening: &OpeningRead, now: u64) -> bool {
        if opening.number > self.newest_number() {
            self.raise(Alarm::OpeningOnNumberNotIssued, opening.base, now);
            return false;
        }
        let current_number = self.current_number();
        let Some(index) = self
            .issued
            .iter()
            .position(|issued| issued.number == opening.number)
        else {
            return false;
        };
        let Some(issued) = self.issued.get(index) else {
            return false;
        };
        if issued.key_id != opening.key_id {
            self.raise(Alarm::OpeningKeyIdNotIssued, opening.base, now);
            return false;
        }
        let ephemeral_hash: [u8; 32] = Sha256::digest(opening.ephemeral_public).into();
        if let Some(accepted) = issued.accepted {
            if accepted != ephemeral_hash {
                self.raise(Alarm::TwoOpeningsOnOneNumber, opening.base, now);
            }
            return false;
        }
        if opening.number <= current_number {
            return false;
        }
        let Some(private) = issued.private.as_ref() else {
            return false;
        };
        let Some(base) = self
            .epochs
            .iter()
            .find(|epoch| epoch.keys.id() == opening.base)
        else {
            return false;
        };
        let opened = open_as_receiver(
            &base.keys,
            &self.label,
            opening.number,
            private,
            &opening.ephemeral_public,
            &self.sender_device_public,
            &self.device,
        );
        let confirmed = opened
            .ok()
            .filter(|opened| bool::from(opened.confirmation.ct_eq(&opening.confirmation)));
        let Some(opened) = confirmed else {
            self.raise(Alarm::OpeningConfirmationFailed, opening.base, now);
            return false;
        };
        if let Some(issued) = self.issued.get_mut(index) {
            issued.accepted = Some(ephemeral_hash);
            issued.private = None;
        }
        if let Some(current) = self.epochs.last_mut() {
            current.left_at = Some(now);
        }
        self.epochs.push(ReceiverEpoch::new(opened.epoch));
        true
    }

    /// Makes a refresh when one is due and seals the newest, until it is
    /// acknowledged, in the current epoch, the one left most recently
    /// and the epoch of the most recent alarm.
    pub fn write(&mut self, now: u64) -> Vec<Notice> {
        self.expire(now);
        self.refresh_written = false;
        let due = match self.issued.last_mut() {
            None => true,
            Some(newest) => age(now, &mut newest.made_at) >= REFRESH_EVERY_SECONDS,
        };
        if due {
            self.make_refresh(now);
        }
        if !self.refresh_queued {
            return Vec::new();
        }
        let Some(newest) = self.issued.last() else {
            return Vec::new();
        };
        let (number, public, name) = (newest.number, newest.public, newest.name);

        let count = self.epochs.len();
        let mut targets = vec![count.saturating_sub(1)];
        if count >= 2 {
            targets.push(count.saturating_sub(2));
        }
        if let Some((alarmed, _)) = self.alarm_epoch {
            let index = self.epochs.iter().position(|e| e.keys.id() == alarmed);
            if let Some(index) = index.filter(|index| !targets.contains(index)) {
                targets.push(index);
            }
        }

        let this_week = week(now);
        let mut written = Vec::new();
        for index in targets {
            let Some(epoch) = self.epochs.get_mut(index) else {
                continue;
            };
            let attempt = epoch.refresh_attempts.entry(this_week).or_insert(0);
            if *attempt >= ATTEMPTS {
                continue;
            }
            let at = refresh_address(epoch.keys.control_key(), this_week, *attempt);
            *attempt = attempt.saturating_add(1);
            let record = Record::Refresh {
                refresh_public: public,
                number,
                proof_tag: proof_tag(epoch.keys.proof_key(), number, &public, &name),
            };
            written.push(seal(&at, &record.to_plaintext()));
            epoch.last_sealed_at = Some(now);
        }
        self.refresh_written = !written.is_empty();
        written
    }

    fn make_refresh(&mut self, now: u64) {
        let Some(number) = self.newest_number().checked_add(1) else {
            return;
        };
        let private = X25519KeyPair::generate();
        let public = *private.public_key();
        self.issued.push(Issued {
            number,
            public,
            key_id: key_id(&public),
            private: Some(private),
            name: self.current_epoch(),
            made_at: now,
            accepted: None,
        });
        self.refresh_queued = true;
        let mut held = 0usize;
        for issued in self.issued.iter_mut().rev() {
            if issued.private.is_none() {
                continue;
            }
            held = held.saturating_add(1);
            if held > REFRESH_KEYS_HELD {
                issued.private = None;
            }
        }
    }

    /// The relay took the last write.
    pub fn acknowledge(&mut self) {
        if self.refresh_written {
            self.refresh_queued = false;
        }
    }

    fn expire(&mut self, now: u64) {
        let alarm_is_old = self
            .alarm_epoch
            .as_mut()
            .is_some_and(|(_, alarmed_at)| age(now, alarmed_at) >= RETENTION_SECONDS);
        if alarm_is_old {
            self.alarm_epoch = None;
        }
        let alarmed = self.alarm_epoch.map(|(id, _)| id);
        let current = self.epochs.len().saturating_sub(1);
        let mut index = 0usize;
        self.epochs.retain_mut(|epoch| {
            let is_current = index == current;
            index = index.saturating_add(1);
            if is_current {
                if let Some(reader) = epoch.reader.as_mut() {
                    reader.expire(now);
                }
                return true;
            }
            // The epoch of the latest alarm keeps what sealing a refresh
            // there needs; its notice chain ends three periods after the
            // receiver left it, as every left epoch's does.
            let keep_for_alarm = Some(epoch.keys.id()) == alarmed;
            let since_left = epoch.left_at.as_mut().map_or(0, |left| age(now, left));
            if since_left >= RETENTION_SECONDS {
                epoch.reader = None;
            } else if let Some(reader) = epoch.reader.as_mut() {
                reader.expire(now);
            }
            let since_sealed = epoch
                .last_sealed_at
                .as_mut()
                .map_or(u64::MAX, |sealed| age(now, sealed));
            keep_for_alarm || since_left.min(since_sealed) < RETENTION_SECONDS
        });

        let newest = self.newest_number();
        self.issued.retain_mut(|issued| {
            issued.number == newest || age(now, &mut issued.made_at) < RETENTION_SECONDS
        });
        self.first_records.forget_unwatched(now);
    }
}

struct SenderEpoch {
    keys: Epoch,
    /// `None` only if the epoch's chain key was already taken.
    writer: Option<ChainWriter>,
    opened_at: u64,
    named_at: Option<u64>,
    /// The next attempt of an opening under each number with this epoch
    /// as its base: an address is written once, whoever asks.
    opening_attempts: BTreeMap<u32, u8>,
}

impl SenderEpoch {
    fn new(mut keys: Epoch, opened_at: u64, named_at: Option<u64>) -> Self {
        let writer = keys
            .take_notice_chain()
            .map(|chain_key| ChainWriter::new(&chain_key));
        Self {
            keys,
            writer,
            opened_at,
            named_at,
            opening_attempts: BTreeMap::new(),
        }
    }
}

struct RefreshRead {
    number: u32,
    public: [u8; 32],
    named: Option<EpochId>,
    opened: bool,
    read_at: u64,
}

struct OpeningQueued {
    number: u32,
    base: EpochId,
    epoch: EpochId,
    ephemeral_public: [u8; 32],
    key_id: [u8; 8],
    confirmation: [u8; 16],
    /// No address is left for it under its number in its base.
    spent: bool,
}

struct RefreshFound {
    number: u32,
    public: [u8; 32],
    proof: [u8; 16],
    read_in: EpochId,
    tag: [u8; TAG_BYTES],
    notice: Notice,
    address_week: u32,
}

pub struct Sender {
    label: Label,
    device: X25519KeyPair,
    receiver_device_public: [u8; 32],
    epochs: Vec<SenderEpoch>,
    writes_in: EpochId,
    highest_named: u32,
    erased: VecDeque<EpochId>,
    refreshes: Vec<RefreshRead>,
    openings: Vec<OpeningQueued>,
    latest: Option<(Zeroizing<[u8; 32]>, [u8; 31])>,
    view_due: bool,
    update_queued: bool,
    /// Counts edits, so that an acknowledgement clears only the edit the
    /// acknowledged write carried.
    edits: u64,
    edits_written: Option<u64>,
    positions_assigned_at: VecDeque<u64>,
    first_records: FirstRecords,
    alarms: Vec<Alarm>,
}

impl Sender {
    pub fn new(
        seed: Epoch,
        label: Label,
        device: X25519KeyPair,
        receiver_device_public: [u8; 32],
        now: u64,
    ) -> Self {
        let writes_in = seed.id();
        Self {
            label,
            device,
            receiver_device_public,
            epochs: vec![SenderEpoch::new(seed, now, Some(now))],
            writes_in,
            highest_named: 0,
            erased: VecDeque::new(),
            refreshes: Vec::new(),
            openings: Vec::new(),
            latest: None,
            view_due: false,
            update_queued: false,
            edits: 0,
            edits_written: None,
            positions_assigned_at: VecDeque::new(),
            first_records: FirstRecords::default(),
            alarms: Vec::new(),
        }
    }

    pub fn alarms(&self) -> &[Alarm] {
        &self.alarms
    }

    /// The epoch update keys are written in.
    pub fn writes_in(&self) -> EpochId {
        self.writes_in
    }

    /// The numbers of the epochs it keeps, in rising order: what a later
    /// copy of the device could still read or write in.
    pub fn kept_numbers(&self) -> Vec<u32> {
        let mut numbers: Vec<u32> = self.epochs.iter().map(|e| e.keys.number()).collect();
        numbers.sort_unstable();
        numbers
    }

    pub fn keeps(&self, epoch: &EpochId) -> bool {
        self.epochs.iter().any(|kept| kept.keys.id() == *epoch)
    }

    /// The newest update of the contact's group; it replaces one still
    /// queued, and the replaced key is erased.
    pub fn queue_update(&mut self, update_key: &[u8; 32], commitment: &[u8; 31]) {
        self.latest = Some((Zeroizing::new(*update_key), *commitment));
        self.update_queued = true;
        self.edits = self.edits.saturating_add(1);
    }

    /// Reads one session's notices: refreshes at the addresses of every
    /// epoch it keeps. Those it already knows cost nothing; of the rest it
    /// handles the eight with the highest numbers, in rising order, and
    /// leaves the others unread so that a later delivery can still be
    /// read.
    pub fn read(&mut self, notices: &[Notice], now: u64) {
        self.retain(now);
        let mut new = Vec::new();
        for refresh in self.find_refreshes(notices, now) {
            if self.knows(&refresh) {
                self.first_records.remember(
                    refresh.tag,
                    &refresh.notice,
                    now,
                    Some(refresh.address_week),
                );
            } else {
                new.push(refresh);
            }
        }
        new.sort_by_key(|refresh| refresh.number);
        let skip = new.len().saturating_sub(REFRESHES_A_SESSION);
        for refresh in new.into_iter().skip(skip) {
            self.first_records.remember(
                refresh.tag,
                &refresh.notice,
                now,
                Some(refresh.address_week),
            );
            self.handle(&refresh, now);
        }
        self.retain(now);
    }

    /// Whether this refresh, with this key and this name, was read before.
    fn knows(&self, refresh: &RefreshFound) -> bool {
        let Some(proof_key) = self
            .epochs
            .iter()
            .find(|epoch| epoch.keys.id() == refresh.read_in)
            .map(|epoch| Zeroizing::new(*epoch.keys.proof_key()))
        else {
            return false;
        };
        let named = self.named_by(refresh, &proof_key);
        self.refreshes.iter().any(|read| {
            read.number == refresh.number && read.public == refresh.public && read.named == named
        })
    }

    fn find_refreshes(&mut self, notices: &[Notice], now: u64) -> Vec<RefreshFound> {
        let this_week = week(now);
        let mut watched: BTreeMap<[u8; TAG_BYTES], (Keyed, EpochId, u32)> = BTreeMap::new();
        for epoch in &self.epochs {
            let from = this_week.saturating_sub(WEEKS_WATCHED_BACK);
            for watched_week in from..=this_week.saturating_add(1) {
                for attempt in 0..ATTEMPTS {
                    let at = refresh_address(epoch.keys.control_key(), watched_week, attempt);
                    watched.insert(at.tag, (at, epoch.keys.id(), watched_week));
                }
            }
        }

        let mut found = Vec::new();
        for notice in notices {
            let tag = tag_of(notice);
            let Some((at, read_in, address_week)) = watched.get(&tag) else {
                continue;
            };
            let (read_in, address_week) = (*read_in, *address_week);
            let Some(plaintext) = open(at, notice) else {
                continue;
            };
            match self.first_records.seen(&tag, notice) {
                Seen::Again => continue,
                Seen::Different => self.alarms.push(Alarm::TwoRecordsAtOneAddress),
                Seen::First => {}
            }
            let Ok(Record::Refresh {
                refresh_public,
                number,
                proof_tag,
            }) = Record::from_plaintext(&plaintext)
            else {
                self.first_records
                    .remember(tag, notice, now, Some(address_week));
                continue;
            };
            found.push(RefreshFound {
                number,
                public: refresh_public,
                proof: proof_tag,
                read_in,
                tag,
                notice: *notice,
                address_week,
            });
        }
        found
    }

    /// The epoch a refresh names, among those kept and then those erased.
    fn named_by(&self, refresh: &RefreshFound, proof_key: &[u8; 32]) -> Option<EpochId> {
        self.epochs
            .iter()
            .map(|epoch| epoch.keys.id())
            .chain(self.erased.iter().copied())
            .find(|id| {
                let expected = proof_tag(proof_key, refresh.number, &refresh.public, id);
                bool::from(expected.ct_eq(&refresh.proof))
            })
    }

    fn handle(&mut self, refresh: &RefreshFound, now: u64) {
        let Some(read_in) = self
            .epochs
            .iter()
            .position(|epoch| epoch.keys.id() == refresh.read_in)
        else {
            return;
        };
        let Some(proof_key) = self
            .epochs
            .get(read_in)
            .map(|epoch| Zeroizing::new(*epoch.keys.proof_key()))
        else {
            return;
        };
        let named = self.named_by(refresh, &proof_key);

        let under_number = |read: &&RefreshRead| read.number == refresh.number;
        if self
            .refreshes
            .iter()
            .filter(under_number)
            .any(|read| read.public == refresh.public && read.named == named)
        {
            return;
        }
        if self.refreshes.iter().any(|read| under_number(&read)) {
            self.alarms.push(Alarm::RefreshConflict);
        }
        let opened_before = self
            .refreshes
            .iter()
            .filter(under_number)
            .any(|read| read.public == refresh.public && read.opened);

        match named {
            Some(id) => {
                if let Some(epoch) = self.epochs.iter_mut().find(|e| e.keys.id() == id) {
                    epoch.named_at = Some(now);
                    self.highest_named = self.highest_named.max(epoch.keys.number());
                    self.writes_in = id;
                    self.view_due = true;
                    self.openings.retain(|opening| opening.epoch != id);
                }
            }
            None => {
                self.alarms.push(Alarm::ForeignName);
                self.writes_in = refresh.read_in;
                self.view_due = true;
            }
        }

        if !opened_before {
            self.open_on(refresh, read_in, now);
        }
        self.refreshes.push(RefreshRead {
            number: refresh.number,
            public: refresh.public,
            named,
            opened: true,
            read_at: now,
        });
    }

    fn open_on(&mut self, refresh: &RefreshFound, read_in: usize, now: u64) {
        let Some(base) = self.epochs.get(read_in) else {
            return;
        };
        let ephemeral = X25519KeyPair::generate();
        let Ok(opened) = open_as_sender(
            &base.keys,
            &self.label,
            refresh.number,
            &refresh.public,
            &ephemeral,
            &self.device,
            &self.receiver_device_public,
        ) else {
            return;
        };
        self.openings.push(OpeningQueued {
            number: refresh.number,
            base: refresh.read_in,
            epoch: opened.epoch.id(),
            ephemeral_public: *ephemeral.public_key(),
            key_id: key_id(&refresh.public),
            confirmation: opened.confirmation,
            spent: false,
        });
        self.epochs.push(SenderEpoch::new(opened.epoch, now, None));
    }

    /// Writes the openings not yet named, each at its next address until
    /// its fourth, and the newest update key when a refresh was followed
    /// or an edit is waiting.
    pub fn write(&mut self, now: u64) -> Vec<Notice> {
        self.retain(now);
        self.edits_written = None;
        let mut written = Vec::new();
        for opening in &mut self.openings {
            let Some(base) = self.epochs.iter_mut().find(|e| e.keys.id() == opening.base) else {
                continue;
            };
            let attempt = base.opening_attempts.entry(opening.number).or_insert(0);
            if *attempt >= ATTEMPTS {
                opening.spent = true;
                continue;
            }
            let at = opening_address(base.keys.opening_key(), opening.number, *attempt);
            *attempt = attempt.saturating_add(1);
            let record = Record::Opening {
                ephemeral_public: opening.ephemeral_public,
                number: opening.number,
                key_id: opening.key_id,
                confirmation: opening.confirmation,
            };
            written.push(seal(&at, &record.to_plaintext()));
        }
        self.openings.retain(|opening| !opening.spent);

        if (self.view_due || self.update_queued)
            && self.positions_assigned_at.len() < POSITIONS_A_PERIOD
        {
            let writes_in = self.writes_in;
            let epoch = self.epochs.iter_mut().find(|e| e.keys.id() == writes_in);
            let writer = epoch.and_then(|epoch| epoch.writer.as_mut());
            if let (Some(writer), Some((update_key, commitment))) = (writer, self.latest.as_ref()) {
                let (_, at) = writer.assign();
                self.positions_assigned_at.push_back(now);
                let record = Record::UpdateKey {
                    update_key: update_key.clone(),
                    commitment: *commitment,
                };
                written.push(seal(&at, &record.to_plaintext()));
                self.edits_written = Some(self.edits);
            }
        }
        written
    }

    /// The relay took the last write.
    pub fn acknowledge(&mut self) {
        if let Some(edits_written) = self.edits_written.take() {
            self.view_due = false;
            if edits_written == self.edits {
                self.update_queued = false;
            }
        }
    }

    fn erase(&mut self, id: EpochId) {
        self.epochs.retain(|epoch| epoch.keys.id() != id);
        self.openings
            .retain(|opening| opening.epoch != id && opening.base != id);
        self.erased.push_back(id);
        while self.erased.len() > ERASED_IDS_KEPT {
            self.erased.pop_front();
        }
    }

    fn retain(&mut self, now: u64) {
        let writes_in = self.writes_in;
        let highest_named = self.highest_named;
        let mut to_erase = Vec::new();
        for epoch in &mut self.epochs {
            let id = epoch.keys.id();
            if id == writes_in {
                continue;
            }
            // The receiver only moves upwards and seals every refresh in
            // its current epoch. Until a refresh names a higher epoch, an
            // epoch may hold the receiver, named or not, whatever an older
            // refresh delivered late made the sender write in meanwhile.
            if epoch.keys.number() >= highest_named {
                continue;
            }
            let expired = match epoch.named_at.as_mut() {
                Some(named_at) => age(now, named_at) >= RETENTION_SECONDS,
                None => age(now, &mut epoch.opened_at) >= RETENTION_SECONDS,
            };
            if expired {
                to_erase.push(id);
            }
        }
        for id in to_erase {
            self.erase(id);
        }

        loop {
            let named = self.epochs.iter().filter(|e| e.named_at.is_some()).count();
            if named <= NAMED_EPOCHS_KEPT {
                break;
            }
            let oldest = self
                .epochs
                .iter()
                .filter(|e| e.keys.id() != writes_in && e.keys.number() < highest_named)
                .filter_map(|e| e.named_at.map(|at| (at, e.keys.id())))
                .min();
            match oldest {
                Some((_, id)) => self.erase(id),
                None => break,
            }
        }
        loop {
            let unnamed = self.epochs.iter().filter(|e| e.named_at.is_none()).count();
            if unnamed <= UNNAMED_EPOCHS_KEPT {
                break;
            }
            let lowest = self
                .epochs
                .iter()
                .filter(|e| e.named_at.is_none() && e.keys.id() != writes_in)
                .map(|e| (e.keys.number(), e.keys.id()))
                .min();
            match lowest {
                Some((_, id)) => self.erase(id),
                None => break,
            }
        }

        self.refreshes
            .retain_mut(|read| age(now, &mut read.read_at) < REFRESH_MEMORY_SECONDS);
        while self.refreshes.len() > REFRESHES_REMEMBERED {
            let oldest = self
                .refreshes
                .iter()
                .enumerate()
                .min_by_key(|(_, read)| read.read_at)
                .map(|(index, _)| index);
            match oldest {
                Some(index) => {
                    self.refreshes.remove(index);
                }
                None => break,
            }
        }
        while let Some(assigned_at) = self.positions_assigned_at.front_mut() {
            if age(now, assigned_at) < PERIOD_SECONDS {
                break;
            }
            self.positions_assigned_at.pop_front();
        }
        self.first_records.forget_unwatched(now);
    }
}
