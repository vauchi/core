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
//! completes the exchange via `complete_link_exchange` (ADR-050 T5b) — a
//! v2 bootstrap establishes a live, updatable `Exchanged` Link contact +
//! Double Ratchet, a v1 peer falls back to a frozen
//! `import_received_link_card`; on `Failed` it renders
//! `exchange_link_failed`. Frontends pull no session object and own no
//! workflow logic (ADR-021/043 Humble UI; ADR-031 command/event).
//!
//! Mirror of `app_engine/link_responder.rs` for the initiator half — the
//! initiator additionally owns the share URL and walks the engine through
//! its `WaitingForResponse` / `Retrieving` presentation states as the
//! two-gate session advances.
//!
//! See `_private/docs/problems/2026-05-11-link-exchange-engine-graduation/`.

use super::{AppEngine, AppScreen};
use crate::ui::ActionResult;

#[cfg(all(feature = "network-http", feature = "storage"))]
use vauchi_core::Command;
use vauchi_core::Event;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::link_initiator::{
    LinkInitiatorFailureReason, LinkInitiatorSession, LinkInitiatorState,
};
use vauchi_core::exchange::link_mode;

/// Initiator polling budget — mirrors the ADR-035 device-link window
/// (300 s). After this many seconds without a terminal state, a `tick`
/// transitions the machine to `Failed(PollingTimedOut)`.
const INITIATOR_POLL_DEADLINE_SECS: u64 = 300;

impl AppEngine {
    /// Build / drop the engine-owned initiator machine as navigation
    /// enters or leaves `AppScreen::LinkExchange`. Called from
    /// `navigate_to_internal` after the screen-presentation lifecycle
    /// hooks. On entry the initiator machine is built (ADR-049); its
    /// escrow commands stay *in the machine* for the core poll
    /// (`advance_link_initiator_session`) to drive — they are no longer
    /// pushed onto the frontend `pending_commands` queue. The generated
    /// share URL is pushed into the renderer.
    pub(super) fn sync_link_initiator_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::LinkExchange);
        let is = matches!(new, AppScreen::LinkExchange);
        match (was, is) {
            (true, false) => {
                self.link_initiator = None;
                self.link_initiator_x3dh = None;
            }
            (false, true) => self.build_link_initiator(),
            _ => {}
        }
    }

    /// Construct the initiator machine and store it (ADR-049). Its initial
    /// presence-deposit commands stay queued in the machine for the core
    /// poll to drive. The generated share URL is pushed into the
    /// `LinkExchangeEngine` renderer via `set_share_url`. No-op (leaving the
    /// renderer on its share-url screen with an empty URL) if no identity
    /// exists — rare and non-fatal here.
    fn build_link_initiator(&mut self) {
        // Fresh per-exchange X3DH keypair: its public half is signed into the
        // v2 bootstrap; its secret completes the exchange on `Finalized`.
        let x3dh = X3DHKeyPair::generate();
        let card_bytes = match self.build_link_card_bytes_v2(x3dh.public_key()) {
            Some(bytes) => bytes,
            None => return,
        };
        let (initiation, presence_commands) = link_mode::initiator_generate();
        let share_url = initiation.url.clone();
        let deadline = self.vauchi.clock().unix_seconds() + INITIATOR_POLL_DEADLINE_SECS;
        self.link_initiator = Some(LinkInitiatorSession::new(
            initiation,
            presence_commands,
            card_bytes,
            deadline,
        ));
        self.link_initiator_x3dh = Some(x3dh);
        let _ = self
            .engine
            .apply_update(crate::ui::EngineUpdate::LinkExchange(
                crate::ui::LinkExchangeUpdate::ShareUrl(share_url),
            ));
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
                let completed = match self.link_initiator_x3dh.as_ref() {
                    Some(x3dh) => self.complete_link_card_bytes(&card_bytes, x3dh),
                    // No retained key (v1 peer / pre-T5b session) — frozen import.
                    None => self.import_link_card_bytes(&card_bytes),
                };
                match completed {
                    Ok(contact_id) => {
                        // Link mode assigns no group, so group_names is empty.
                        let summary = self.build_exchange_summary(&contact_id, Vec::new());
                        let _ = self
                            .engine
                            .apply_update(crate::ui::EngineUpdate::LinkExchange(
                                crate::ui::LinkExchangeUpdate::Succeeded(summary),
                            ));
                        // Ceremony (M2 S4): the card completed + persisted just
                        // above — validated success, once per link session.
                        self.extend_pending_commands(vec![
                            crate::ui::exchange::ceremony::exchange_celebrate(),
                        ]);
                    }
                    Err(_) => {
                        let _ = self
                            .engine
                            .apply_update(crate::ui::EngineUpdate::LinkExchange(
                                crate::ui::LinkExchangeUpdate::Failed("decrypt_error".to_string()),
                            ));
                    }
                }
                self.link_initiator = None;
                self.link_initiator_x3dh = None;
                None
            }
            LinkInitiatorState::Failed(reason) => {
                let id = link_initiator_failure_id(&reason);
                let _ = self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::LinkExchange(
                        crate::ui::LinkExchangeUpdate::Failed(id),
                    ));
                self.link_initiator = None;
                None
            }
            LinkInitiatorState::Polling => {
                let _ = self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::LinkExchange(
                        crate::ui::LinkExchangeUpdate::Waiting,
                    ));
                if new_commands.is_empty() {
                    None
                } else {
                    Some(ActionResult::Commands {
                        commands: new_commands,
                    })
                }
            }
            LinkInitiatorState::Retrieving => {
                let _ = self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::LinkExchange(
                        crate::ui::LinkExchangeUpdate::Retrieving,
                    ));
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

    /// Advance the engine-owned link-mode initiator one relay step
    /// (ADR-049). Called each `poll_notifications` tick (a no-op off the
    /// `LinkExchange` screen). Core drives the two-gate escrow round-trip:
    /// it executes the machine's queued presence/card deposits and
    /// retrieves via `Vauchi::run_escrow_command`, re-issues a `Check` per
    /// tick for the gate the machine is currently watching (handshake while
    /// `Polling`, escrow while `Retrieving`), and ticks the polling
    /// deadline. Returns `true` if any machine event was applied.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    pub(crate) fn advance_link_initiator_session(&mut self) -> bool {
        if !matches!(self.screen, AppScreen::LinkExchange) {
            return false;
        }
        let now = self.vauchi.clock().unix_seconds();
        let mut advanced = false;

        // 1. Execute whatever the machine has queued (presence deposits,
        //    then the card deposit + escrow Check once the handshake
        //    completes; retrieves). Each event may cascade follow-ups.
        let queued = match self.link_initiator.as_mut() {
            Some(machine) => machine.drain_pending_commands(),
            None => return false,
        };
        for command in queued {
            advanced |= self.run_and_feed_initiator_command(&command);
        }

        // 2. Re-issue a Check for the gate the machine currently watches
        //    (it queues a Check only on each transition, not per tick).
        let gate = self
            .link_initiator
            .as_ref()
            .and_then(|machine| match machine.current_state() {
                LinkInitiatorState::Polling => Some(machine.handshake_gate_bytes()),
                LinkInitiatorState::Retrieving => machine.escrow_gate_bytes(),
                _ => None,
            });
        if let Some(gate_hash) = gate {
            let check = Command::RelayEscrowCheck {
                gate_hash,
                suggested_interval_ms: 0,
            };
            advanced |= self.run_and_feed_initiator_command(&check);
        }

        // 3. Enforce the polling deadline (Failed(PollingTimedOut)).
        if let Some(machine) = self.link_initiator.as_mut() {
            machine.tick(now);
        }
        advanced
    }

    /// Run one escrow command for the initiator, feed the resulting event
    /// back, and recursively run the follow-up commands
    /// `route_link_initiator_hardware_event` returns. A blob retrieved from
    /// the handshake gate is the responder's epk and is fed as `LinkOpened`
    /// (which the machine consumes to derive the escrow keys); escrow-gate
    /// blobs stay `RelayEscrowBlobReceived`.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    fn run_and_feed_initiator_command(&mut self, command: &Command) -> bool {
        let Some(mut event) = self.vauchi.run_escrow_command(command) else {
            return false;
        };
        if let Event::RelayEscrowBlobReceived { gate_hash, blob } = &event {
            let is_handshake = self
                .link_initiator
                .as_ref()
                .is_some_and(|machine| machine.handshake_gate_bytes() == *gate_hash);
            if is_handshake {
                event = Event::LinkOpened {
                    peer_public_key: blob.clone(),
                };
            }
        }
        if let Some(ActionResult::Commands { commands }) =
            self.route_link_initiator_hardware_event(&event)
        {
            for command in commands {
                self.run_and_feed_initiator_command(&command);
            }
        }
        true
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
