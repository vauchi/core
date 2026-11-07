// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Bluetooth link of the in-person hand-back (vauchi/private#796, P33).
//!
//! The guardian shows a fresh attempt token in its QR and advertises what
//! derives from it; the phone connects only to a match. As in Glance, the
//! token is random per attempt, so a bystander cannot re-recognise a device
//! (#726). A wrong device sharing the short prefix fails harmlessly: the
//! phone accepts only frames of the backup its share names.

use std::collections::VecDeque;

use vauchi_core::exchange::{VAUCHI_BLE_SERVICE_UUID, attempt_token_from_oob, fresh_attempt_token};
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::sync::guardian_piece::GuardianBackupPiece;
use vauchi_core::{Command, Event};

use super::{AppEngine, AppScreen};
use crate::orchestrator::recovery_receiver::{ReceiveState, RecoveryTransferReceiver};
use crate::orchestrator::recovery_transfer::{
    RecoveryTransferSender, TransferState, is_ble_failure,
};
use crate::ui::{ActionResult, EngineUpdate, HandBackTransfer, RecoveryHelpUpdate, RecoveryUpdate};

const LINK_WAIT_SECS: u32 = 120;
/// `BleGattReady` is optional for a platform, so a link that never reports it
/// starts after this grace rather than waiting forever.
const READY_GRACE_SECS: u32 = 2;
const SMALLEST_MTU: u16 = 23;
const LARGEST_MTU: u16 = 517;

pub(super) enum RecoveryLink {
    Advertising {
        pieces: VecDeque<GuardianBackupPiece>,
        deadline: u64,
    },
    Connected {
        device_id: String,
        pieces: VecDeque<GuardianBackupPiece>,
        mtu: u16,
        start_at: u64,
    },
    Sending {
        device_id: String,
        sender: RecoveryTransferSender,
        rest: VecDeque<GuardianBackupPiece>,
        mtu: u16,
    },
    Seeking {
        advertised: [u8; 32],
        backup_hash: [u8; 32],
        deadline: u64,
    },
    Dialling {
        device_id: String,
        backup_hash: [u8; 32],
        deadline: u64,
    },
    Receiving(RecoveryTransferReceiver),
}

impl AppEngine {
    fn link_now(&self) -> u64 {
        self.vauchi.clock().unix_seconds()
    }

    /// Starts advertising for a guardian's hand-back; returns the token its
    /// QR carries.
    pub(super) fn begin_hand_back_link(&mut self, pieces: &[Vec<u8>]) -> Option<[u8; 32]> {
        self.cancel_recovery_link();
        let pieces = pieces
            .iter()
            .map(|piece| GuardianBackupPiece::decode(piece).ok())
            .collect::<Option<VecDeque<_>>>()?;
        let token = fresh_attempt_token();
        self.recovery_link = Some(RecoveryLink::Advertising {
            pieces,
            deadline: self.link_now() + u64::from(LINK_WAIT_SECS),
        });
        self.extend_pending_commands([
            Command::BleStartAdvertising {
                service_uuid: VAUCHI_BLE_SERVICE_UUID.into(),
                payload: attempt_token_from_oob(&token).to_vec(),
            },
            Command::schedule_wakeup(LINK_WAIT_SECS, LINK_WAIT_SECS, 0, None),
        ]);
        Some(token)
    }

    /// Starts looking for the guardian whose QR the phone just scanned.
    pub(super) fn begin_share_link(&mut self, token: &[u8; 32], backup_hash: [u8; 32]) {
        self.cancel_recovery_link();
        self.recovery_link = Some(RecoveryLink::Seeking {
            advertised: attempt_token_from_oob(token),
            backup_hash,
            deadline: self.link_now() + u64::from(LINK_WAIT_SECS),
        });
        self.extend_pending_commands([
            Command::BleStartScanning {
                service_uuid: VAUCHI_BLE_SERVICE_UUID.into(),
            },
            Command::schedule_wakeup(LINK_WAIT_SECS, LINK_WAIT_SECS, 0, None),
        ]);
        self.show_receiving(HandBackTransfer::InProgress);
    }

    /// Ends the link, stopping whatever the hardware shell is doing for it.
    pub(super) fn cancel_recovery_link(&mut self) {
        let commands = match self.recovery_link.take() {
            Some(RecoveryLink::Advertising { .. }) => vec![Command::BleStopAdvertising],
            Some(RecoveryLink::Seeking { .. }) => vec![Command::BleStopScanning],
            Some(RecoveryLink::Connected { device_id, .. }) => vec![
                Command::BleDisconnect {
                    device_id,
                    direction: BleLinkDirection::Inbound,
                },
                Command::BleStopAdvertising,
            ],
            Some(RecoveryLink::Dialling { device_id, .. }) => vec![Command::BleDisconnect {
                device_id,
                direction: BleLinkDirection::Outbound,
            }],
            Some(RecoveryLink::Sending { mut sender, .. }) => {
                let mut commands = sender.cancel();
                commands.push(Command::BleStopAdvertising);
                commands
            }
            Some(RecoveryLink::Receiving(mut receiver)) => receiver.cancel(),
            None => Vec::new(),
        };
        self.extend_pending_commands(commands);
    }

    /// The link belongs to the screen that started it. Its cached engine forgets
    /// the transfer too, so coming back shows no stale share or progress.
    pub(super) fn end_recovery_link_on_leaving(&mut self) {
        if !matches!(self.screen, AppScreen::Recovery | AppScreen::RecoveryHelp) {
            return;
        }
        self.cancel_recovery_link();
        self.engine.apply_update(EngineUpdate::RecoveryHelp(
            RecoveryHelpUpdate::HandBackTransfer(HandBackTransfer::Idle),
        ));
        self.show_receiving(HandBackTransfer::Idle);
    }

    /// The phone's cancel: the transfer stops and its late parts go nowhere.
    pub(super) fn intercept_cancel_transfer_action(&mut self) -> ActionResult {
        self.cancel_recovery_link();
        self.show_receiving(HandBackTransfer::Idle);
        ActionResult::UpdateScreen(self.engine.current_screen())
    }

    /// Consumes the link's hardware events; `None` leaves the event to the
    /// usual routing.
    pub(super) fn route_recovery_link_event(&mut self, event: &Event) -> Option<ActionResult> {
        self.recovery_link.as_ref()?;
        if !matches!(self.screen, AppScreen::Recovery | AppScreen::RecoveryHelp) {
            self.cancel_recovery_link();
            return None;
        }
        match self.recovery_link.take()? {
            RecoveryLink::Advertising { pieces, deadline } => match event {
                Event::BleConnected {
                    device_id,
                    direction: BleLinkDirection::Inbound,
                } => {
                    self.recovery_link = Some(RecoveryLink::Connected {
                        device_id: device_id.clone(),
                        pieces,
                        mtu: SMALLEST_MTU,
                        start_at: self.link_now() + u64::from(READY_GRACE_SECS),
                    });
                    // Advertising continues: on Linux, stopping it releases the
                    // GATT server this link depends on.
                    self.extend_pending_commands([Command::schedule_wakeup(
                        READY_GRACE_SECS,
                        READY_GRACE_SECS,
                        0,
                        None,
                    )]);
                    Some(self.show_sending(HandBackTransfer::InProgress))
                }
                failure if is_ble_failure(failure) => {
                    Some(self.show_sending(HandBackTransfer::Failed))
                }
                _ => {
                    self.recovery_link = Some(RecoveryLink::Advertising { pieces, deadline });
                    None
                }
            },
            RecoveryLink::Connected {
                device_id,
                pieces,
                mtu,
                start_at,
            } => match event {
                Event::BleMtuNegotiated { device_id: id, mtu }
                    if device_id.is_empty() || id == &device_id =>
                {
                    self.recovery_link = Some(RecoveryLink::Connected {
                        device_id,
                        pieces,
                        mtu: clamped_mtu(*mtu),
                        start_at,
                    });
                    Some(ActionResult::Commands {
                        commands: Vec::new(),
                    })
                }
                Event::BleGattReady { device_id: id }
                    if device_id.is_empty() || id == &device_id =>
                {
                    Some(self.start_next_piece(device_id, pieces, mtu))
                }
                Event::BleDisconnected { device_id: id, .. }
                    if device_id.is_empty() || id == &device_id =>
                {
                    Some(self.show_sending(HandBackTransfer::Failed))
                }
                failure if is_ble_failure(failure) => {
                    self.extend_pending_commands([Command::BleDisconnect {
                        device_id,
                        direction: BleLinkDirection::Inbound,
                    }]);
                    Some(self.show_sending(HandBackTransfer::Failed))
                }
                _ => {
                    self.recovery_link = Some(RecoveryLink::Connected {
                        device_id,
                        pieces,
                        mtu,
                        start_at,
                    });
                    None
                }
            },
            RecoveryLink::Sending {
                device_id,
                sender,
                rest,
                mtu,
            } => {
                // BlueZ names a peripheral's MTU only on a write, so it arrives with
                // the first acknowledgement and can only serve the next piece.
                let mtu = match event {
                    Event::BleMtuNegotiated { mtu: reported, .. } => clamped_mtu(*reported),
                    _ => mtu,
                };
                if !is_link_observation(event) {
                    self.recovery_link = Some(RecoveryLink::Sending {
                        device_id,
                        sender,
                        rest,
                        mtu,
                    });
                    return None;
                }
                if sender.callback().publish(event.clone()).is_err() {
                    return Some(self.finish_sending(sender, device_id, rest, mtu, true));
                }
                Some(self.finish_sending(sender, device_id, rest, mtu, false))
            }
            RecoveryLink::Seeking {
                advertised,
                backup_hash,
                deadline,
            } => match event {
                Event::BleDeviceDiscovered { id, adv_data, .. }
                    if adv_data.len() >= 2 && advertised.starts_with(adv_data) =>
                {
                    self.recovery_link = Some(RecoveryLink::Dialling {
                        device_id: id.clone(),
                        backup_hash,
                        deadline,
                    });
                    Some(ActionResult::Commands {
                        commands: vec![
                            Command::BleStopScanning,
                            Command::BleConnect {
                                device_id: id.clone(),
                            },
                        ],
                    })
                }
                failure if is_ble_failure(failure) => {
                    Some(self.show_receiving_result(HandBackTransfer::Failed))
                }
                _ => {
                    self.recovery_link = Some(RecoveryLink::Seeking {
                        advertised,
                        backup_hash,
                        deadline,
                    });
                    None
                }
            },
            RecoveryLink::Dialling {
                device_id,
                backup_hash,
                deadline,
            } => match event {
                Event::BleConnected {
                    device_id: id,
                    direction: BleLinkDirection::Outbound,
                } if id == &device_id => {
                    let Ok(receiver) = RecoveryTransferReceiver::new(
                        backup_hash,
                        device_id,
                        BleLinkDirection::Outbound,
                    ) else {
                        return Some(self.show_receiving_result(HandBackTransfer::Failed));
                    };
                    Some(self.poll_receiver(receiver))
                }
                Event::BleDisconnected { device_id: id, .. } if id == &device_id => {
                    Some(self.show_receiving_result(HandBackTransfer::Failed))
                }
                failure if is_ble_failure(failure) => {
                    Some(self.show_receiving_result(HandBackTransfer::Failed))
                }
                _ => {
                    self.recovery_link = Some(RecoveryLink::Dialling {
                        device_id,
                        backup_hash,
                        deadline,
                    });
                    None
                }
            },
            RecoveryLink::Receiving(receiver) => {
                if !is_link_observation(event) {
                    self.recovery_link = Some(RecoveryLink::Receiving(receiver));
                    return None;
                }
                if receiver.callback().publish(event.clone()).is_err() {
                    let mut receiver = receiver;
                    self.extend_pending_commands(receiver.cancel());
                    return Some(self.show_receiving_result(HandBackTransfer::Failed));
                }
                Some(self.poll_receiver(receiver))
            }
        }
    }

    /// Runs the link's timers on a wakeup. Returns whether the screen changed,
    /// since a wakeup otherwise renders nothing.
    pub(super) fn poll_recovery_link(&mut self) -> bool {
        let now = self.link_now();
        let Some(link) = self.recovery_link.take() else {
            return false;
        };
        let result = match link {
            RecoveryLink::Advertising { deadline, .. } if now >= deadline => {
                self.show_sending(HandBackTransfer::Failed)
            }
            RecoveryLink::Connected {
                device_id,
                pieces,
                mtu,
                start_at,
            } if now >= start_at => self.start_next_piece(device_id, pieces, mtu),
            RecoveryLink::Sending {
                device_id,
                sender,
                rest,
                mtu,
            } => self.finish_sending(sender, device_id, rest, mtu, false),
            RecoveryLink::Seeking { deadline, .. } if now >= deadline => {
                self.extend_pending_commands([Command::BleStopScanning]);
                self.show_receiving_result(HandBackTransfer::Failed)
            }
            RecoveryLink::Dialling {
                deadline,
                device_id,
                ..
            } if now >= deadline => {
                self.extend_pending_commands([Command::BleDisconnect {
                    device_id,
                    direction: BleLinkDirection::Outbound,
                }]);
                self.show_receiving_result(HandBackTransfer::Failed)
            }
            RecoveryLink::Receiving(receiver) => self.poll_receiver(receiver),
            waiting => {
                self.recovery_link = Some(waiting);
                return false;
            }
        };
        matches!(result, ActionResult::UpdateScreen(_))
    }

    fn start_next_piece(
        &mut self,
        device_id: String,
        mut pieces: VecDeque<GuardianBackupPiece>,
        mtu: u16,
    ) -> ActionResult {
        let Some(piece) = pieces.pop_front() else {
            return self.show_sending(HandBackTransfer::Failed);
        };
        let sender =
            RecoveryTransferSender::new(&piece, mtu, device_id.clone(), BleLinkDirection::Inbound);
        let Ok(sender) = sender else {
            return self.show_sending(HandBackTransfer::Failed);
        };
        let sender = sender.keeping_link_after_ack(!pieces.is_empty());
        self.finish_sending(sender, device_id, pieces, mtu, false)
    }

    fn finish_sending(
        &mut self,
        mut sender: RecoveryTransferSender,
        device_id: String,
        rest: VecDeque<GuardianBackupPiece>,
        mtu: u16,
        refused: bool,
    ) -> ActionResult {
        if refused {
            self.extend_pending_commands(sender.cancel());
            return self.show_sending(HandBackTransfer::Failed);
        }
        let commands = sender.poll(self.link_now());
        self.extend_pending_commands(commands);
        match sender.state() {
            TransferState::Sending => {
                self.recovery_link = Some(RecoveryLink::Sending {
                    device_id,
                    sender,
                    rest,
                    mtu,
                });
                ActionResult::Commands {
                    commands: Vec::new(),
                }
            }
            TransferState::Acknowledged if !rest.is_empty() => {
                self.start_next_piece(device_id, rest, mtu)
            }
            TransferState::Acknowledged => self.show_sending(HandBackTransfer::Finished),
            TransferState::Failed | TransferState::Cancelled => {
                self.show_sending(HandBackTransfer::Failed)
            }
        }
    }

    fn poll_receiver(&mut self, mut receiver: RecoveryTransferReceiver) -> ActionResult {
        let commands = receiver.poll(&self.vauchi, self.link_now());
        self.extend_pending_commands(commands);
        match receiver.state() {
            ReceiveState::Receiving => {
                self.recovery_link = Some(RecoveryLink::Receiving(receiver));
                ActionResult::Commands {
                    commands: Vec::new(),
                }
            }
            ReceiveState::Complete => {
                self.show_receiving(HandBackTransfer::Finished);
                self.offer_restore_when_ready();
                ActionResult::UpdateScreen(self.engine.current_screen())
            }
            ReceiveState::Failed | ReceiveState::Cancelled => {
                self.show_receiving_result(HandBackTransfer::Failed)
            }
        }
    }

    /// Ending the guardian's side withdraws its advertisement, which is also
    /// what ends an inbound link on a shell that cannot disconnect one.
    fn show_sending(&mut self, transfer: HandBackTransfer) -> ActionResult {
        if matches!(
            transfer,
            HandBackTransfer::Finished | HandBackTransfer::Failed
        ) {
            self.extend_pending_commands([Command::BleStopAdvertising]);
        }
        self.engine.apply_update(EngineUpdate::RecoveryHelp(
            RecoveryHelpUpdate::HandBackTransfer(transfer),
        ));
        ActionResult::UpdateScreen(self.engine.current_screen())
    }

    fn show_receiving(&mut self, transfer: HandBackTransfer) {
        self.engine
            .apply_update(EngineUpdate::Recovery(RecoveryUpdate::BackupTransfer(
                transfer,
            )));
    }

    fn show_receiving_result(&mut self, transfer: HandBackTransfer) -> ActionResult {
        self.show_receiving(transfer);
        ActionResult::UpdateScreen(self.engine.current_screen())
    }

    /// Moves the phone to its restore step once its store allows an attempt.
    pub(super) fn offer_restore_when_ready(&mut self) {
        if let Ok(Some(status)) = self.vauchi.hand_back_restore_status()
            && status.can_attempt_restore
        {
            self.engine.apply_update(EngineUpdate::Recovery(
                RecoveryUpdate::HandBackRestoreReady {
                    handed_back: status.handed_back,
                    needs_passphrase: status.needs_passphrase,
                },
            ));
        }
    }
}

fn clamped_mtu(reported: u32) -> u16 {
    u16::try_from(reported)
        .unwrap_or(LARGEST_MTU)
        .clamp(SMALLEST_MTU, LARGEST_MTU)
}

fn is_link_observation(event: &Event) -> bool {
    matches!(
        event,
        Event::BleCharacteristicRead { .. }
            | Event::BleCharacteristicNotified { .. }
            | Event::BleDisconnected { .. }
    ) || is_ble_failure(event)
}
