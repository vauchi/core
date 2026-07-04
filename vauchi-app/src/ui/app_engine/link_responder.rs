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
//! On `Finalized` core completes the exchange via
//! `complete_link_exchange` (ADR-050 T5b) — a v2 bootstrap establishes a
//! live, updatable `Exchanged` Link contact + Double Ratchet, a v1 peer
//! falls back to a frozen `import_received_link_card`; on `Failed` it
//! renders `link_responder_failed`. Frontends pull no session object and
//! own no workflow logic (ADR-021/043 Humble UI; ADR-031 command/event).
//!
//! Design: `_private/docs/designs/2026-05-25-slice-32l-phase-2-responder-screen-driven-design.md`.

use super::{AppEngine, AppScreen};
use crate::ui::ActionResult;
use crate::ui::ScreenModel;

#[cfg(all(feature = "network-http", feature = "storage"))]
use vauchi_core::Command;
use vauchi_core::Event;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::link_mode::{
    DeepLinkPayload, parse_card_payload, responder_respond_with_card_bytes,
    serialize_card_payload_v2,
};
use vauchi_core::exchange::link_responder::{
    LinkResponderFailureReason, LinkResponderSession, LinkResponderState,
};

/// Responder polling budget — mirrors the ADR-035 device-link window
/// (300 s). After this many seconds without a `RelayEscrowReady`, a
/// `tick` transitions the machine to `Failed(PollingTimedOut)`.
const RESPONDER_POLL_DEADLINE_SECS: u64 = 300;

impl AppEngine {
    /// Terminal success — the sender's card was retrieved and persisted.
    /// Builds the rich success summary from the persisted contact, attaches
    /// it, and transitions the responder engine to
    /// `link_responder_completed`.
    pub fn link_responder_completed(&mut self, contact_id: &str) -> Option<ScreenModel> {
        // Build the summary first so the immutable `self.vauchi` borrow ends
        // before the `&mut self` engine borrow. Link mode assigns no group.
        let summary = self.build_exchange_summary(contact_id, Vec::new());
        // Ceremony (M2 S4): the sender's card was retrieved + persisted —
        // validated success, once per responder session.
        self.extend_pending_commands(vec![crate::ui::exchange::ceremony::exchange_celebrate()]);
        self.engine
            .apply_update(crate::ui::EngineUpdate::LinkResponder(
                crate::ui::LinkResponderUpdate::Completed(summary),
            ))
            .then(|| self.engine.current_screen())
    }

    /// Terminal failure. `reason` is the stable `LinkResponder` failure
    /// id. Transitions the responder engine to `link_responder_failed`.
    pub fn link_responder_failed(&mut self, reason: String) -> Option<ScreenModel> {
        self.engine
            .apply_update(crate::ui::EngineUpdate::LinkResponder(
                crate::ui::LinkResponderUpdate::Failed(reason),
            ))
            .then(|| self.engine.current_screen())
    }

    /// Build / drop the engine-owned responder machine as navigation
    /// enters or leaves `AppScreen::DeepLinkResponder`. Called from
    /// `navigate_to_internal` after the screen-presentation lifecycle
    /// hooks. On entry the responder machine is built (ADR-049); its
    /// initial escrow commands stay *in the machine* for the core poll
    /// (`advance_link_responder_session`) to drive — they are no longer
    /// pushed onto the frontend `pending_commands` queue.
    pub(super) fn sync_link_responder_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::DeepLinkResponder { .. });
        let is = matches!(new, AppScreen::DeepLinkResponder { .. });
        match (was, is) {
            (true, false) => {
                self.link_responder = None;
                self.link_responder_x3dh = None;
            }
            (false, true) => {
                if let AppScreen::DeepLinkResponder { payload } = new {
                    let payload = payload.clone();
                    self.build_link_responder(&payload);
                }
            }
            _ => {}
        }
    }

    /// Construct the responder machine for `payload` and store it
    /// (ADR-049). The machine's initial `RelayEscrowDeposit + RelayEscrowCheck`
    /// commands stay queued *in the machine* so the core poll
    /// (`advance_link_responder_session`) drains and executes them over the
    /// relay — they are no longer surfaced to the frontend `pending_commands`
    /// queue, which no frontend ever executed (the gap this ADR closes).
    /// No-op (leaving the screen on its waiting state) if no identity exists
    /// or the ECDH / key-derive fails — both rare and non-fatal here.
    fn build_link_responder(&mut self, payload: &DeepLinkPayload) {
        // Fresh per-exchange X3DH keypair: its public half is signed into the
        // v2 bootstrap; its secret completes the exchange on `Finalized`.
        let x3dh = X3DHKeyPair::generate();
        let card_bytes = match self.build_link_card_bytes_v2(x3dh.public_key()) {
            Some(bytes) => bytes,
            None => return,
        };
        let (keys, deposits) =
            match responder_respond_with_card_bytes(payload.as_parsed(), &card_bytes) {
                Ok(parts) => parts,
                Err(_) => return,
            };
        let deadline = self.vauchi.clock().unix_seconds() + RESPONDER_POLL_DEADLINE_SECS;
        self.link_responder = Some(LinkResponderSession::new(keys, deposits, deadline));
        self.link_responder_x3dh = Some(x3dh);
    }

    /// Build the v2 symmetric-exchange bootstrap (ADR-050) we deposit: our
    /// card + identity signature over `x3dh_pubkey` and our relay routing, so
    /// the peer can verify it and establish a live update channel. Shared by
    /// both link builders. `None` if no identity / card read fails (rare,
    /// non-fatal — the caller leaves the screen on its waiting state).
    pub(super) fn build_link_card_bytes_v2(&self, x3dh_pubkey: &[u8; 32]) -> Option<Vec<u8>> {
        let identity = self.vauchi.identity()?;
        let identity_pubkey = *identity.signing_public_key();
        let display_name = identity.display_name().to_string();
        let card = match self.vauchi.own_card() {
            Ok(Some(card)) => card,
            Ok(None) => ContactCard::new(&display_name),
            Err(_) => return None,
        };
        Some(serialize_card_payload_v2(
            &identity_pubkey,
            identity.signing_keypair(),
            x3dh_pubkey,
            self.vauchi.relay_server_url(),
            &card,
        ))
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
                let completed = match self.link_responder_x3dh.as_ref() {
                    Some(x3dh) => self.complete_link_card_bytes(&card_bytes, x3dh),
                    // No retained key (v1 peer / pre-T5b session) — frozen import.
                    None => self.import_link_card_bytes(&card_bytes),
                };
                match completed {
                    Ok(contact_id) => {
                        self.link_responder_completed(&contact_id);
                    }
                    Err(_) => {
                        self.link_responder_failed("decrypt_error".to_string());
                    }
                }
                self.link_responder = None;
                self.link_responder_x3dh = None;
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
    /// Returns the persisted contact id (for building the success summary).
    pub(super) fn import_link_card_bytes(&self, card_bytes: &[u8]) -> Result<String, String> {
        let (_signing_key, card) = parse_card_payload(card_bytes).map_err(|e| e.to_string())?;
        self.vauchi
            .import_received_link_card(card)
            .map_err(|e| e.to_string())
    }

    /// Complete a link exchange from the peer's finalized payload using the
    /// retained X3DH secret (ADR-050 T5b): a v2 bootstrap yields a live,
    /// updatable `Exchanged` contact + Double Ratchet; a v1 payload falls
    /// back to a frozen import inside `complete_link_exchange`. The frontend
    /// never sees the bytes. Shared by both link finalize paths.
    /// Returns the persisted contact id (for building the success summary).
    pub(super) fn complete_link_card_bytes(
        &self,
        card_bytes: &[u8],
        our_x3dh: &X3DHKeyPair,
    ) -> Result<String, String> {
        self.vauchi
            .complete_link_exchange(card_bytes, our_x3dh)
            .map_err(|e| e.to_string())
    }

    /// Advance the engine-owned link responder one relay step (ADR-049).
    ///
    /// Called each `poll_notifications` tick (a no-op off the responder
    /// screen / with no live machine). Core — not the frontend — now
    /// drives the escrow round-trip: it executes the machine's queued
    /// deposit/retrieve commands via `Vauchi::run_escrow_command` and
    /// feeds the resulting `RelayEscrow*` events back through
    /// `route_link_responder_hardware_event`. Because the machine queues a
    /// gate `Check` only once, while it is still `Polling` we re-issue one
    /// per tick to detect both escrow slots filling; a `Ready` then drives
    /// the queued `Retrieve` in the same tick so an already-deposited peer
    /// completes immediately. Finally `tick` enforces the polling
    /// deadline. Returns `true` if any machine event was applied.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    pub(crate) fn advance_link_responder_session(&mut self) -> bool {
        if !matches!(self.screen, AppScreen::DeepLinkResponder { .. }) {
            return false;
        }
        let now = self.vauchi.clock().unix_seconds();
        let mut advanced = false;

        // 1. Execute whatever the machine has queued (initial deposits +
        //    its one Check on entry). Each event may cascade into follow-up
        //    commands (Ready → Retrieve → Finalized) handled by the helper.
        let queued = match self.link_responder.as_mut() {
            Some(machine) => machine.drain_pending_commands(),
            None => return false,
        };
        for command in queued {
            advanced |= self.run_and_feed_escrow_command(&command);
        }

        // 2. Active gate poll while still waiting (the machine queues a
        //    Check only on entry; re-issue one per tick until the gate
        //    fills). A Ready here cascades into the Retrieve.
        let polling_gate = self.link_responder.as_ref().and_then(|machine| {
            matches!(machine.current_state(), LinkResponderState::Polling)
                .then(|| machine.gate_hash_bytes())
        });
        if let Some(gate_hash) = polling_gate {
            let check = Command::RelayEscrowCheck {
                gate_hash,
                suggested_interval_ms: 0,
            };
            advanced |= self.run_and_feed_escrow_command(&check);
        }

        // 3. Enforce the polling deadline (Failed(PollingTimedOut)).
        if let Some(machine) = self.link_responder.as_mut() {
            machine.tick(now);
        }
        advanced
    }

    /// Execute one escrow command over the relay, feed the resulting
    /// `RelayEscrow*` event into the responder machine, and recursively run
    /// any follow-up commands the machine queues in response (e.g. the
    /// `Retrieve` it emits on `Ready`).
    /// `route_link_responder_hardware_event` drains those follow-ups and
    /// returns them as `ActionResult::Commands` — under ADR-049 core runs
    /// them itself instead of handing them to a frontend. Returns `true` if
    /// an event applied. Recursion is bounded by the machine's terminal
    /// states (`Finalized` / `Failed` drain nothing).
    #[cfg(all(feature = "network-http", feature = "storage"))]
    fn run_and_feed_escrow_command(&mut self, command: &Command) -> bool {
        let Some(event) = self.vauchi.run_escrow_command(command) else {
            return false;
        };
        if let Some(ActionResult::Commands { commands }) =
            self.route_link_responder_hardware_event(&event)
        {
            for command in commands {
                self.run_and_feed_escrow_command(&command);
            }
        }
        true
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
