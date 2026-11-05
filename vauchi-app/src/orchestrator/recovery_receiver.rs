// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Durable recovery reception on an established, callback-scoped BLE link.
//! The native owner captures the handle at link creation and executes teardown.
//! This orchestrator does not establish native links or restore an identity.

use std::num::NonZeroUsize;
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::platform::executor::{EventInbox, InboxLimits, Operation};
use vauchi_core::sync::guardian_transfer::{HandBackReceiver, TransferError};
use vauchi_core::{Command, Event, Vauchi};

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
        if peer.is_empty() || peer.len() > 128 {
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
        })
    }

    /// Capture at native callback creation; never relabel a shared queue's events.
    pub fn callback(&self) -> Operation {
        self.callback.clone()
    }
    pub fn state(&self) -> ReceiveState {
        self.state
    }

    /// Core monotonic seconds bound an attempt to five minutes, including queued work.
    /// A refusal terminates the link; retry starts the unfinished piece afresh.
    pub fn poll(&mut self, phone: &Vauchi, now: u64) -> Vec<Command> {
        if self.state != ReceiveState::Receiving {
            return Vec::new();
        }
        let mut commands = Vec::new();
        match self.deadline {
            Some(deadline) if now >= deadline => return self.finish(ReceiveState::Failed),
            None => {
                let Some(deadline) = now.checked_add(300) else {
                    return self.finish(ReceiveState::Failed);
                };
                self.deadline = Some(deadline);
                commands.push(Command::schedule_wakeup(300, 300, 0, None));
            }
            _ => {}
        }
        for _ in 0..4 {
            let Some(event) = self.inbox.pop() else {
                break;
            };
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
                Event::HardwareError { transport, .. }
                | Event::HardwareUnavailable { transport }
                | Event::PermissionDenied { transport }
                    if transport == "BLE" =>
                {
                    return self.finish(ReceiveState::Failed);
                }
                _ => continue,
            };
            let Ok(ack) = phone.take_hand_back_frame(&mut self.receiver, data) else {
                return self.finish(ReceiveState::Failed);
            };
            let Ok(progress) = phone.storage().recovery().backup_piece_progress(&self.hash) else {
                return self.finish(ReceiveState::Failed);
            };
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
            if progress.is_some_and(|(received, count)| received == count) {
                commands.extend(self.finish(ReceiveState::Complete));
                break;
            }
        }
        commands
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
