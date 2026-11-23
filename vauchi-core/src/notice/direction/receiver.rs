// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The receiver of one direction: it refreshes, reads openings and moves.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::*;
use crate::crypto::X25519KeyPair;
use crate::notice::chain::ChainReader;
use crate::notice::keys::{
    Epoch, Keyed, Label, key_id, open_as_receiver, opening_address, proof_tag, refresh_address,
};
use crate::notice::record::Record;
use crate::notice::seal::{TAG_BYTES, open, seal};

#[derive(serde::Serialize, serde::Deserialize)]
struct Issued {
    number: u32,
    public: [u8; 32],
    #[serde(with = "crate::notice::saved::optional_key_pair")]
    private: Option<X25519KeyPair>,
    key_id: [u8; 8],
    name: EpochId,
    made_at: u64,
    accepted: Option<[u8; 32]>,
}

#[cfg_attr(feature = "testing", derive(Clone))]
#[derive(serde::Serialize, serde::Deserialize)]
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

#[cfg_attr(feature = "testing", derive(Clone))]
#[derive(serde::Serialize, serde::Deserialize)]
struct OpeningRead {
    number: u32,
    attempt: u8,
    base: EpochId,
    ephemeral_public: [u8; 32],
    key_id: [u8; 8],
    confirmation: [u8; 16],
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Receiver {
    label: Label,
    #[serde(with = "crate::notice::saved::key_pair")]
    device: X25519KeyPair,
    sender_device_public: [u8; 32],
    seed: EpochId,
    /// Oldest first; the last is the current epoch.
    epochs: Vec<ReceiverEpoch>,
    issued: Vec<Issued>,
    refresh_queued: bool,
    refresh_written: bool,
    /// The epochs the newest refresh was sealed in by an acknowledged
    /// session, and those this session sealed it in; a copy cut by a grant
    /// waits for a later session (gate review 6).
    sealed_in: Vec<EpochId>,
    written_in: Vec<EpochId>,
    copies_left_after_write: bool,
    alarm_epoch: Option<(EpochId, u64)>,
    first_records: FirstRecords,
    alarms: Vec<Alarm>,
}

#[cfg(feature = "testing")]
impl Clone for Issued {
    fn clone(&self) -> Self {
        Self {
            number: self.number,
            public: self.public,
            private: self.private.as_ref().map(copy_key),
            key_id: self.key_id,
            name: self.name,
            made_at: self.made_at,
            accepted: self.accepted,
        }
    }
}

#[cfg(feature = "testing")]
impl Clone for Receiver {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            device: copy_key(&self.device),
            sender_device_public: self.sender_device_public,
            seed: self.seed,
            epochs: self.epochs.clone(),
            issued: self.issued.clone(),
            refresh_queued: self.refresh_queued,
            refresh_written: self.refresh_written,
            sealed_in: self.sealed_in.clone(),
            written_in: self.written_in.clone(),
            copies_left_after_write: self.copies_left_after_write,
            alarm_epoch: self.alarm_epoch,
            first_records: self.first_records.clone(),
            alarms: self.alarms.clone(),
        }
    }
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
            sealed_in: Vec::new(),
            written_in: Vec::new(),
            copies_left_after_write: false,
            alarm_epoch: None,
            first_records: FirstRecords::default(),
            alarms: Vec::new(),
        }
    }

    /// This direction's state as saved between sessions (section 5).
    pub fn save(&self) -> Result<Zeroizing<Vec<u8>>, crate::notice::saved::SavedError> {
        crate::notice::saved::save(crate::notice::saved::Kind::Receiver, self)
    }

    /// Loads saved state and erases what expired before anything reads
    /// (P2, owner 2026-11-23): a device off for weeks keeps its expired
    /// keys only until it next runs.
    pub fn load(bytes: &[u8], now: u64) -> Result<Self, crate::notice::saved::SavedError> {
        let mut receiver: Self =
            crate::notice::saved::load(crate::notice::saved::Kind::Receiver, bytes)?;
        receiver.expire(now);
        Ok(receiver)
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

    /// The number of the refresh the current epoch was opened on; 0 for
    /// the seed.
    pub fn current_number(&self) -> u32 {
        self.epochs.last().map_or(0, |epoch| epoch.keys.number())
    }

    /// The number of the newest refresh made.
    pub fn newest_number(&self) -> u32 {
        self.issued.last().map_or(0, |issued| issued.number)
    }

    fn raise(&mut self, alarm: Alarm, read_in: EpochId, now: u64) {
        self.alarms.push(alarm);
        self.alarm_epoch = Some((read_in, now));
        // R5: the epoch of the alarm is a refresh target at once, so the
        // newest refresh is queued again for the epochs it now lacks
        // (gate review 7, Kimi).
        if !self.issued.is_empty() {
            self.refresh_queued = true;
        }
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
        self.write_within(now, usize::MAX)
    }

    /// How many refresh copies this direction would write now: one for
    /// each epoch it seals in that has an address left this week and has
    /// no acknowledged copy yet.
    pub fn wants(&self, now: u64) -> usize {
        let due = self
            .issued
            .last()
            .is_none_or(|newest| now.saturating_sub(newest.made_at) >= REFRESH_EVERY_SECONDS);
        if !due && !self.refresh_queued {
            return 0;
        }
        let sealed: &[EpochId] = if due { &[] } else { &self.sealed_in };
        let this_week = week(now);
        self.refresh_targets(sealed)
            .into_iter()
            .filter(|index| self.has_refresh_address(*index, this_week))
            .count()
    }

    fn has_refresh_address(&self, index: usize, this_week: u32) -> bool {
        self.epochs.get(index).is_some_and(|epoch| {
            epoch.refresh_attempts.get(&this_week).copied().unwrap_or(0) < ATTEMPTS
        })
    }

    /// The epochs a refresh is sealed in, current first, then the one left
    /// most recently and the epoch of the most recent alarm, less those in
    /// `sealed`. An epoch with no address left this week still needs its
    /// copy, in a later week (gate review 7).
    fn refresh_targets(&self, sealed: &[EpochId]) -> Vec<usize> {
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
        targets.retain(|index| {
            self.epochs
                .get(*index)
                .is_some_and(|epoch| !sealed.contains(&epoch.keys.id()))
        });
        targets
    }

    /// Writes at most `copies` refresh copies, the current epoch first; a
    /// copy left out spends no address (protocol section 8).
    pub fn write_within(&mut self, now: u64, copies: usize) -> Vec<Notice> {
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

        let this_week = week(now);
        let required = self.refresh_targets(&self.sealed_in);
        let mut targets: Vec<usize> = required
            .iter()
            .copied()
            .filter(|index| self.has_refresh_address(*index, this_week))
            .collect();
        targets.truncate(copies);
        self.copies_left_after_write = required.len() > targets.len();
        self.written_in.clear();
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
            self.written_in.push(epoch.keys.id());
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
        self.sealed_in.clear();
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
            self.sealed_in.append(&mut self.written_in);
            if !self.copies_left_after_write {
                self.refresh_queued = false;
            }
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
