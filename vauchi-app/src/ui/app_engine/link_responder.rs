// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link-mode responder lifecycle on `AppEngine` (slice 32l Phase 2).
//!
//! The `AppEngine` owns the pure-core `LinkResponderSession` state
//! machine while the active screen is `AppScreen::DeepLinkResponder`,
//! replacing the retired `vauchi-platform` cycle-thread wrapper. The
//! machine is built on entry (its `RelayEscrowDeposit`/`Check` commands
//! flow out through the `pending_commands` queue), driven by
//! `RelayEscrow*` hardware events via `handle_hardware_event` (follow-up
//! commands surface as `ActionResult::Commands`), and dropped on exit.
//! On `Finalized` core decodes + persists the received card via
//! `import_received_link_card`; on `Failed` it renders
//! `link_responder_failed`. Frontends pull no session object and own no
//! workflow logic (ADR-021/043 Humble UI; ADR-031 command/event).
//!
//! Design: `_private/docs/designs/2026-05-25-slice-32l-phase-2-responder-screen-driven-design.md`.

use super::{AppEngine, AppScreen};
use crate::ui::ActionResult;
use crate::ui::LinkResponderEngine;
use crate::ui::ScreenModel;
use crate::ui::WorkflowEngine;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::link_mode::{
    DeepLinkPayload, parse_card_payload, responder_respond_with_card_bytes, serialize_card_payload,
};
use vauchi_core::exchange::link_responder::{
    LinkResponderFailureReason, LinkResponderSession, LinkResponderState,
};
use vauchi_core::{Command, Event};

/// Responder polling budget — mirrors the ADR-035 device-link window
/// (300 s). After this many seconds without a `RelayEscrowReady`, a
/// `tick` transitions the machine to `Failed(PollingTimedOut)`.
const RESPONDER_POLL_DEADLINE_SECS: u64 = 300;

impl AppEngine {
    /// Terminal success — the sender's card was retrieved and persisted.
    /// Transitions the responder engine to `link_responder_completed`.
    pub fn link_responder_completed(&mut self) -> Option<ScreenModel> {
        let engine = self.link_responder_engine_mut()?;
        engine.transition_to_completed();
        Some(engine.current_screen())
    }

    /// Terminal failure. `reason` is the stable `LinkResponder` failure
    /// id. Transitions the responder engine to `link_responder_failed`.
    pub fn link_responder_failed(&mut self, reason: String) -> Option<ScreenModel> {
        let engine = self.link_responder_engine_mut()?;
        engine.transition_to_failed(reason);
        Some(engine.current_screen())
    }

    fn link_responder_engine_mut(&mut self) -> Option<&mut LinkResponderEngine> {
        if !matches!(self.screen, AppScreen::DeepLinkResponder { .. }) {
            return None;
        }
        self.engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<LinkResponderEngine>())
    }

    /// Build / drop the engine-owned responder machine as navigation
    /// enters or leaves `AppScreen::DeepLinkResponder`. Called from
    /// `navigate_to_internal` after the screen-presentation lifecycle
    /// hooks. On entry the machine's initial deposit commands are pushed
    /// onto `pending_commands` so the same drain that carries
    /// `screen_entered` commands surfaces them to the frontend.
    pub(super) fn sync_link_responder_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::DeepLinkResponder { .. });
        let is = matches!(new, AppScreen::DeepLinkResponder { .. });
        match (was, is) {
            (true, false) => self.link_responder = None,
            (false, true) => {
                if let AppScreen::DeepLinkResponder { payload } = new {
                    let payload = payload.clone();
                    let deposits = self.build_link_responder(&payload);
                    self.pending_commands.extend(deposits);
                }
            }
            _ => {}
        }
    }

    /// Construct the responder machine for `payload` and return its
    /// initial `RelayEscrowDeposit ×2 + RelayEscrowCheck` commands.
    /// Returns an empty vec (leaving the screen on its waiting state) if
    /// no identity exists or the ECDH / key-derive fails — both rare and
    /// non-fatal at this layer.
    fn build_link_responder(&mut self, payload: &DeepLinkPayload) -> Vec<Command> {
        let (signing_key, display_name) = match self.vauchi.identity() {
            Some(identity) => (
                *identity.signing_public_key(),
                identity.display_name().to_string(),
            ),
            None => return Vec::new(),
        };
        let card = match self.vauchi.own_card() {
            Ok(Some(card)) => card,
            Ok(None) => ContactCard::new(&display_name),
            Err(_) => return Vec::new(),
        };
        let card_bytes = serialize_card_payload(&signing_key, &card);
        let (keys, deposits) =
            match responder_respond_with_card_bytes(payload.as_parsed(), &card_bytes) {
                Ok(parts) => parts,
                Err(_) => return Vec::new(),
            };
        let deadline = self.vauchi.clock().unix_seconds() + RESPONDER_POLL_DEADLINE_SECS;
        let mut machine = LinkResponderSession::new(keys, deposits, deadline);
        let initial = machine.drain_pending_commands();
        self.link_responder = Some(machine);
        initial
    }

    /// Feed a `RelayEscrow*` hardware event to the engine-owned responder
    /// machine and reconcile the screen with its new state. Returns
    /// `ActionResult::Commands` for the follow-up `RelayEscrowRetrieve`
    /// while still polling/retrieving, and `None` once a terminal screen
    /// has been rendered. A no-op (returns `None`) off the responder
    /// screen, for non-`RelayEscrow*` events, or when no machine is live —
    /// so the caller can pass every hardware event through unconditionally.
    pub(super) fn route_link_responder_hardware_event(
        &mut self,
        event: &Event,
    ) -> Option<ActionResult> {
        if !matches!(self.screen, AppScreen::DeepLinkResponder { .. }) {
            return None;
        }
        if !matches!(
            event,
            Event::RelayEscrowReady { .. }
                | Event::RelayEscrowFailed { .. }
                | Event::RelayEscrowBlobReceived { .. }
        ) {
            return None;
        }
        let (state, new_commands) = {
            let machine = self.link_responder.as_mut()?;
            machine.apply_hardware_event(event.clone());
            (
                machine.current_state().clone(),
                machine.drain_pending_commands(),
            )
        };
        match state {
            LinkResponderState::Finalized { card_bytes } => {
                match self.import_link_card_bytes(&card_bytes) {
                    Ok(()) => {
                        self.link_responder_completed();
                    }
                    Err(_) => {
                        self.link_responder_failed("decrypt_error".to_string());
                    }
                }
                self.link_responder = None;
                None
            }
            LinkResponderState::Failed(reason) => {
                self.link_responder_failed(link_responder_failure_id(&reason));
                self.link_responder = None;
                None
            }
            LinkResponderState::Polling | LinkResponderState::Retrieving => {
                if new_commands.is_empty() {
                    None
                } else {
                    Some(ActionResult::Commands {
                        commands: new_commands,
                    })
                }
            }
        }
    }

    /// Decode a finalized card payload (`[version][pubkey][card]`) and
    /// persist it via the core import path (ADR-034 trust derivation +
    /// idempotent dedup live there). The frontend never sees the bytes.
    pub(super) fn import_link_card_bytes(&self, card_bytes: &[u8]) -> Result<(), String> {
        let (_signing_key, card) = parse_card_payload(card_bytes).map_err(|e| e.to_string())?;
        self.vauchi
            .import_received_link_card(card)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Map a `LinkResponderFailureReason` to the stable failure id the
/// `LinkResponderEngine` renderer keys its message off of.
fn link_responder_failure_id(reason: &LinkResponderFailureReason) -> String {
    match reason {
        LinkResponderFailureReason::PollingTimedOut => "polling_timed_out",
        LinkResponderFailureReason::DepositRejected => "deposit_rejected",
        LinkResponderFailureReason::DecryptError { .. } => "decrypt_error",
        LinkResponderFailureReason::Cancelled => "cancelled",
    }
    .to_string()
}
