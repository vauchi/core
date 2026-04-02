// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link exchange sub-flow — screen builders and step logic for
//! Link mode (asynchronous relay-mediated exchange via shared URL).
//!
//! Follows the `exchange_qr.rs` pattern: steps, screen builders,
//! action handler returning an outcome for the parent engine.

use crate::ui::*;
use vauchi_core::exchange::ExchangeHardwareEvent;
use vauchi_core::exchange::command::ExchangeCommand;
use vauchi_core::exchange::link_mode::LinkInitiation;

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
            },
            ScreenAction {
                id: "cancel".into(),
                label: "Cancel".into(),
                style: ActionStyle::Secondary,
                enabled: true,
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
        }],
        actions: vec![ScreenAction {
            id: "cancel".into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
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
    PollHandshakeGate { commands: Vec<ExchangeCommand> },
    /// Handshake gate ready — retrieve responder's epk.
    RetrieveFromHandshake { commands: Vec<ExchangeCommand> },
    /// Relay escrow failed — show error to user.
    Failed { reason: String },
}

/// Handle a hardware event during Link mode.
///
/// Returns `Some(outcome)` if the event was handled, `None` if ignored.
/// The parent engine interprets the outcome (state transitions, screen updates).
pub(super) fn handle_link_hw_event(
    li: &LinkInitiation,
    event: &ExchangeHardwareEvent,
) -> Option<LinkHardwareOutcome> {
    match event {
        ExchangeHardwareEvent::LinkShared => {
            let gate =
                hex::decode(&li.handshake_slot).expect("hex from hex::encode is always valid");
            Some(LinkHardwareOutcome::PollHandshakeGate {
                commands: vec![ExchangeCommand::RelayEscrowCheck {
                    gate_hash: gate,
                    suggested_interval_ms: LINK_POLL_INTERVAL_MS,
                }],
            })
        }

        ExchangeHardwareEvent::RelayEscrowReady { gate_hash } => {
            let hs_gate =
                hex::decode(&li.handshake_slot).expect("hex from hex::encode is always valid");
            if *gate_hash == hs_gate {
                // Handshake gate ready — retrieve responder's epk.
                // GET authenticates with our presence_slot and returns
                // the OTHER slot's blob (responder's epk).
                let slot =
                    hex::decode(&li.presence_slot).expect("hex from hex::encode is always valid");
                return Some(LinkHardwareOutcome::RetrieveFromHandshake {
                    commands: vec![ExchangeCommand::RelayEscrowRetrieve {
                        gate_hash: gate_hash.clone(),
                        slot_hash: slot,
                    }],
                });
            }
            // Escrow gate ready — handled after initiator_complete when escrow_keys are set
            None
        }

        ExchangeHardwareEvent::RelayEscrowFailed { reason, .. } => {
            Some(LinkHardwareOutcome::Failed {
                reason: reason.clone(),
            })
        }

        _ => None,
    }
}
