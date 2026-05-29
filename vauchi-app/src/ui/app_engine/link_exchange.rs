// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link-mode initiator lifecycle on `AppEngine` (slice 32l Phase 3).
//!
//! The `AppEngine` owns the pure-core `LinkInitiatorSession` state
//! machine while the active screen is `AppScreen::LinkExchange`,
//! replacing the retired `ui/exchange/link.rs` sub-flow. The machine is
//! built on entry (its presence-deposit `Command`s flow out through the
//! `pending_commands` queue and its share URL is pushed into the
//! `LinkExchangeEngine` renderer via `set_share_url`), driven by
//! `LinkShared` / `LinkOpened` / `RelayEscrow*` hardware events via
//! `route_link_initiator_hardware_event` (follow-up commands surface as
//! `ActionResult::Commands`), and dropped on exit. On `Finalized` core
//! decodes + persists the received card via `import_received_link_card`;
//! on `Failed` it renders `exchange_link_failed`. Frontends pull no
//! session object and own no workflow logic (ADR-021/043 Humble UI;
//! ADR-031 command/event).
//!
//! Mirror of `app_engine/link_responder.rs` for the initiator half — the
//! initiator additionally owns the share URL and walks the engine through
//! its `WaitingForResponse` / `Retrieving` presentation states as the
//! two-gate session advances.
//!
//! See `_private/docs/problems/2026-05-11-link-exchange-engine-graduation/`.

use super::{AppEngine, AppScreen};
use crate::ui::ActionResult;
use crate::ui::LinkExchangeEngine;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::link_initiator::{
    LinkInitiatorFailureReason, LinkInitiatorSession, LinkInitiatorState,
};
use vauchi_core::exchange::link_mode::{self, serialize_card_payload};
use vauchi_core::{Command, Event};

/// Initiator polling budget — mirrors the ADR-035 device-link window
/// (300 s). After this many seconds without a terminal state, a `tick`
/// transitions the machine to `Failed(PollingTimedOut)`.
const INITIATOR_POLL_DEADLINE_SECS: u64 = 300;

impl AppEngine {
    fn link_exchange_engine_mut(&mut self) -> Option<&mut LinkExchangeEngine> {
        if !matches!(self.screen, AppScreen::LinkExchange) {
            return None;
        }
        self.engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<LinkExchangeEngine>())
    }

    /// Build / drop the engine-owned initiator machine as navigation
    /// enters or leaves `AppScreen::LinkExchange`. Called from
    /// `navigate_to_internal` after the screen-presentation lifecycle
    /// hooks. On entry the machine's initial presence-deposit commands
    /// are pushed onto `pending_commands` so the same drain that carries
    /// `screen_entered` commands surfaces them to the frontend, and the
    /// generated share URL is pushed into the renderer.
    pub(super) fn sync_link_initiator_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::LinkExchange);
        let is = matches!(new, AppScreen::LinkExchange);
        match (was, is) {
            (true, false) => self.link_initiator = None,
            (false, true) => {
                let presence = self.build_link_initiator();
                self.pending_commands.extend(presence);
            }
            _ => {}
        }
    }

    /// Construct the initiator machine and return its initial presence
    /// deposit commands. The generated share URL is pushed into the
    /// `LinkExchangeEngine` renderer via `set_share_url`. Returns an
    /// empty vec (leaving the renderer on its share-url screen with an
    /// empty URL) if no identity exists — rare and non-fatal here.
    fn build_link_initiator(&mut self) -> Vec<Command> {
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
        let (initiation, presence_commands) = link_mode::initiator_generate();
        let share_url = initiation.url.clone();
        let deadline = self.vauchi.clock().unix_seconds() + INITIATOR_POLL_DEADLINE_SECS;
        let mut machine =
            LinkInitiatorSession::new(initiation, presence_commands, card_bytes, deadline);
        let initial = machine.drain_pending_commands();
        self.link_initiator = Some(machine);
        if let Some(engine) = self.link_exchange_engine_mut() {
            engine.set_share_url(share_url);
        }
        initial
    }

    /// Feed a `LinkShared` / `LinkOpened` / `RelayEscrow*` hardware event
    /// to the engine-owned initiator machine and reconcile the renderer
    /// with its new state. Returns `ActionResult::Commands` for follow-up
    /// relay calls while still polling/retrieving, and `None` once a
    /// terminal screen has been rendered. A no-op (returns `None`) off the
    /// LinkExchange screen, for unrelated events, or when no machine is
    /// live — so the caller can pass every hardware event through
    /// unconditionally.
    pub(super) fn route_link_initiator_hardware_event(
        &mut self,
        event: &Event,
    ) -> Option<ActionResult> {
        if !matches!(self.screen, AppScreen::LinkExchange) {
            return None;
        }
        if !matches!(
            event,
            Event::LinkShared
                | Event::LinkOpened { .. }
                | Event::RelayEscrowReady { .. }
                | Event::RelayEscrowFailed { .. }
                | Event::RelayEscrowBlobReceived { .. }
        ) {
            return None;
        }
        let (state, new_commands) = {
            let machine = self.link_initiator.as_mut()?;
            machine.apply_hardware_event(event.clone());
            (
                machine.current_state().clone(),
                machine.drain_pending_commands(),
            )
        };
        match state {
            LinkInitiatorState::Finalized { card_bytes } => {
                match self.import_link_card_bytes(&card_bytes) {
                    Ok(()) => {
                        if let Some(engine) = self.link_exchange_engine_mut() {
                            engine.transition_to_success();
                        }
                    }
                    Err(_) => {
                        if let Some(engine) = self.link_exchange_engine_mut() {
                            engine.transition_to_failed("decrypt_error".to_string());
                        }
                    }
                }
                self.link_initiator = None;
                None
            }
            LinkInitiatorState::Failed(reason) => {
                let id = link_initiator_failure_id(&reason);
                if let Some(engine) = self.link_exchange_engine_mut() {
                    engine.transition_to_failed(id);
                }
                self.link_initiator = None;
                None
            }
            LinkInitiatorState::Polling => {
                if let Some(engine) = self.link_exchange_engine_mut() {
                    engine.transition_to_waiting();
                }
                if new_commands.is_empty() {
                    None
                } else {
                    Some(ActionResult::Commands {
                        commands: new_commands,
                    })
                }
            }
            LinkInitiatorState::Retrieving => {
                if let Some(engine) = self.link_exchange_engine_mut() {
                    engine.transition_to_retrieving();
                }
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
}

/// Map a `LinkInitiatorFailureReason` to the stable failure id the
/// `LinkExchangeEngine` renderer keys its message off of.
fn link_initiator_failure_id(reason: &LinkInitiatorFailureReason) -> String {
    match reason {
        LinkInitiatorFailureReason::PollingTimedOut => "polling_timed_out",
        LinkInitiatorFailureReason::HandshakeFailed { .. } => "handshake_failed",
        LinkInitiatorFailureReason::DecryptError { .. } => "decrypt_error",
        LinkInitiatorFailureReason::DepositRejected => "deposit_rejected",
        LinkInitiatorFailureReason::Cancelled => "cancelled",
    }
    .to_string()
}
