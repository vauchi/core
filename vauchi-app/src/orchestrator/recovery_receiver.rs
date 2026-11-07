// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Durable recovery reception on an established, callback-scoped BLE link.
//! The native owner captures the handle at link creation and executes teardown.
//! This orchestrator does not establish native links or restore an identity.

use super::recovery_transfer::is_ble_failure;
use std::num::NonZeroUsize;
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::platform::executor::{EventInbox, InboxLimits, Operation};
use vauchi_core::sync::guardian_transfer::{HandBackReceiver, TransferError};
use vauchi_core::{Command, Event, Vauchi};

const SILENCE_LIMIT_SECS: u32 = 300;
/// An `RBA1` acknowledgement's piece index and end offset, both big-endian, so
/// comparing the bytes orders positions through the backup.
const ACK_POSITION: std::ops::Range<usize> = 4..8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveState {
    Receiving,
    Complete,
    Cancelled,
    Failed,
}

pub struct RecoveryTransferReceiver {
    receiver: HandBackReceiver,
    hash: [u8; 32],
    peer: String,
    direction: BleLinkDirection,
    inbox: EventInbox,
    callback: Operation,
    state: ReceiveState,
    deadline: Option<u64>,
    furthest: Option<[u8; 4]>,
}

impl RecoveryTransferReceiver {
    /// The durable API checks active recovery and an accepted share on every frame.
    /// # Errors
    /// Refuses empty or oversized OS link identifiers.
    pub fn new(
        hash: [u8; 32],
        peer: String,
        direction: BleLinkDirection,
    ) -> Result<Self, TransferError> {
        // As for the sender: an inbound link has an empty id on some OSes.
        let unnamed_dial = peer.is_empty() && direction == BleLinkDirection::Outbound;
        if unnamed_dial || peer.len() > 128 {
            return Err(TransferError::State);
        }
        let mut inbox = EventInbox::new(InboxLimits {
            operations: NonZeroUsize::new(1).unwrap(),
            events: NonZeroUsize::new(4).unwrap(),
            event_bytes: NonZeroUsize::new(4096).unwrap(),
        });
        let callback = inbox.start().map_err(|_| TransferError::State)?;
        Ok(Self {
            receiver: HandBackReceiver::new(hash),
            hash,
            peer,
            direction,
            inbox,
            callback,
            state: ReceiveState::Receiving,
            deadline: None,
            furthest: None,
        })
    }

    /// Capture at native callback creation; never relabel a shared queue's events.
    pub fn callback(&self) -> Operation {
        self.callback.clone()
    }
    pub fn state(&self) -> ReceiveState {
        self.state
    }

    /// Core monotonic seconds end an attempt after five minutes without forward
    /// progress, including queued work. A whole attempt has no limit, because a
    /// piece at the smallest MTU can take minutes and restarts if cut short. A
    /// refusal terminates the link; retry starts the unfinished piece afresh.
    pub fn poll(&mut self, phone: &Vauchi, now: u64) -> Vec<Command> {
        if self.state != ReceiveState::Receiving {
            return Vec::new();
        }
        let mut commands = Vec::new();
        match self.deadline {
            Some(deadline) if now >= deadline => return self.finish(ReceiveState::Failed),
            None => {
                let Some(deadline) = now.checked_add(u64::from(SILENCE_LIMIT_SECS)) else {
                    return self.finish(ReceiveState::Failed);
                };
                self.deadline = Some(deadline);
                commands.push(Command::schedule_wakeup(
                    SILENCE_LIMIT_SECS,
                    SILENCE_LIMIT_SECS,
                    0,
                    None,
                ));
            }
            _ => {}
        }
        let mut observed = false;
        for _ in 0..4 {
            let Some(event) = self.inbox.pop() else {
                break;
            };
            observed = true;
            let data = match &event {
                Event::BleCharacteristicRead {
                    device_id,
                    direction,
                    uuid,
                    data,
                }
                | Event::BleCharacteristicNotified {
                    device_id,
                    direction,
                    uuid,
                    data,
                } if device_id == &self.peer
                    && direction == &self.direction
                    && uuid
                        == match self.direction {
                            BleLinkDirection::Outbound => CHAR_DATA_NOTIFY,
                            BleLinkDirection::Inbound => CHAR_DATA_WRITE,
                        } =>
                {
                    data
                }
                Event::BleDisconnected {
                    device_id,
                    direction,
                    ..
                } if device_id == &self.peer && direction == &self.direction => {
                    return self.finish(ReceiveState::Failed);
                }
                failure if is_ble_failure(failure) => {
                    return self.finish(ReceiveState::Failed);
                }
                _ => continue,
            };
            let Ok(ack) = phone.take_hand_back_frame(&mut self.receiver, data) else {
                return self.finish(ReceiveState::Failed);
            };
            self.note_progress(&ack, now);
            let Ok(progress) = phone.storage().recovery().backup_piece_progress(&self.hash) else {
                return self.finish(ReceiveState::Failed);
            };
            // A11: stored progress may predate this replay; finish only at its final frame.
            let final_frame = ack.get(6..8) == data.get(8..10);
            commands.push(Command::BleWriteCharacteristic {
                device_id: self.peer.clone(),
                direction: self.direction,
                uuid: match self.direction {
                    BleLinkDirection::Outbound => CHAR_DATA_WRITE,
                    BleLinkDirection::Inbound => CHAR_DATA_NOTIFY,
                }
                .into(),
                data: ack,
            });
            if final_frame && progress.is_some_and(|(received, count)| received == count) {
                commands.extend(self.finish(ReceiveState::Complete));
                break;
            }
        }
        // The timer armed earlier fires before a limit that progress moved.
        if !observed
            && commands.is_empty()
            && let Some(left) = self.deadline.map(|deadline| deadline.saturating_sub(now))
        {
            let left = u32::try_from(left).unwrap_or(SILENCE_LIMIT_SECS);
            commands.push(Command::schedule_wakeup(left, left, 0, None));
        }
        commands
    }

    /// Only a new furthest position counts: a replay is answered but moves
    /// nothing, or a peer could hold the link open for ever.
    fn note_progress(&mut self, ack: &[u8], now: u64) {
        let Some(position) = ack
            .get(ACK_POSITION)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        else {
            return;
        };
        if self.furthest.is_none_or(|furthest| position > furthest) {
            self.furthest = Some(position);
            self.deadline = now.checked_add(u64::from(SILENCE_LIMIT_SECS));
        }
    }

    pub fn cancel(&mut self) -> Vec<Command> {
        if self.state != ReceiveState::Receiving {
            return Vec::new();
        }
        self.finish(ReceiveState::Cancelled)
    }

    fn finish(&mut self, state: ReceiveState) -> Vec<Command> {
        self.state = state;
        self.callback.cancel();
        self.receiver.cancel();
        vec![Command::BleDisconnect {
            device_id: self.peer.clone(),
            direction: self.direction,
        }]
    }
}
