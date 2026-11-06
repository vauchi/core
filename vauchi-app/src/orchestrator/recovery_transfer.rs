// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Core-owned sending of one encrypted recovery piece over an established link.
//!
//! This Rust orchestrator is not a shell or binding entrypoint. The native
//! resource owner must capture `callback()` when creating the selected link,
//! execute every returned command, and stop its OS work on disconnect. Never
//! forward an old shared-queue observation through a new callback handle.
//! AppEngine ceremony routing and presentation are separate integration work.

use std::num::NonZeroUsize;
use vauchi_core::exchange::{CHAR_DATA_NOTIFY, CHAR_DATA_WRITE};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::platform::executor::{EventInbox, InboxLimits, Operation};
use vauchi_core::sync::guardian_piece::GuardianBackupPiece;
use vauchi_core::sync::guardian_transfer::{HandBackSender, TransferError};
use vauchi_core::{Command, Event};

const ACK_TIMEOUT_SECS: u32 = 5;
const MAX_RETRIES: u8 = 3;

/// Transport completion acknowledges one piece, never storage or restoration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    Sending,
    Acknowledged,
    Cancelled,
    Failed,
}

pub struct RecoveryTransferSender {
    sender: HandBackSender,
    device_id: String,
    direction: BleLinkDirection,
    inbox: EventInbox,
    callback: Operation,
    state: TransferState,
    deadline: Option<u64>,
    retries: u8,
    keep_link_after_ack: bool,
}

impl RecoveryTransferSender {
    /// Uses Core's selected, established link and its ATT MTU.
    ///
    /// # Errors
    /// Refuses an empty/oversized OS device id or an invalid ATT MTU/piece.
    pub fn new(
        piece: &GuardianBackupPiece,
        mtu: u16,
        device_id: String,
        direction: BleLinkDirection,
    ) -> Result<Self, TransferError> {
        // A peripheral cannot name its central on every OS, so an inbound link
        // has an empty id; only a dialled link must name its peer.
        let unnamed_dial = device_id.is_empty() && direction == BleLinkDirection::Outbound;
        if unnamed_dial || device_id.len() > 128 {
            return Err(TransferError::State);
        }
        let sender = HandBackSender::new(piece, mtu)?;
        let mut inbox = EventInbox::new(InboxLimits {
            operations: NonZeroUsize::new(1).unwrap(),
            events: NonZeroUsize::new(4).unwrap(),
            event_bytes: NonZeroUsize::new(4096).unwrap(),
        });
        let callback = inbox.start().map_err(|_| TransferError::State)?;
        Ok(Self {
            sender,
            device_id,
            direction,
            inbox,
            callback,
            state: TransferState::Sending,
            deadline: None,
            retries: 0,
            keep_link_after_ack: false,
        })
    }

    /// A backup spans several pieces on one link, so a piece that is not the
    /// last must leave the link up when it is acknowledged.
    pub fn keeping_link_after_ack(mut self, keep: bool) -> Self {
        self.keep_link_after_ack = keep;
        self
    }

    /// Capture this at native callback creation. Delivery failure is not success:
    /// the resource owner must retain/retry a refused event within a bound or
    /// terminate the operation. Cancellation discards queued observations too.
    pub fn callback(&self) -> Operation {
        self.callback.clone()
    }

    pub fn state(&self) -> TransferState {
        self.state
    }

    /// Drains bounded observations, emits at most one write, and owns retries.
    /// `now` is Core's monotonic time in seconds. Process returned commands
    /// before polling again; `ScheduleWakeup` requests the next deadline poll.
    pub fn poll(&mut self, now: u64) -> Vec<Command> {
        if self.state != TransferState::Sending {
            return Vec::new();
        }
        for _ in 0..4 {
            let Some(event) = self.inbox.pop() else {
                break;
            };
            if let Some(data) = self.acknowledgement(&event) {
                if self.sender.acknowledge(data).is_err() {
                    return self.finish(TransferState::Failed);
                }
                if self.sender.is_complete() {
                    return self.finish(TransferState::Acknowledged);
                }
                match self.sender.next_packet() {
                    Ok(Some(packet)) => {
                        self.retries = 0;
                        return self.write(packet, now);
                    }
                    Ok(None) => {}
                    Err(_) => return self.finish(TransferState::Failed),
                }
            } else if self.link_failed(&event) {
                return self.finish(TransferState::Failed);
            }
        }
        let Some(deadline) = self.deadline else {
            return match self.sender.next_packet() {
                Ok(Some(packet)) => self.write(packet, now),
                _ => self.finish(TransferState::Failed),
            };
        };
        if now < deadline {
            return Vec::new();
        }
        if self.retries == MAX_RETRIES {
            return self.finish(TransferState::Failed);
        }
        self.retries += 1;
        match self.sender.retry_packet() {
            Ok(packet) => self.write(packet, now),
            Err(_) => self.finish(TransferState::Failed),
        }
    }

    /// Invalidates queued/late callbacks and asks the owner to stop native work.
    /// Accepted recovery shares and durable pieces are untouched.
    pub fn cancel(&mut self) -> Vec<Command> {
        if self.state != TransferState::Sending {
            return Vec::new();
        }
        self.finish(TransferState::Cancelled)
    }

    fn acknowledgement<'a>(&self, event: &'a Event) -> Option<&'a [u8]> {
        match event {
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
            } if self.names_this_link(device_id, *direction) && uuid == self.ack_uuid() => {
                Some(data)
            }
            _ => None,
        }
    }

    /// An unnamed inbound link was reported without an id, but a shell may
    /// name the central on later events. Acknowledgements are flow control,
    /// not authentication (A11), so the direction is enough to match it.
    fn names_this_link(&self, device_id: &str, direction: BleLinkDirection) -> bool {
        direction == self.direction
            && (device_id == self.device_id
                || (self.device_id.is_empty() && direction == BleLinkDirection::Inbound))
    }

    fn ack_uuid(&self) -> &str {
        match self.direction {
            BleLinkDirection::Outbound => CHAR_DATA_NOTIFY,
            BleLinkDirection::Inbound => CHAR_DATA_WRITE,
        }
    }

    fn link_failed(&self, event: &Event) -> bool {
        match event {
            Event::BleDisconnected {
                device_id,
                direction,
                ..
            } => self.names_this_link(device_id, *direction),
            Event::HardwareError { transport, .. }
            | Event::HardwareUnavailable { transport }
            | Event::PermissionDenied { transport } => transport == "BLE",
            _ => false,
        }
    }

    fn write(&mut self, data: Vec<u8>, now: u64) -> Vec<Command> {
        let Some(deadline) = now.checked_add(u64::from(ACK_TIMEOUT_SECS)) else {
            return self.finish(TransferState::Failed);
        };
        self.deadline = Some(deadline);
        let uuid = match self.direction {
            BleLinkDirection::Outbound => CHAR_DATA_WRITE,
            BleLinkDirection::Inbound => CHAR_DATA_NOTIFY,
        };
        vec![
            Command::BleWriteCharacteristic {
                device_id: self.device_id.clone(),
                direction: self.direction,
                uuid: uuid.into(),
                data,
            },
            Command::schedule_wakeup(ACK_TIMEOUT_SECS, ACK_TIMEOUT_SECS, 0, None),
        ]
    }

    fn finish(&mut self, state: TransferState) -> Vec<Command> {
        self.state = state;
        self.callback.cancel();
        self.sender.cancel();
        if state == TransferState::Acknowledged && self.keep_link_after_ack {
            return Vec::new();
        }
        vec![Command::BleDisconnect {
            device_id: self.device_id.clone(),
            direction: self.direction,
        }]
    }
}
