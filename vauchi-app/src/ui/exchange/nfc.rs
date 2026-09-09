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
//!
//! Wiring status (2026-05-29): the initiator path is reachable
//! (`ExchangeMode::TapTap` -> `start_taptap_mode` -> `new_initiator`); the
//! responder path is wired lazily on the first `NfcDataReceived` (HCE
//! bootstrap in `ExchangeEngine::handle_hardware_event`, which spins up a
//! `new_responder` flow so the engine — not the legacy `MobileExchangeSession`
//! — drives the HCE handshake). The file-level `dead_code` allow now only
//! covers Phase-1 outcome-enum items not yet consumed. See
//! `_private/docs/problems/2026-05-29-nfc-exchange-mode-entry-wiring`.

#![allow(dead_code)] // Phase-1 outcome-enum follow-ups not yet consumed:
// NfcHardwareOutcome::{Complete.card_bytes, Consumed} + RelayHandoff. The
// responder path itself is now wired (see module docs).

use vauchi_core::clock::SystemClock;
use vauchi_core::exchange::escrow::{EscrowKeys, EscrowRole};
use vauchi_core::exchange::nfc_apdu::{self, ApduCommand, ChainReassembler, SW_SUCCESS};
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
    /// (Responder only) collects chained EXCHANGE chunks off the raw wire.
    reassembler: ChainReassembler,
}

impl NfcExchangeFlow {
    pub(super) fn new_initiator(identity: Identity, display_name: String) -> Self {
        let session = NfcHandshakeSession::new_initiator(&identity, display_name);
        Self {
            step: NfcStep::Idle,
            session,
            is_initiator: true,
            identity,
            reassembler: ChainReassembler::new(),
        }
    }

    pub(super) fn new_responder(identity: Identity, display_name: String) -> Self {
        let session = NfcHandshakeSession::new_responder(&identity, display_name);
        Self {
            step: NfcStep::Idle,
            session,
            is_initiator: false,
            identity,
            reassembler: ChainReassembler::new(),
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
        let (payload, apdus) = if self.is_initiator {
            let payload = self
                .session
                .create_key_offer(&self.identity, now)
                .map_err(|e| NfcFlowError::Protocol(e.to_string()))?;
            let apdus = nfc_apdu::frame_command(&payload)
                .map_err(|e| NfcFlowError::Protocol(e.to_string()))?;
            (payload, apdus)
        } else {
            (Vec::new(), Vec::new())
        };
        self.step = NfcStep::AwaitingTap;
        Ok(vec![Command::NfcActivate { payload, apdus }])
    }

    /// Process a hardware event. Failure events (`NfcFailed`, plus
    /// `HardwareError`/`HardwareUnavailable`/`PermissionDenied` with
    /// `transport == "nfc"`) are short-circuited at the top — pattern
    /// mirrored from `BleExchangeFlow::handle_event`. Raw wire bytes
    /// (`NfcApduReceived`) are decoded here by role; the legacy
    /// `NfcDataReceived` keeps the shape today's shells deliver
    /// (`data || SW` from a reader, bare payload from the HCE side).
    pub(super) fn handle_event(&mut self, event: &Event) -> NfcHardwareOutcome {
        if let Event::NfcFailed { reason } = event {
            return self.fail_with_fallback(reason.clone());
        }
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

        if matches!(self.step, NfcStep::Idle | NfcStep::Complete) {
            return NfcHardwareOutcome::Ignored;
        }
        match event {
            Event::NfcApduReceived { bytes } => self.handle_raw_apdu(bytes),
            Event::NfcDataReceived { data } if self.is_initiator => {
                self.handle_reader_response(data)
            }
            Event::NfcDataReceived { data } => self.handle_peer_payload(data),
            _ => NfcHardwareOutcome::Ignored,
        }
    }

    fn handle_raw_apdu(&mut self, bytes: &[u8]) -> NfcHardwareOutcome {
        if self.is_initiator {
            return self.handle_reader_response(bytes);
        }
        match nfc_apdu::decode_command(bytes) {
            Ok(ApduCommand::Select) => Self::acknowledge(),
            Ok(ApduCommand::Exchange { data, more }) => match self.reassembler.push(&data, more) {
                Ok(Some(payload)) => self.handle_peer_payload(&payload),
                Ok(None) => Self::acknowledge(),
                Err(e) => self.fail_with_fallback(e.to_string()),
            },
            Ok(ApduCommand::Unknown { ins }) => {
                self.fail_with_fallback(format!("Unsupported NFC command {ins:02X}"))
            }
            Err(e) => self.fail_with_fallback(e.to_string()),
        }
    }

    fn handle_reader_response(&mut self, bytes: &[u8]) -> NfcHardwareOutcome {
        match nfc_apdu::decode_response(bytes) {
            Ok(data) => self.handle_peer_payload(&data),
            Err(e) => self.fail_with_fallback(e.to_string()),
        }
    }

    fn handle_peer_payload(&mut self, data: &[u8]) -> NfcHardwareOutcome {
        match self.step {
            NfcStep::AwaitingTap => self.handle_awaiting_tap(data),
            NfcStep::PayloadSent => self.handle_payload_sent_complete(),
            NfcStep::AckSent => self.handle_ack_sent(data),
            NfcStep::Idle | NfcStep::Complete => NfcHardwareOutcome::Ignored,
        }
    }

    /// The HCE side must answer every command APDU or the OS times the
    /// tap out; SELECT and non-final chunks get a bare `90 00`.
    fn acknowledge() -> NfcHardwareOutcome {
        NfcHardwareOutcome::Consumed {
            commands: vec![Self::status_command(SW_SUCCESS)],
        }
    }

    fn status_command(sw: [u8; 2]) -> Command {
        let response = nfc_apdu::status_response(sw);
        Command::NfcSendApdu {
            data: response.clone(),
            apdus: vec![response],
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
            let our_encrypted_card =
                match self.session.process_key_ack(key_ack, encrypted_card, now) {
                    Ok(card) => card,
                    Err(e) => return self.fail_with_fallback(e.to_string()),
                };
            match nfc_apdu::frame_exchange(&our_encrypted_card) {
                Ok(apdus) => {
                    self.step = NfcStep::PayloadSent;
                    NfcHardwareOutcome::StepAdvanced {
                        commands: vec![Command::NfcSendApdu {
                            data: our_encrypted_card,
                            apdus,
                        }],
                    }
                }
                Err(e) => self.fail_with_fallback(e.to_string()),
            }
        } else {
            // Responder: `data` is the initiator's key offer.
            let (our_ack, our_encrypted_card) =
                match self.session.process_key_offer(&self.identity, data, now) {
                    Ok(reply) => reply,
                    Err(e) => return self.fail_with_fallback(e.to_string()),
                };
            let mut reply = our_ack;
            reply.extend(our_encrypted_card);
            match nfc_apdu::frame_response(&reply) {
                Ok(response) => {
                    self.step = NfcStep::AckSent;
                    NfcHardwareOutcome::StepAdvanced {
                        commands: vec![Command::NfcSendApdu {
                            data: response.clone(),
                            apdus: vec![response],
                        }],
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
                tracing::info!("[Exchange] NFC exchange complete");
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
        //
        // Phase 3 terminal-ACK: emit `Command::NfcSendApdu { 0x9000 }`
        // before `NfcDeactivate` so HCE's binder thread always returns
        // via the same channel as Phase 1 / Phase 2 responses. Without
        // this, Android HCE would have no APDU response for the final
        // Phase-3 inbound APDU and the OS would treat the exchange as
        // timed-out at the OS level (~125ms hardware deadline) even
        // though the engine completed successfully. iOS initiator side
        // ignores the extra `NfcSendApdu` — the legacy `NfcDeactivate`
        // arrives immediately after and tears the session down. See
        // `2026-05-20-nfc-hce-responder-sync-boundary/README.md`
        // §"Terminal-phase ACK" option (a).
        match self.session.process_encrypted_card(data) {
            Ok(result) => {
                self.step = NfcStep::Complete;
                tracing::info!("[Exchange] NFC exchange complete");
                NfcHardwareOutcome::Complete {
                    card_bytes: result
                        .remote_card
                        .to_bytes()
                        .expect("NfcCardPayload re-serialization is infallible by construction"),
                    commands: vec![Self::status_command(SW_SUCCESS), Command::NfcDeactivate],
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
pub(super) fn build_nfc_screen(step: &NfcStep, locale: crate::i18n::Locale) -> ScreenModel {
    let t = |key: &str| crate::i18n::get_string(locale, key);
    let (screen_id, title_key, subtitle_key): (&str, &str, &str) = match step {
        NfcStep::Idle => (
            "exchange_nfc_idle",
            "exchange.nfc.preparing_title",
            "exchange.nfc.preparing_subtitle",
        ),
        NfcStep::AwaitingTap => (
            "exchange_nfc_awaiting_tap",
            "exchange.nfc.awaiting_tap_title",
            "exchange.nfc.awaiting_tap_subtitle",
        ),
        NfcStep::PayloadSent | NfcStep::AckSent => (
            "exchange_nfc_in_progress",
            "exchange.nfc.in_progress_title",
            "exchange.nfc.in_progress_subtitle",
        ),
        NfcStep::Complete => (
            "exchange_nfc_complete",
            "exchange.nfc.complete_title",
            "exchange.nfc.complete_subtitle",
        ),
    };
    let title = t(title_key);
    let subtitle = t(subtitle_key);

    ScreenModel {
        screen_id: screen_id.into(),
        title: title.clone(),
        subtitle: Some(subtitle.clone()),
        components: vec![Component::Text {
            a11y: None,
            id: "nfc_status".into(),
            content: subtitle,
            style: TextStyle::Body,
        }],
        contextual_actions: vec![ScreenAction {
            id: "cancel".into(),
            label: t("action.cancel"),
            style: ActionStyle::Secondary,
            enabled: !matches!(step, NfcStep::Complete),
            a11y: None,
        }],
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
#[path = "nfc_tests.rs"]
mod tests;
