// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link exchange sub-flow — screen builders and step logic for
//! Link mode (asynchronous relay-mediated exchange via shared URL).
//!
//! Follows the `exchange_qr.rs` pattern: steps, screen builders,
//! action handler returning an outcome for the parent engine.

use crate::ui::*;
use vauchi_core::Command;
use vauchi_core::Event;
use vauchi_core::exchange::escrow::EscrowKeys;
use vauchi_core::exchange::link_mode::{self, LinkInitiation, LinkModeError};

/// Link mode polling interval (30s per design spec, backoff to 5 min).
pub(super) const LINK_POLL_INTERVAL_MS: u32 = 30_000;

/// Steps specific to the Link exchange sub-flow.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LinkStep {
    /// Generating URL and showing share sheet.
    ShareUrl,
    /// Waiting for responder to deposit their card.
    WaitingForResponse,
    /// Retrieving and decrypting the responder's card.
    /// Entered when RelayEscrowReady event arrives (hardware event path).
    Retrieving,
}

impl LinkStep {
    pub(super) fn step_number(&self, base: u8) -> u8 {
        base + match self {
            Self::ShareUrl => 0,
            Self::WaitingForResponse => 1,
            Self::Retrieving => 2,
        }
    }

    pub(super) const STEP_COUNT: u8 = 3;
}

/// Builds the "Share Link" screen (URL ready, share sheet pending).
pub(super) fn build_share_url_screen(url: &str, progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_share_url".into(),
        title: "Share Link".into(),
        subtitle: Some("Send this link to exchange contacts".into()),
        components: vec![Component::Text {
            id: "link_url".into(),
            content: url.to_string(),
            style: TextStyle::Body,
        }],
        actions: vec![
            ScreenAction {
                id: "share".into(),
                label: "Share".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            },
            ScreenAction {
                id: "cancel".into(),
                label: "Cancel".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
        ],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Waiting for Response" screen.
pub(super) fn build_waiting_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_link_waiting".into(),
        title: "Waiting for Response".into(),
        subtitle: Some("The link has been shared. Waiting for the other person...".into()),
        components: vec![Component::StatusIndicator {
            id: "waiting_status".into(),
            icon: None,
            title: "Waiting...".into(),
            detail: Some("They need to open the link to complete the exchange.".into()),
            status: Status::InProgress,
            a11y: Some(A11y {
                label: Some("Waiting for response".into()),
                hint: Some(
                    "The other person needs to open the link to complete the exchange".into(),
                ),
                role: None,
            }),
        }],
        actions: vec![ScreenAction {
            id: "cancel".into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Retrieving" screen (fetching + decrypting).
pub(super) fn build_retrieving_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_link_retrieving".into(),
        title: "Completing Exchange".into(),
        subtitle: None,
        components: vec![Component::StatusIndicator {
            id: "retrieving_status".into(),
            icon: None,
            title: "Retrieving contact...".into(),
            detail: None,
            status: Status::InProgress,
            a11y: Some(A11y {
                label: Some("Retrieving contact".into()),
                hint: Some("Fetching and decrypting the contact card".into()),
                role: None,
            }),
        }],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Handle a user action while in a Link sub-flow step.
pub(super) fn handle_link_action(
    step: &LinkStep,
    action: &UserAction,
) -> Option<LinkActionOutcome> {
    match (step, action) {
        (LinkStep::ShareUrl, UserAction::ActionPressed { action_id }) if action_id == "share" => {
            Some(LinkActionOutcome::ShareRequested)
        }
        (LinkStep::ShareUrl, UserAction::ActionPressed { action_id }) if action_id == "cancel" => {
            Some(LinkActionOutcome::Cancelled)
        }
        (LinkStep::WaitingForResponse, UserAction::ActionPressed { action_id })
            if action_id == "cancel" =>
        {
            Some(LinkActionOutcome::Cancelled)
        }
        _ => None,
    }
}

/// Outcome of a Link sub-flow action, interpreted by the parent engine.
pub(super) enum LinkActionOutcome {
    /// User pressed "Share" — emit ShowShareSheet command.
    ShareRequested,
    /// User cancelled the link exchange.
    Cancelled,
}

/// Outcome of a Link hardware event, interpreted by the parent engine.
pub(super) enum LinkHardwareOutcome {
    /// Start polling the handshake gate for the responder's epk.
    PollHandshakeGate { commands: Vec<Command> },
    /// Handshake gate ready — retrieve responder's epk.
    RetrieveFromHandshake { commands: Vec<Command> },
    /// Responder's epk received — DH done, card deposited, polling escrow gate.
    /// Engine must store the returned `EscrowKeys` for later decryption.
    DhCompleteCardDeposited {
        commands: Vec<Command>,
        escrow_keys: EscrowKeys,
    },
    /// Escrow gate ready — retrieve responder's encrypted card.
    RetrieveFromEscrow { commands: Vec<Command> },
    /// Responder's card decrypted — exchange complete.
    /// `card_bytes` contains the deserialized contact card (saved by AppEngine).
    #[allow(dead_code)] // Used once contact saving is wired via AppEngine callback
    ExchangeComplete { card_bytes: Vec<u8> },
    /// Relay escrow failed — show error to user.
    Failed { reason: String },
}

/// Handle a hardware event during the handshake phase (before DH).
///
/// Returns `Some(outcome)` if the event was handled, `None` if ignored.
pub(super) fn handle_link_hw_event(
    li: &LinkInitiation,
    event: &Event,
) -> Option<LinkHardwareOutcome> {
    match event {
        Event::LinkShared => {
            let gate =
                hex::decode(&li.handshake_slot).expect("hex from hex::encode is always valid");
            Some(LinkHardwareOutcome::PollHandshakeGate {
                commands: vec![Command::RelayEscrowCheck {
                    gate_hash: gate,
                    suggested_interval_ms: LINK_POLL_INTERVAL_MS,
                }],
            })
        }

        Event::RelayEscrowReady { gate_hash } => {
            let hs_gate =
                hex::decode(&li.handshake_slot).expect("hex from hex::encode is always valid");
            if *gate_hash == hs_gate {
                let slot =
                    hex::decode(&li.presence_slot).expect("hex from hex::encode is always valid");
                return Some(LinkHardwareOutcome::RetrieveFromHandshake {
                    commands: vec![Command::RelayEscrowRetrieve {
                        gate_hash: gate_hash.clone(),
                        slot_hash: slot,
                    }],
                });
            }
            None
        }

        Event::RelayEscrowFailed { reason, .. } => Some(LinkHardwareOutcome::Failed {
            reason: reason.clone(),
        }),

        _ => None,
    }
}

/// Handle `LinkOpened` — the responder's epk has been retrieved.
///
/// Performs ECDH, encrypts the initiator's card, and returns commands
/// to deposit + poll the escrow gate.
pub(super) fn handle_link_opened(
    li: &LinkInitiation,
    peer_public_key: &[u8],
    card_plaintext: &[u8],
) -> Result<LinkHardwareOutcome, LinkModeError> {
    let epk: [u8; 32] =
        peer_public_key
            .try_into()
            .map_err(|_| LinkModeError::MalformedPeerKey {
                received: peer_public_key.len(),
            })?;

    let keys = link_mode::initiator_derive_keys(&li.secret_key_bytes, &epk)?;
    let encrypted_card = keys
        .encrypt_card(card_plaintext)
        .map_err(|e| LinkModeError::CardCryptoFailed(e.to_string()))?;

    let mut commands = link_mode::build_initiator_deposit(&keys, encrypted_card);

    // Immediately start polling the escrow gate (responder already deposited)
    let escrow_gate = hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid");
    commands.push(Command::RelayEscrowCheck {
        gate_hash: escrow_gate,
        suggested_interval_ms: 1_000, // 1s — user is actively waiting
    });

    Ok(LinkHardwareOutcome::DhCompleteCardDeposited {
        commands,
        escrow_keys: keys,
    })
}

/// Handle hardware events during the escrow phase (after DH, keys known).
///
/// Returns `Some(outcome)` if the event was handled, `None` if ignored.
pub(super) fn handle_escrow_hw_event(
    keys: &EscrowKeys,
    event: &Event,
) -> Option<LinkHardwareOutcome> {
    match event {
        Event::RelayEscrowReady { gate_hash } => {
            let expected =
                hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid");
            if *gate_hash == expected {
                let slot =
                    hex::decode(&keys.our_slot).expect("hex from hex::encode is always valid");
                return Some(LinkHardwareOutcome::RetrieveFromEscrow {
                    commands: vec![Command::RelayEscrowRetrieve {
                        gate_hash: gate_hash.clone(),
                        slot_hash: slot,
                    }],
                });
            }
            None
        }

        Event::RelayEscrowBlobReceived { blob, .. } => match keys.decrypt_card(blob) {
            Ok(card_bytes) => Some(LinkHardwareOutcome::ExchangeComplete { card_bytes }),
            Err(e) => Some(LinkHardwareOutcome::Failed {
                reason: format!("Card decryption failed: {e}"),
            }),
        },

        Event::RelayEscrowFailed { reason, .. } => Some(LinkHardwareOutcome::Failed {
            reason: reason.clone(),
        }),

        _ => None,
    }
}
