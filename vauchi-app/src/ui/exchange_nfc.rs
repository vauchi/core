// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! NFC exchange sub-flow — drives the 3-phase encrypted handshake
//! through the `ExchangeCommand` / `ExchangeHardwareEvent` protocol
//! (ADR-031), removing the need for the `MobileNfcHandshake` UniFFI
//! session Object.
//!
//! Design: `_private/docs/designs/2026-05-19-nfc-exchange-engine-design.md`.
//! Parent record:
//! `_private/docs/problems/2026-05-19-nfc-exchange-engine-graduation/`.
//!
//! State machine:
//!
//! ```text
//! Idle ── activate() ──► AwaitingTap
//!                            │
//!     ┌──────────────────────┼──────────────────────┐
//!     │ initiator            │ responder            │
//!     ▼                      ▼                      │
//!  PayloadSent            AckSent                   │
//!     │                      │                      │
//!     └──── NfcDataReceived ─┴──► Complete          │
//! ```
//!
//! Cadence is per-phase, not per-APDU (iOS CoreNFC and Android HCE
//! both reassemble extended APDUs transparently — see design doc §4).

#![allow(dead_code)] // wired into ExchangeEngine in a follow-up commit

use vauchi_core::clock::SystemClock;
use vauchi_core::exchange::escrow::{EscrowKeys, EscrowRole};
use vauchi_core::exchange::{
    NFC_PAYLOAD_SIZE, NfcCardPayload, NfcHandshakeSession, NfcHandshakeState,
};
use vauchi_core::identity::Identity;
use vauchi_core::{Command, Event};

use crate::ui::*;

// ── Step enum ──────────────────────────────────────────────────────────────

/// Steps specific to the NFC exchange sub-flow.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum NfcStep {
    /// Pre-activation. Waiting for parent `ExchangeEngine` to flip step.
    Idle,
    /// `Command::NfcActivate` emitted; awaiting `Event::NfcDataReceived`
    /// with the peer's response.
    AwaitingTap,
    /// (Initiator only) Phase 2 processed; `Command::NfcSendApdu`
    /// emitted with our encrypted card; awaiting Phase 3 confirmation.
    PayloadSent,
    /// (Responder only) Phase 1 processed; `Command::NfcSendApdu`
    /// emitted with our key ack + encrypted card; awaiting Phase 3
    /// (initiator's encrypted card).
    AckSent,
    /// 3-phase handshake complete on both sides.
    Complete,
}

impl NfcStep {
    pub(super) fn step_number(&self, base: u8) -> u8 {
        base + match self {
            Self::Idle | Self::AwaitingTap => 0,
            Self::PayloadSent | Self::AckSent => 1,
            Self::Complete => 2,
        }
    }

    /// Matches QrStep/LinkStep/BleStep for the parent progress bar.
    pub(super) const STEP_COUNT: u8 = 3;
}

// ── Outcome enum ───────────────────────────────────────────────────────────

/// Result of handling a hardware event in the NFC sub-flow.
#[derive(Debug)]
pub(super) enum NfcHardwareOutcome {
    /// Step advanced — parent should update screen. May emit commands.
    StepAdvanced { commands: Vec<Command> },
    /// NFC exchange completed — card bytes available.
    Complete {
        card_bytes: Vec<u8>,
        commands: Vec<Command>,
    },
    /// NFC failed — offer relay fallback if `relay_handoff.is_some()`,
    /// otherwise the parent should route to a QR-fallback or cancel
    /// screen.
    FailedWithFallback {
        reason: String,
        relay_handoff: Option<RelayHandoff>,
    },
    /// Event consumed but no step change. May emit commands.
    Consumed { commands: Vec<Command> },
    /// Event not handled by NFC flow.
    Ignored,
}

/// Payload for `Command::RelayEscrowDeposit` when the NFC tap drops
/// after the shared key has been established. Computed by the
/// sub-flow; the parent emits the actual escrow Command.
#[derive(Debug)]
pub(super) struct RelayHandoff {
    pub gate_hash: Vec<u8>,
    pub slot_hash: Vec<u8>,
    pub encrypted_card: Vec<u8>,
}

// ── Flow struct ────────────────────────────────────────────────────────────

/// NFC exchange sub-flow. Owns the underlying `NfcHandshakeSession`
/// (unlike `BleExchangeFlow`, which delegates to `ExchangeSession`)
/// because NFC's 3-phase state is the *whole* protocol state — there
/// is nothing useful for an outer container to hold separately.
pub(super) struct NfcExchangeFlow {
    step: NfcStep,
    session: NfcHandshakeSession,
    is_initiator: bool,
    /// Cached identity reference for `process_key_offer` (responder)
    /// and `create_key_offer` (initiator) — both need the full
    /// `Identity` to sign their NFC payload.
    identity: Identity,
}

impl NfcExchangeFlow {
    pub(super) fn new_initiator(identity: Identity, display_name: String) -> Self {
        let session = NfcHandshakeSession::new_initiator(&identity, display_name);
        Self {
            step: NfcStep::Idle,
            session,
            is_initiator: true,
            identity,
        }
    }

    pub(super) fn new_responder(identity: Identity, display_name: String) -> Self {
        let session = NfcHandshakeSession::new_responder(&identity, display_name);
        Self {
            step: NfcStep::Idle,
            session,
            is_initiator: false,
            identity,
        }
    }

    pub(super) fn step(&self) -> &NfcStep {
        &self.step
    }

    /// Transition from Idle to AwaitingTap and emit the initial
    /// activation Command. Initiator sends its key offer in the
    /// payload; responder sends empty (it waits for the peer's
    /// key offer first).
    pub(super) fn activate(&mut self) -> Result<Vec<Command>, NfcFlowError> {
        if !matches!(self.step, NfcStep::Idle) {
            return Err(NfcFlowError::WrongState);
        }
        let now = SystemClock::shared().unix_seconds();
        let payload = if self.is_initiator {
            self.session
                .create_key_offer(&self.identity, now)
                .map_err(|e| NfcFlowError::Protocol(e.to_string()))?
        } else {
            Vec::<u8>::new()
        };
        self.step = NfcStep::AwaitingTap;
        Ok(vec![Command::NfcActivate { payload }])
    }

    /// Process a hardware event. Cross-transport failure events
    /// (`HardwareError`/`HardwareUnavailable`/`PermissionDenied`
    /// with `transport == "nfc"` and `BleDisconnected` are
    /// short-circuited at the top — pattern mirrored from
    /// `BleExchangeFlow::handle_event`.
    pub(super) fn handle_event(&mut self, event: &Event) -> NfcHardwareOutcome {
        if let Event::HardwareError { transport, error } = event
            && transport.eq_ignore_ascii_case("nfc")
        {
            return self.fail_with_fallback(error.clone());
        }
        if let Event::HardwareUnavailable { transport } = event
            && transport.eq_ignore_ascii_case("nfc")
        {
            return self.fail_with_fallback("NFC not available".into());
        }
        if let Event::PermissionDenied { transport } = event
            && transport.eq_ignore_ascii_case("nfc")
        {
            return self.fail_with_fallback("NFC permission denied".into());
        }

        match (&self.step, event) {
            (NfcStep::AwaitingTap, Event::NfcDataReceived { data }) => {
                self.handle_awaiting_tap(data)
            }
            (NfcStep::PayloadSent, Event::NfcDataReceived { .. }) => {
                self.handle_payload_sent_complete()
            }
            (NfcStep::AckSent, Event::NfcDataReceived { data }) => self.handle_ack_sent(data),
            (NfcStep::Complete, _) => NfcHardwareOutcome::Ignored,
            _ => NfcHardwareOutcome::Ignored,
        }
    }

    // ── per-state handlers ─────────────────────────────────────────────────

    fn handle_awaiting_tap(&mut self, data: &[u8]) -> NfcHardwareOutcome {
        let now = SystemClock::shared().unix_seconds();
        if self.is_initiator {
            // Initiator: `data` is the responder's (key_ack || encrypted_card).
            // Key ack is always exactly NFC_PAYLOAD_SIZE bytes; the rest is
            // the encrypted card.
            if data.len() <= NFC_PAYLOAD_SIZE {
                return self.fail_with_fallback(format!(
                    "NFC phase-2 response too short: {} bytes",
                    data.len()
                ));
            }
            let (key_ack, encrypted_card) = data.split_at(NFC_PAYLOAD_SIZE);
            match self.session.process_key_ack(key_ack, encrypted_card, now) {
                Ok(our_encrypted_card) => {
                    self.step = NfcStep::PayloadSent;
                    NfcHardwareOutcome::StepAdvanced {
                        commands: vec![Command::NfcSendApdu {
                            data: our_encrypted_card,
                        }],
                    }
                }
                Err(e) => self.fail_with_fallback(e.to_string()),
            }
        } else {
            // Responder: `data` is the initiator's key offer.
            match self.session.process_key_offer(&self.identity, data, now) {
                Ok((our_ack, our_encrypted_card)) => {
                    let mut framed = our_ack;
                    framed.extend(our_encrypted_card);
                    self.step = NfcStep::AckSent;
                    NfcHardwareOutcome::StepAdvanced {
                        commands: vec![Command::NfcSendApdu { data: framed }],
                    }
                }
                Err(e) => self.fail_with_fallback(e.to_string()),
            }
        }
    }

    fn handle_payload_sent_complete(&mut self) -> NfcHardwareOutcome {
        // Initiator: the peer ACK'd our Phase 3 encrypted-card send. The
        // session already cached the remote card during process_key_ack.
        match self.session.confirm_send_success() {
            Ok(result) => {
                self.step = NfcStep::Complete;
                NfcHardwareOutcome::Complete {
                    card_bytes: result
                        .remote_card
                        .to_bytes()
                        .expect("NfcCardPayload re-serialization is infallible by construction"),
                    commands: vec![Command::NfcDeactivate],
                }
            }
            Err(e) => self.fail_with_fallback(e.to_string()),
        }
    }

    fn handle_ack_sent(&mut self, data: &[u8]) -> NfcHardwareOutcome {
        // Responder: `data` is the initiator's encrypted card.
        match self.session.process_encrypted_card(data) {
            Ok(result) => {
                self.step = NfcStep::Complete;
                NfcHardwareOutcome::Complete {
                    card_bytes: result
                        .remote_card
                        .to_bytes()
                        .expect("NfcCardPayload re-serialization is infallible by construction"),
                    commands: vec![Command::NfcDeactivate],
                }
            }
            Err(e) => self.fail_with_fallback(e.to_string()),
        }
    }

    /// Compute a relay handoff when failure occurs after the shared
    /// key has been established, otherwise return `None`. Wires up
    /// the path described in design doc §5: route the shared key
    /// from `NfcHandshakeSession::enter_relay_fallback` through
    /// `EscrowKeys::derive` + `encrypt_card`, mirroring the Link-mode
    /// pattern at `link_mode.rs:95`.
    fn try_relay_handoff(&mut self) -> Option<RelayHandoff> {
        // Only meaningful after the shared key is established.
        match self.session.state() {
            NfcHandshakeState::KeyAckReceived { .. } | NfcHandshakeState::PayloadSent { .. } => {}
            _ => return None,
        }

        // `enter_relay_fallback` mutates the session into RelayFallback
        // state and yields the shared_key derived during the in-band
        // handshake.
        let (_exchange_id, shared_key) = self.session.enter_relay_fallback().ok()?;

        // Build our card payload from the same fields the in-band
        // handshake would have encrypted, then serialize via postcard
        // for the relay deposit blob.
        let card_payload = NfcCardPayload::new(
            *self.session.our_identity_key(),
            self.identity.display_name().to_string(),
            *self.session.our_exchange_key(),
        );
        let card_bytes = card_payload.to_bytes().ok()?;

        let role = if self.is_initiator {
            EscrowRole::Initiator
        } else {
            EscrowRole::Responder
        };
        let escrow_keys = EscrowKeys::derive(shared_key.as_bytes(), role);
        let encrypted_card = escrow_keys.encrypt_card(&card_bytes).ok()?;

        Some(RelayHandoff {
            gate_hash: hex::decode(&escrow_keys.gate_hash).ok()?,
            slot_hash: hex::decode(&escrow_keys.our_slot).ok()?,
            encrypted_card,
        })
    }

    fn fail_with_fallback(&mut self, reason: String) -> NfcHardwareOutcome {
        let relay_handoff = self.try_relay_handoff();
        self.step = NfcStep::Complete; // absorbing; no further events handled
        NfcHardwareOutcome::FailedWithFallback {
            reason,
            relay_handoff,
        }
    }
}

#[derive(Debug)]
pub(super) enum NfcFlowError {
    WrongState,
    Protocol(String),
}

// ── Screen builders ────────────────────────────────────────────────────────

/// Build a `ScreenModel` for any NFC sub-flow step. Phase 1 ships a
/// minimal placeholder shape — the production renderer copy follows
/// in a later phase once `NfcExchangeView` is retired. The screen-id
/// + cancel action are stable so iOS/Android can route on them today.
pub(super) fn build_nfc_screen(step: &NfcStep, progress: Progress) -> ScreenModel {
    let (screen_id, title, subtitle): (&str, &str, &str) = match step {
        NfcStep::Idle => ("exchange_nfc_idle", "Preparing NFC", "Just a moment..."),
        NfcStep::AwaitingTap => (
            "exchange_nfc_awaiting_tap",
            "Tap to exchange",
            "Hold your phones back-to-back",
        ),
        NfcStep::PayloadSent | NfcStep::AckSent => (
            "exchange_nfc_in_progress",
            "Exchanging cards",
            "Keep your phones still",
        ),
        NfcStep::Complete => ("exchange_nfc_complete", "Done", "Cards exchanged"),
    };

    ScreenModel {
        screen_id: screen_id.into(),
        title: title.into(),
        subtitle: Some(subtitle.into()),
        components: vec![Component::Text {
            id: "nfc_status".into(),
            content: subtitle.into(),
            style: TextStyle::Body,
        }],
        actions: vec![ScreenAction {
            id: "cancel".into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: !matches!(step, NfcStep::Complete),
            a11y: None,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: tests need pub(super) visibility of NfcStep,
// NfcHardwareOutcome, RelayHandoff, and NfcExchangeFlow internals.
// Integration tests with public-only surface land in
// core/vauchi-core/tests/it/nfc_exchange_flow_tests.rs in a follow-up
// commit.
#[cfg(test)]
mod tests {
    use super::*;
    use vauchi_core::Event;

    fn make_identity(name: &str) -> Identity {
        Identity::create(name, 0)
    }

    // @internal
    #[test]
    fn new_initiator_starts_idle() {
        let flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        assert_eq!(*flow.step(), NfcStep::Idle);
        assert!(flow.is_initiator);
    }

    // @internal
    #[test]
    fn new_responder_starts_idle() {
        let flow = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
        assert_eq!(*flow.step(), NfcStep::Idle);
        assert!(!flow.is_initiator);
    }

    // @internal
    #[test]
    fn initiator_activate_emits_nfc_activate_with_key_offer_payload() {
        let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        let commands = flow.activate().expect("activate");
        assert_eq!(*flow.step(), NfcStep::AwaitingTap);
        assert_eq!(commands.len(), 1);
        match &commands[0] {
            Command::NfcActivate { payload } => {
                assert_eq!(
                    payload.len(),
                    NFC_PAYLOAD_SIZE,
                    "initiator activation must carry exactly one ExchangeNfc payload"
                );
            }
            other => panic!("expected NfcActivate, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn responder_activate_emits_nfc_activate_with_empty_payload() {
        let mut flow = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
        let commands = flow.activate().expect("activate");
        assert_eq!(*flow.step(), NfcStep::AwaitingTap);
        match &commands[0] {
            Command::NfcActivate { payload } => {
                assert!(
                    payload.is_empty(),
                    "responder activation payload must be empty"
                );
            }
            other => panic!("expected NfcActivate, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn activate_from_non_idle_step_is_an_error() {
        let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        flow.activate().expect("first activate");
        let err = flow.activate().expect_err("second activate must fail");
        assert!(matches!(err, NfcFlowError::WrongState));
    }

    // @internal
    #[test]
    fn full_handshake_initiator_to_complete() {
        // Two flows simulate a real 3-phase exchange via direct
        // command/event plumbing (no NFC transport — events are
        // synthesised from the peer's emitted Commands).
        let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());

        // Phase 1: Alice activates (sends key offer), Bob activates (waits).
        let alice_cmds = alice.activate().expect("alice activate");
        let _ = bob.activate().expect("bob activate");
        let alice_offer = match &alice_cmds[0] {
            Command::NfcActivate { payload } => payload.clone(),
            other => panic!("expected NfcActivate, got {other:?}"),
        };

        // Bob receives Alice's offer.
        let bob_outcome = bob.handle_event(&Event::NfcDataReceived { data: alice_offer });
        let bob_response = match bob_outcome {
            NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
                Command::NfcSendApdu { data } => data.clone(),
                other => panic!("bob expected NfcSendApdu, got {other:?}"),
            },
            other => panic!("bob expected StepAdvanced, got {other:?}"),
        };
        assert_eq!(*bob.step(), NfcStep::AckSent);

        // Phase 2: Alice receives Bob's (key_ack || encrypted_card).
        let alice_outcome = alice.handle_event(&Event::NfcDataReceived { data: bob_response });
        let alice_card = match alice_outcome {
            NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
                Command::NfcSendApdu { data } => data.clone(),
                other => panic!("alice expected NfcSendApdu, got {other:?}"),
            },
            other => panic!("alice expected StepAdvanced, got {other:?}"),
        };
        assert_eq!(*alice.step(), NfcStep::PayloadSent);

        // Phase 3: Bob receives Alice's encrypted card → Complete.
        let bob_final = bob.handle_event(&Event::NfcDataReceived { data: alice_card });
        match bob_final {
            NfcHardwareOutcome::Complete {
                card_bytes,
                commands,
            } => {
                assert!(!card_bytes.is_empty());
                assert!(commands.iter().any(|c| matches!(c, Command::NfcDeactivate)));
            }
            other => panic!("bob expected Complete, got {other:?}"),
        }
        assert_eq!(*bob.step(), NfcStep::Complete);

        // Alice's terminating event: ACK of her Phase 3 send. The
        // payload bytes are irrelevant — confirm_send_success doesn't
        // parse them.
        let alice_final = alice.handle_event(&Event::NfcDataReceived {
            data: vec![0x90, 0x00],
        });
        match alice_final {
            NfcHardwareOutcome::Complete {
                card_bytes,
                commands,
            } => {
                assert!(!card_bytes.is_empty());
                assert!(commands.iter().any(|c| matches!(c, Command::NfcDeactivate)));
            }
            other => panic!("alice expected Complete, got {other:?}"),
        }
        assert_eq!(*alice.step(), NfcStep::Complete);
    }

    // @internal
    #[test]
    fn permission_denied_routes_to_fail_with_fallback() {
        let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        flow.activate().expect("activate");
        let outcome = flow.handle_event(&Event::PermissionDenied {
            transport: "nfc".into(),
        });
        match outcome {
            NfcHardwareOutcome::FailedWithFallback {
                reason,
                relay_handoff,
            } => {
                assert!(reason.to_lowercase().contains("permission"));
                // Pre-shared-key failure: no relay handoff available.
                assert!(relay_handoff.is_none());
            }
            other => panic!("expected FailedWithFallback, got {other:?}"),
        }
        // Absorbing state.
        assert_eq!(*flow.step(), NfcStep::Complete);
    }

    // @internal
    #[test]
    fn hardware_error_for_other_transport_is_ignored() {
        let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        flow.activate().expect("activate");
        let outcome = flow.handle_event(&Event::HardwareError {
            transport: "ble".into(),
            error: "ignored".into(),
        });
        assert!(matches!(outcome, NfcHardwareOutcome::Ignored));
        assert_eq!(*flow.step(), NfcStep::AwaitingTap);
    }

    // @internal
    #[test]
    fn responder_failure_after_key_ack_yields_relay_handoff() {
        // Drive Bob into AckSent / KeyAckReceived by feeding Alice's
        // key offer to him.
        let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
        let alice_cmds = alice.activate().expect("alice activate");
        let _ = bob.activate().expect("bob activate");
        let offer = match &alice_cmds[0] {
            Command::NfcActivate { payload } => payload.clone(),
            other => panic!("expected NfcActivate, got {other:?}"),
        };
        let bob_outcome = bob.handle_event(&Event::NfcDataReceived { data: offer });
        assert!(matches!(
            bob_outcome,
            NfcHardwareOutcome::StepAdvanced { .. }
        ));
        assert_eq!(*bob.step(), NfcStep::AckSent);

        // Trigger a hardware error after the shared key exists.
        let outcome = bob.handle_event(&Event::HardwareError {
            transport: "nfc".into(),
            error: "tag lost mid-exchange".into(),
        });
        match outcome {
            NfcHardwareOutcome::FailedWithFallback {
                reason,
                relay_handoff,
            } => {
                assert!(reason.contains("tag lost"));
                let handoff =
                    relay_handoff.expect("post-shared-key failure must yield a relay handoff");
                assert_eq!(handoff.gate_hash.len(), 32, "gate_hash is SHA-256");
                assert_eq!(handoff.slot_hash.len(), 32, "slot_hash is SHA-256");
                assert!(
                    !handoff.encrypted_card.is_empty(),
                    "encrypted_card must carry the blob"
                );
            }
            other => panic!("expected FailedWithFallback, got {other:?}"),
        }
    }

    // ── Screen-builder coverage ────────────────────────────────────────────

    fn dummy_progress() -> Progress {
        Progress {
            current_step: 4,
            total_steps: 8,
            label: None,
        }
    }

    fn action_ids(screen: &ScreenModel) -> Vec<String> {
        screen.actions.iter().map(|a| a.id.clone()).collect()
    }

    // @internal
    #[test]
    fn idle_screen_has_cancel_affordance() {
        let s = build_nfc_screen(&NfcStep::Idle, dummy_progress());
        assert_eq!(s.screen_id, "exchange_nfc_idle");
        assert_eq!(action_ids(&s), vec!["cancel".to_string()]);
        assert!(s.actions.iter().any(|a| a.id == "cancel" && a.enabled));
    }

    // @internal
    #[test]
    fn awaiting_tap_screen_has_cancel_affordance() {
        let s = build_nfc_screen(&NfcStep::AwaitingTap, dummy_progress());
        assert_eq!(s.screen_id, "exchange_nfc_awaiting_tap");
        assert_eq!(action_ids(&s), vec!["cancel".to_string()]);
        assert!(s.actions.iter().any(|a| a.id == "cancel" && a.enabled));
    }

    // @internal
    #[test]
    fn in_progress_screens_share_screen_id_and_keep_cancel_enabled() {
        let sent = build_nfc_screen(&NfcStep::PayloadSent, dummy_progress());
        let ack = build_nfc_screen(&NfcStep::AckSent, dummy_progress());
        assert_eq!(sent.screen_id, "exchange_nfc_in_progress");
        assert_eq!(ack.screen_id, "exchange_nfc_in_progress");
        assert_eq!(action_ids(&sent), vec!["cancel".to_string()]);
        assert_eq!(action_ids(&ack), vec!["cancel".to_string()]);
    }

    // @internal
    #[test]
    fn complete_screen_disables_cancel() {
        let s = build_nfc_screen(&NfcStep::Complete, dummy_progress());
        assert_eq!(s.screen_id, "exchange_nfc_complete");
        // Cancel is still listed (so the action surface is stable across
        // states) but disabled — the exchange has already completed.
        assert_eq!(action_ids(&s), vec!["cancel".to_string()]);
        assert!(s.actions.iter().any(|a| a.id == "cancel" && !a.enabled));
    }

    // ── CC-13 proptest: Complete is absorbing ──────────────────────────────
    //
    // Engine-walker reachability tests (the CC-22 pattern used by
    // `core/vauchi-app/tests/reachability/exchange_ble.rs`) are deferred
    // to a later phase: the production entry path through ExchangeEngine
    // — `ExchangeMode::Nfc` + `start_nfc_mode` — has not been added yet
    // (this Phase 1 ships test-reachable wiring only, per the engine-
    // graduation record's Phase 4). The walker can't BFS into NFC steps
    // without that entry, so we exercise the sub-flow's invariants at the
    // unit-test layer instead.

    use proptest::prelude::*;

    fn arb_post_complete_event() -> impl Strategy<Value = Event> {
        prop_oneof![
            // NFC data could keep arriving after Complete (stale APDUs,
            // re-tap, etc.) — must not re-open the state.
            (any::<Vec<u8>>()).prop_map(|data| Event::NfcDataReceived { data }),
            // Transport-level errors scoped to NFC.
            Just(Event::HardwareError {
                transport: "nfc".into(),
                error: "spurious".into(),
            }),
            Just(Event::PermissionDenied {
                transport: "nfc".into(),
            }),
            Just(Event::HardwareUnavailable {
                transport: "nfc".into(),
            }),
            // Cross-transport noise — must be Ignored.
            Just(Event::BleDeviceDiscovered {
                id: "stray".into(),
                rssi: -50,
                adv_data: vec![],
            }),
            Just(Event::HardwareError {
                transport: "ble".into(),
                error: "stray".into(),
            }),
        ]
    }

    fn drive_to_complete() -> NfcExchangeFlow {
        // Use the happy-path initiator drive (mirrors
        // `full_handshake_initiator_to_complete`).
        let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
        let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
        let alice_cmds = alice.activate().expect("alice activate");
        let _ = bob.activate().expect("bob activate");
        let offer = match &alice_cmds[0] {
            Command::NfcActivate { payload } => payload.clone(),
            _ => unreachable!(),
        };
        let bob_outcome = bob.handle_event(&Event::NfcDataReceived { data: offer });
        let bob_response = match bob_outcome {
            NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
                Command::NfcSendApdu { data } => data.clone(),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        let alice_outcome = alice.handle_event(&Event::NfcDataReceived { data: bob_response });
        let alice_card = match alice_outcome {
            NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
                Command::NfcSendApdu { data } => data.clone(),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        // Phase 3: Bob receives Alice's encrypted card → Complete.
        let _ = bob.handle_event(&Event::NfcDataReceived { data: alice_card });
        let _ = alice.handle_event(&Event::NfcDataReceived {
            data: vec![0x90, 0x00],
        });
        // Both Alice and Bob are now Complete; return Alice so the
        // proptest exercises an initiator that has finished its
        // handshake (Phase 3 confirm path).
        alice
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// CC-13 invariant: once `NfcExchangeFlow` is in `Complete`, no
        /// random subsequent event transitions it away. Symptom this
        /// would catch: a late `NfcDataReceived` after Complete
        /// silently re-running the handshake from a partial state, or
        /// a stray hardware error flipping the step back to a
        /// non-terminal value.
        // @internal
        #[test]
        fn complete_is_absorbing(events in prop::collection::vec(arb_post_complete_event(), 0..20)) {
            let mut flow = drive_to_complete();
            prop_assert_eq!(flow.step(), &NfcStep::Complete);
            for event in &events {
                let _ = flow.handle_event(event);
                prop_assert_eq!(
                    flow.step(),
                    &NfcStep::Complete,
                    "Complete must be absorbing; event {:?} transitioned out",
                    event,
                );
            }
        }
    }
}
