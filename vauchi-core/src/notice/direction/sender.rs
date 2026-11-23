// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The sender of one direction: it follows names, opens and writes.

use std::collections::{BTreeMap, VecDeque};

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::*;
use crate::crypto::X25519KeyPair;
use crate::notice::chain::{ChainWriter, PERIOD_SECONDS};
use crate::notice::keys::{
    Epoch, Keyed, Label, key_id, open_as_sender, opening_address, proof_tag, refresh_address,
};
use crate::notice::record::Record;
use crate::notice::seal::{TAG_BYTES, open, seal};

/// One group's latest update in this direction.
#[derive(serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "testing", derive(Clone))]
struct GroupUpdate {
    update_key: Zeroizing<[u8; 32]>,
    commitment: [u8; 31],
    /// Counts edits, so that an acknowledgement clears only the edit the
    /// acknowledged write carried.
    edit: u64,
    queued: bool,
    /// Its key went out since the view was last due.
    viewed: bool,
    /// When a notice of it was last written, acknowledged or not: the
    /// group written longest ago goes first, updates and views alike, so
    /// no group is starved by the cap of positions a period (gate reviews
    /// 8 and 9, Codex).
    written_at: Option<u64>,
}

#[cfg_attr(feature = "testing", derive(Clone))]
#[derive(serde::Serialize, serde::Deserialize)]
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

#[cfg_attr(feature = "testing", derive(Clone))]
#[derive(serde::Serialize, serde::Deserialize)]
struct RefreshRead {
    number: u32,
    public: [u8; 32],
    named: Option<EpochId>,
    opened: bool,
    read_at: u64,
}

#[cfg_attr(feature = "testing", derive(Clone))]
#[derive(serde::Serialize, serde::Deserialize)]
struct OpeningQueued {
    number: u32,
    base: EpochId,
    epoch: EpochId,
    ephemeral_public: [u8; 32],
    key_id: [u8; 8],
    confirmation: [u8; 16],
    /// No address is left for it under its number in its base.
    spent: bool,
    /// When it was last written: openings and views share a direction's
    /// control slots by how long each has waited (owner, 2026-11-23, gate
    /// review 10).
    written_at: Option<u64>,
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

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Sender {
    label: Label,
    #[serde(with = "crate::notice::saved::key_pair")]
    device: X25519KeyPair,
    receiver_device_public: [u8; 32],
    epochs: Vec<SenderEpoch>,
    writes_in: EpochId,
    highest_named: u32,
    erased: VecDeque<EpochId>,
    refreshes: Vec<RefreshRead>,
    openings: Vec<OpeningQueued>,
    /// Each group's latest update, by group ID (S5, section 8).
    groups: BTreeMap<GroupId, GroupUpdate>,
    view_due: bool,
    /// The groups, edit numbers and time of this session's notices.
    written_groups: Vec<(GroupId, u64, u64)>,
    /// What the last write wrote, by queue, so a session counts a turn
    /// only for the queue served (gate review 11, Codex).
    #[serde(skip)]
    last_written: Wants,
    positions_assigned_at: VecDeque<u64>,
    highest_opened_this_session: Option<u32>,
    first_records: FirstRecords,
    alarms: Vec<Alarm>,
}

#[cfg(feature = "testing")]
impl Clone for Sender {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            device: copy_key(&self.device),
            receiver_device_public: self.receiver_device_public,
            epochs: self.epochs.clone(),
            writes_in: self.writes_in,
            highest_named: self.highest_named,
            erased: self.erased.clone(),
            refreshes: self.refreshes.clone(),
            openings: self.openings.clone(),
            groups: self.groups.clone(),
            view_due: self.view_due,
            written_groups: self.written_groups.clone(),
            last_written: self.last_written,
            positions_assigned_at: self.positions_assigned_at.clone(),
            highest_opened_this_session: self.highest_opened_this_session,
            first_records: self.first_records.clone(),
            alarms: self.alarms.clone(),
        }
    }
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
            groups: BTreeMap::new(),
            view_due: false,
            written_groups: Vec::new(),
            last_written: Wants::default(),
            positions_assigned_at: VecDeque::new(),
            highest_opened_this_session: None,
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

    /// The number of the epoch update keys are written in.
    pub fn writes_in_number(&self) -> u32 {
        self.epochs
            .iter()
            .find(|epoch| epoch.keys.id() == self.writes_in)
            .map_or(0, |epoch| epoch.keys.number())
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

    /// The newest update of a direction with one group (group 0).
    pub fn queue_update(&mut self, update_key: &[u8; 32], commitment: &[u8; 31]) {
        let queued = self.queue_group_update(0, update_key, commitment);
        debug_assert!(queued.is_ok(), "group 0 alone never reaches the cap");
    }

    /// The newest update of one of the contact's groups; it replaces one
    /// still queued for that group, and the replaced key is erased.
    pub fn queue_group_update(
        &mut self,
        group: GroupId,
        update_key: &[u8; 32],
        commitment: &[u8; 31],
    ) -> Result<(), TooManyGroups> {
        if !self.groups.contains_key(&group) && self.groups.len() >= GROUPS_A_CONTACT {
            return Err(TooManyGroups);
        }
        let entry = self.groups.entry(group).or_insert_with(|| GroupUpdate {
            update_key: Zeroizing::new(*update_key),
            commitment: *commitment,
            edit: 0,
            queued: false,
            viewed: false,
            written_at: None,
        });
        entry.update_key = Zeroizing::new(*update_key);
        entry.commitment = *commitment;
        entry.edit = entry.edit.saturating_add(1);
        entry.queued = true;
        Ok(())
    }

    /// The contact left `group`: its latest key is erased from this
    /// direction, no view carries it, and its place under the cap of 32 is
    /// free (gate review 10, Claude).
    pub fn leave_group(&mut self, group: GroupId) {
        self.groups.remove(&group);
        self.written_groups
            .retain(|(written, _, _)| *written != group);
    }

    fn view_is_due(&mut self) {
        self.view_due = true;
        for group in self.groups.values_mut() {
            group.viewed = false;
        }
    }

    /// Reads one session's notices: refreshes at the addresses of every
    /// epoch it keeps. Those it already knows cost nothing; of the rest it
    /// handles the eight with the highest numbers, in rising order, and
    /// leaves the others unread so that a later delivery can still be
    /// read.
    pub fn read(&mut self, notices: &[Notice], now: u64) {
        // Epochs are erased after the session's refreshes are handled
        // (protocol section 7, step 3): a late refresh may still name one
        // on the day its three periods end.
        self.forget(now);
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
        self.highest_opened_this_session = None;
        for refresh in new.into_iter().skip(skip) {
            self.first_records.remember(
                refresh.tag,
                &refresh.notice,
                now,
                Some(refresh.address_week),
            );
            self.handle(&refresh, now);
        }
        // Once per session, after every refresh is handled (protocol
        // section 7, step 3): a refresh later in the session may have been
        // sealed in an epoch an earlier one would make unreachable.
        let opened = self.highest_opened_this_session.take();
        // Repeated until nothing more is erased: an opening whose epoch is
        // erased leaves the queue, and its base is then no longer kept for
        // it in the same step (protocol section 7, step 3).
        loop {
            let kept = self.epochs.len();
            if let Some(number) = opened {
                self.erase_unreachable(number);
            }
            self.retain(now);
            if self.epochs.len() == kept {
                break;
            }
        }
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
        // Records already opened in this batch, by address: a second,
        // different one at an address alarms even before the first is
        // remembered, and a refresh the session then leaves unread still
        // keeps no hash.
        let mut in_batch: BTreeMap<[u8; TAG_BYTES], [u8; 32]> = BTreeMap::new();
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
                Seen::First => match in_batch.get(&tag) {
                    Some(earlier) if *earlier == record_hash(notice) => continue,
                    Some(_) => self.alarms.push(Alarm::TwoRecordsAtOneAddress),
                    None => {
                        in_batch.insert(tag, record_hash(notice));
                    }
                },
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
                    self.view_is_due();
                    self.openings.retain(|opening| opening.epoch != id);
                }
            }
            None => {
                self.alarms.push(Alarm::ForeignName);
                self.writes_in = refresh.read_in;
                self.view_is_due();
            }
        }

        if !opened_before && self.open_on(refresh, read_in, now) {
            self.highest_opened_this_session =
                self.highest_opened_this_session.max(Some(refresh.number));
        }
        self.refreshes.push(RefreshRead {
            number: refresh.number,
            public: refresh.public,
            named,
            opened: true,
            read_at: now,
        });
    }

    /// When the receiver made refresh `number` it had either moved past
    /// every epoch numbered three or more below, or held three newer
    /// unused keys, which displaced the key such an epoch was opened on.
    /// An epoch still unnamed that low can never become the receiver's.
    fn erase_unreachable(&mut self, number: u32) {
        let Some(below) = number.checked_sub(REFRESH_KEYS_HELD_BY_RECEIVER) else {
            return;
        };
        let writes_in = self.writes_in;
        // The receiver may seal its refreshes in an epoch it can no longer
        // move into: the one it left most recently. An opening queued on
        // that base must still go out, so the base waits for it (at most
        // four sessions).
        let bases = self.opening_bases();
        let unreachable: Vec<EpochId> = self
            .epochs
            .iter()
            .filter(|e| {
                e.named_at.is_none()
                    && e.keys.number() <= below
                    && e.keys.id() != writes_in
                    && !bases.contains(&e.keys.id())
            })
            .map(|e| e.keys.id())
            .collect();
        for id in unreachable {
            self.erase(id);
        }
    }

    /// Whether an epoch was opened: an invalid Diffie-Hellman result opens
    /// none, and then the m - 3 rule has nothing to rest on.
    fn open_on(&mut self, refresh: &RefreshFound, read_in: usize, now: u64) -> bool {
        let Some(base) = self.epochs.get(read_in) else {
            return false;
        };
        // Specification S4: an epoch whose opening has no address left in
        // this base could never be sent.
        if base
            .opening_attempts
            .get(&refresh.number)
            .copied()
            .unwrap_or(0)
            >= ATTEMPTS
        {
            return false;
        }
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
            return false;
        };
        self.openings.push(OpeningQueued {
            number: refresh.number,
            base: refresh.read_in,
            epoch: opened.epoch.id(),
            ephemeral_public: *ephemeral.public_key(),
            key_id: key_id(&refresh.public),
            confirmation: opened.confirmation,
            spent: false,
            written_at: None,
        });
        self.epochs.push(SenderEpoch::new(opened.epoch, now, None));
        true
    }

    /// Writes the openings not yet named, each at its next address until
    /// its fourth, and the newest update key when a refresh was followed
    /// or an edit is waiting.
    pub fn write(&mut self, now: u64) -> Vec<Notice> {
        self.write_within(now, usize::MAX, usize::MAX)
    }

    /// This direction's state as saved between sessions (section 5).
    pub fn save(&self) -> Result<Zeroizing<Vec<u8>>, crate::notice::saved::SavedError> {
        crate::notice::saved::save(crate::notice::saved::Kind::Sender, self)
    }

    /// Loads saved state. S holds no notice key but its newest update key
    /// and erases epochs after a session's refreshes (S4), so loading
    /// erases nothing.
    pub fn load(bytes: &[u8], _now: u64) -> Result<Self, crate::notice::saved::SavedError> {
        crate::notice::saved::load(crate::notice::saved::Kind::Sender, bytes)
    }

    /// The notice chains a copy of this state can read from here on, one
    /// for each kept epoch that still writes (the tests of P3).
    #[cfg(feature = "testing")]
    pub fn copied_chain_readers(&self) -> Vec<crate::notice::chain::ChainReader> {
        self.epochs
            .iter()
            .filter_map(|epoch| epoch.writer.as_ref().map(ChainWriter::reader_from_here))
            .collect()
    }

    /// What this direction would write now: its update-key notice when an
    /// edit is queued, else a view due, and its queued openings.
    pub fn wants(&self, now: u64) -> Wants {
        self.wants_for(now, |_| true)
    }

    /// As `wants`, counting an update only for a group `eligible` allows:
    /// a new edit's notice goes only with its record 0 (section 4).
    pub fn wants_for(&self, now: u64, eligible: impl Fn(GroupId) -> bool) -> Wants {
        let recent = self
            .positions_assigned_at
            .iter()
            .filter(|at| now.saturating_sub(**at) < PERIOD_SECONDS)
            .count();
        let room = recent < POSITIONS_A_PERIOD
            && self
                .epochs
                .iter()
                .find(|epoch| epoch.keys.id() == self.writes_in)
                .and_then(|epoch| epoch.writer.as_ref())
                .is_some_and(ChainWriter::has_room);
        if !room {
            return Wants {
                update: 0,
                control: self.openings.len(),
            };
        }
        // Updates and views share the positions left in the period, in
        // one queue.
        let positions_left = POSITIONS_A_PERIOD.saturating_sub(recent);
        let chosen: Vec<(GroupId, bool)> = self
            .notice_queue(&eligible)
            .into_iter()
            .take(positions_left)
            .collect();
        let update = chosen.iter().filter(|(_, is_update)| *is_update).count();
        Wants {
            update,
            control: self.openings.len().saturating_add(chosen.len() - update),
        }
    }

    /// Groups whose view is still to go this round: not viewed, and with
    /// no update queued (a queued update carries the view).
    fn view_candidates(&self) -> impl Iterator<Item = GroupId> + '_ {
        self.groups
            .iter()
            .filter(|(_, entry)| self.view_due && !entry.viewed && !entry.queued)
            .map(|(group, _)| *group)
    }

    /// The notices this direction would write, updates (`true`) and views
    /// (`false`) in one queue: the group written longest ago first.
    fn notice_queue(&self, eligible: &impl Fn(GroupId) -> bool) -> Vec<(GroupId, bool)> {
        let mut queue: Vec<(GroupId, bool)> = self
            .groups
            .iter()
            .filter(|(group, entry)| entry.queued && eligible(**group))
            .map(|(group, _)| (*group, true))
            .chain(self.view_candidates().map(|group| (group, false)))
            .collect();
        queue.sort_by_key(|(group, _)| {
            (
                self.groups.get(group).and_then(|entry| entry.written_at),
                *group,
            )
        });
        queue
    }

    /// Writes at most `control_slots` control records (openings and views,
    /// by how long each has waited) and `update_slots` update notices. A record left out stays
    /// queued and advances no counter (protocol section 8).
    pub fn write_within(
        &mut self,
        now: u64,
        control_slots: usize,
        update_slots: usize,
    ) -> Vec<Notice> {
        self.write_within_for(now, control_slots, update_slots, |_| true)
    }

    /// As `write_within`, writing an update only for a group `eligible`
    /// allows; an update waiting for its record 0 keeps its group's view.
    pub fn write_within_for(
        &mut self,
        now: u64,
        control_slots: usize,
        update_slots: usize,
        eligible: impl Fn(GroupId) -> bool,
    ) -> Vec<Notice> {
        self.retain(now);
        self.written_groups.clear();
        let mut control_left = control_slots;
        let mut written = Vec::new();
        // Newest first: openings under one number in one base share its
        // four addresses, and the newest key gets the last one left, so a
        // forged key queued earlier cannot use up R's own (spec S4; gate
        // review 4, Claude).
        // Openings and views share the control slots, the one that went out
        // longest ago first (never before ever), an opening first on a tie.
        let mut waiting: Vec<(Option<u64>, bool)> = self
            .openings
            .iter()
            .map(|opening| (opening.written_at, true))
            .chain(
                self.notice_queue(&eligible)
                    .into_iter()
                    .filter(|(_, is_update)| !is_update)
                    // Only views the period's positions can still take
                    // compete for a control slot (gate review 11, Claude).
                    .take(POSITIONS_A_PERIOD.saturating_sub(self.positions_assigned_at.len()))
                    .map(|(group, _)| {
                        (
                            self.groups.get(&group).and_then(|entry| entry.written_at),
                            false,
                        )
                    }),
            )
            .collect();
        waiting.sort_by_key(|(written_at, is_opening)| (*written_at, !*is_opening));
        let openings_allowed = waiting
            .iter()
            .take(control_slots)
            .filter(|(_, is_opening)| *is_opening)
            .count();
        let mut openings_written = 0usize;
        // The openings themselves in waiting order, the newest first on a
        // tie, so that the ones chosen are the ones that waited (gate review
        // 11, Codex) and a second key under a number still gets the last
        // address left (spec S4).
        let mut opening_order: Vec<usize> = (0..self.openings.len()).collect();
        opening_order
            .sort_by_key(|index| (self.openings[*index].written_at, std::cmp::Reverse(*index)));
        for index in opening_order {
            if control_left == 0 || openings_written >= openings_allowed {
                break;
            }
            let opening = &mut self.openings[index];
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
            // After its fourth attempt the opening leaves the queue at once
            // (protocol section 8), so its base is no longer held for it.
            if *attempt >= ATTEMPTS {
                opening.spent = true;
            }
            let record = Record::Opening {
                ephemeral_public: opening.ephemeral_public,
                number: opening.number,
                key_id: opening.key_id,
                confirmation: opening.confirmation,
            };
            written.push(seal(&at, &record.to_plaintext()));
            opening.written_at = Some(now);
            openings_written += 1;
            control_left -= 1;
        }
        self.openings.retain(|opening| !opening.spent);
        let mut update_left = update_slots;
        let mut written_updates = 0usize;
        let mut written_views = 0usize;
        let chosen = self.notice_queue(&eligible);
        for (group, is_update) in chosen {
            let slot_left = if is_update { update_left } else { control_left };
            if slot_left == 0
                || self.positions_assigned_at.len() >= POSITIONS_A_PERIOD
                || !self.groups.contains_key(&group)
            {
                continue;
            }
            let writes_in = self.writes_in;
            let epoch = self.epochs.iter_mut().find(|e| e.keys.id() == writes_in);
            let writer = epoch.and_then(|epoch| epoch.writer.as_mut());
            // A position is assigned only for a notice that is written; an
            // ended chain writes nothing more, and the next epoch the
            // receiver names carries the update.
            let Some((_, at)) = writer.and_then(ChainWriter::assign) else {
                break;
            };
            self.positions_assigned_at.push_back(now);
            let Some(entry) = self.groups.get_mut(&group) else {
                continue;
            };
            entry.written_at = Some(now);
            let record = Record::UpdateKey {
                update_key: entry.update_key.clone(),
                commitment: entry.commitment,
            };
            written.push(seal(&at, &record.to_plaintext()));
            self.written_groups.push((group, entry.edit, now));
            if is_update {
                update_left -= 1;
                written_updates += 1;
            } else {
                control_left -= 1;
                written_views += 1;
            }
        }
        self.last_written = Wants {
            update: written_updates,
            control: openings_written + written_views,
        };
        written
    }

    /// What the last write wrote, by queue.
    pub fn last_written(&self) -> Wants {
        self.last_written
    }

    /// The relay took the last write.
    pub fn acknowledge(&mut self) {
        for (group, edit, _) in self.written_groups.drain(..) {
            if let Some(entry) = self.groups.get_mut(&group) {
                entry.viewed = true;
                if entry.edit == edit {
                    entry.queued = false;
                }
            }
        }
        if self.groups.values().all(|entry| entry.viewed) {
            self.view_due = false;
        }
    }

    /// The epochs a queued opening is to be sealed in. Each waits for its
    /// openings, which leave the queue after four attempts or a naming.
    fn opening_bases(&self) -> Vec<EpochId> {
        self.openings.iter().map(|opening| opening.base).collect()
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
        let bases = self.opening_bases();
        let mut to_erase = Vec::new();
        for epoch in &mut self.epochs {
            let id = epoch.keys.id();
            if id == writes_in || bases.contains(&id) {
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
                .filter(|e| !bases.contains(&e.keys.id()))
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
                .filter(|e| {
                    e.named_at.is_none()
                        && e.keys.id() != writes_in
                        && e.keys.number() < highest_named
                        && !bases.contains(&e.keys.id())
                })
                .map(|e| (e.keys.number(), e.keys.id()))
                .min();
            match lowest {
                Some((_, id)) => self.erase(id),
                None => break,
            }
        }

        self.forget(now);
    }

    /// What lapses with time alone: the refreshes remembered, the
    /// positions counted against the period's cap, the first-record
    /// hashes of addresses no longer watched.
    fn forget(&mut self, now: u64) {
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
