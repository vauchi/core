// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-owned multi-stage exchange (slice 32m T1.2b).
//!
//! The poll-driven [`MultiStageMachine`] lives on `AppEngine` so every
//! frontend — mobile (`PlatformAppEngine` delegates here) and
//! desktop/C-ABI (`vauchi-cabi` wraps `AppEngine` directly) — shares
//! one source of truth. There is no cycle thread and no listener
//! bridge.
//!
//! Mirrors the slice 32l device-link-initiator integration
//! ([`super::device_link_initiator`]) verbatim. The 32l Phase 1a / 1b
//! split applies here too:
//!
//! - **T1.2b (Phase 1a equivalent):** add the AppEngine integration
//!   so cabi/windows get the poll-driven path. PlatformAppEngine
//!   still owns its parallel cycle-thread bridge — would double-drive
//!   on mobile.
//! - **T1.2c (Phase 1b equivalent):** PlatformAppEngine dedup —
//!   stops calling `MobileMultiStageSession::start()` /
//!   `ensure_multi_stage_session` on screen entry. AppEngine becomes
//!   the sole driver.
//! - **T3.1 (Phase 4 equivalent):** delete the cycle-thread
//!   machinery in `vauchi-platform/src/multistage_exchange.rs`.
//!
//! - [`AppEngine::sync_multi_stage_lifecycle`] ensures / cancels the
//!   machine on entry / exit of [`AppScreen::MultiStageExchange`]
//!   (called from `navigate_to_internal`, mirroring the device-link
//!   lifecycle).
//! - [`AppEngine::advance_multi_stage_session`] runs one non-blocking
//!   protocol step; called from `poll_notifications`.
//! - [`AppEngine::apply_multi_stage_event`] maps a [`MultiStageEvent`]
//!   onto the existing `MultiStageExchangeEngine::set_*` setters (the
//!   same bridge entry points the cycle thread used pre-32m).

use super::{AppEngine, AppScreen};
use crate::orchestrator::multi_stage_machine::{
    MultiStageEvent, MultiStageMachine, MultiStagePhase,
};
use vauchi_core::Command;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::AccelerometerProximityState;
use vauchi_core::exchange::AudioProximityState;
use vauchi_core::exchange::mode::ExchangeMode;

/// Exchange payload format version byte. Mirrors the constant in
/// `vauchi-platform/src/mobile_exchange.rs` (which T3.1 retires);
/// kept inline here so vauchi-app doesn't depend on the binding
/// crate. The wire format is shared with the cycle-thread path
/// during T1.2b/T1.2c coexistence — both serializers must agree
/// byte-for-byte.
const EXCHANGE_PAYLOAD_VERSION: u8 = 1;

/// Serialize identity public key + contact card into the
/// multi-stage exchange payload `MultiStageSession::new` expects.
/// Format: `[version: 1][public_key: 32][card_json: rest]`.
fn serialize_exchange_payload(public_key: &[u8; 32], card: &ContactCard) -> Vec<u8> {
    let card_json = serde_json::to_vec(card).expect("ContactCard serialization should not fail");
    let mut payload = Vec::with_capacity(1 + 32 + card_json.len());
    payload.push(EXCHANGE_PAYLOAD_VERSION);
    payload.extend_from_slice(public_key);
    payload.extend_from_slice(&card_json);
    payload
}

/// Wraps the machine + the local exchange payload it was constructed
/// for. The payload is rebuilt on each session (the identity may
/// have rotated) so we don't cache it across screen entries.
pub(crate) struct MultiStageHolder {
    machine: MultiStageMachine,
}

impl AppEngine {
    /// Build / cancel the multi-stage machine on entry / exit of the
    /// `MultiStageExchange` screen. Mirrors `sync_device_link_lifecycle`.
    pub(super) fn sync_multi_stage_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::MultiStageExchange { .. });
        let is = matches!(new, AppScreen::MultiStageExchange { .. });
        match (was, is) {
            (true, false) => self.cancel_multi_stage_session(),
            (false, true) => {
                if let AppScreen::MultiStageExchange { mode } = new {
                    self.ensure_multi_stage_session(*mode);
                }
            }
            _ => {}
        }
    }

    /// Lazily build the machine. Idempotent: a no-op when a machine
    /// is already held. Build errors (missing identity, missing card)
    /// are non-fatal — the screen falls back to the empty engine and
    /// the next navigation attempt re-tries.
    ///
    /// `pub` so the platform layer can rebuild the machine for an
    /// in-place retry without an intermediate navigation
    /// (`PlatformAppEngine::handle_action`'s retry branch in T1.2c).
    pub fn ensure_multi_stage_session(&mut self, mode: ExchangeMode) {
        if self.multi_stage_session.is_some() {
            return;
        }
        let payload = match self.build_multi_stage_payload() {
            Some(p) => p,
            None => {
                log::warn!("multi-stage: cannot start session — no identity / card");
                return;
            }
        };
        // Milliseconds — the MultiStageMachine's per-frame gate compares
        // `now` against `display_duration_ms`. Seconds here froze the QR
        // for ~1000× its window and deadlocked the exchange
        // (2026-06-03-multistage-qr-exchange-stalls-init-on-device).
        let now = self.vauchi.clock().unix_millis();
        let machine = match mode {
            ExchangeMode::Hover => MultiStageMachine::new_hover(payload, now),
            ExchangeMode::TapHoverShake => MultiStageMachine::new_tap_hover_shake(payload, now),
            // Every remaining mode that lands on this screen (Glance)
            // maps to the Glance constructor — no proximity handshake.
            _ => MultiStageMachine::new_glance(payload, now),
        };
        self.multi_stage_session = Some(MultiStageHolder { machine });
    }

    /// Cancel + drop the active machine. Idempotent. `pub` so
    /// binding crates can cancel without a nav-out (matches the
    /// 32l `cancel_device_link_session` signature).
    pub fn cancel_multi_stage_session(&mut self) {
        if let Some(mut holder) = self.multi_stage_session.take() {
            let _ = holder.machine.cancel();
        }
    }

    /// One non-blocking protocol step. Called from
    /// `poll_notifications`. Returns true if the engine's
    /// `ScreenModel` was updated.
    pub(crate) fn advance_multi_stage_session(&mut self) -> bool {
        // Milliseconds — the MultiStageMachine's per-frame gate compares
        // `now` against `display_duration_ms`. Seconds here froze the QR
        // for ~1000× its window and deadlocked the exchange
        // (2026-06-03-multistage-qr-exchange-stalls-init-on-device).
        let now = self.vauchi.clock().unix_millis();
        let event = match self.multi_stage_session.as_mut() {
            Some(holder) => holder.machine.advance(now),
            None => return false,
        };
        self.apply_multi_stage_event(event)
    }

    /// Translate a frontend-emitted [`vauchi_core::Event`] into a
    /// machine transition. Returns the matching `MultiStageEvent` for
    /// `apply_multi_stage_event` consumption; returns
    /// `MultiStageEvent::None` if no machine is active.
    ///
    /// `PlatformAppEngine` / cabi route the platform-emitted
    /// `Event::QrScanned` / `Event::AudioSamplesRecorded` here from
    /// the existing `handle_hardware_event` entry point. T1.2c removes
    /// the parallel cycle-thread route from `PlatformAppEngine`.
    ///
    /// T1.2c calls this from `PlatformAppEngine` for the QrScanned
    /// hardware event and the `peer_scan` TextChanged UserAction.
    pub fn forward_multi_stage_hardware_event(
        &mut self,
        event: &vauchi_core::Event,
    ) -> MultiStageEvent {
        // Milliseconds — the MultiStageMachine's per-frame gate compares
        // `now` against `display_duration_ms`. Seconds here froze the QR
        // for ~1000× its window and deadlocked the exchange
        // (2026-06-03-multistage-qr-exchange-stalls-init-on-device).
        let now = self.vauchi.clock().unix_millis();
        match self.multi_stage_session.as_mut() {
            Some(holder) => holder.machine.handle_hardware_event(event, now),
            None => MultiStageEvent::None,
        }
    }

    /// Map a [`MultiStageEvent`] onto the engine's
    /// `MultiStageExchangeEngine::set_*` setters (the same bridge
    /// entry points the cycle thread used pre-32m). Returns true if
    /// the engine accepted the update.
    ///
    /// `pub` so the platform layer can consume a `MultiStageEvent`
    /// produced by `forward_multi_stage_hardware_event` in the
    /// QrScanned + peer-scan routes.
    pub fn apply_multi_stage_event(&mut self, event: MultiStageEvent) -> bool {
        // Hover-only side effect: when the protocol reports
        // `PeerDiscovered` the orchestrator fires the audio
        // handshake. Mirrors the cycle-thread's
        // `try_autonomous_audio_trigger` (Phase 1.C.3e-vi). Glance
        // sessions return `None` from `try_audio_handshake_start`
        // because they don't enter the `Listening` state.
        if matches!(event, MultiStageEvent::PeerDiscovered)
            && let Some(holder) = self.multi_stage_session.as_mut()
        {
            let cmds = holder.machine.try_audio_handshake_start();
            if !cmds.is_empty() {
                self.extend_pending_commands(cmds);
                let _ = self.apply_multi_stage_audio_proximity(AudioProximityState::Listening);
            }
        }
        // TapHoverShake-only: on the same `PeerDiscovered` edge, start the
        // accelerometer shake capture. `try_accel_capture_start` is mode-gated
        // (Glance/Hover return `[]`) and idempotent, so this is a no-op for
        // every other mode and on repeat discovery events.
        if matches!(event, MultiStageEvent::PeerDiscovered)
            && let Some(holder) = self.multi_stage_session.as_mut()
        {
            let accel_cmds = holder.machine.try_accel_capture_start();
            if !accel_cmds.is_empty() {
                self.extend_pending_commands(accel_cmds);
                let _ =
                    self.apply_multi_stage_accel_proximity(AccelerometerProximityState::Listening);
            }
        }
        match event {
            MultiStageEvent::None => false,
            MultiStageEvent::QrFrameReady(payload) => self.apply_multi_stage_qr_payload(&payload),
            MultiStageEvent::AudioProximityChanged(state) => {
                self.apply_multi_stage_audio_proximity(state)
            }
            MultiStageEvent::AccelProximityChanged(state) => {
                self.apply_multi_stage_accel_proximity(state)
            }
            MultiStageEvent::PeerDiscovered => {
                self.apply_multi_stage_state(vauchi_core::exchange::ProtocolState::Discovered)
            }
            MultiStageEvent::TransferProgress {
                chunks_sent,
                chunks_total,
                chunks_received,
                peer_chunks_total,
            } => self.apply_multi_stage_state(vauchi_core::exchange::ProtocolState::Transferring {
                chunks_sent,
                chunks_total,
                chunks_received,
                peer_chunks_total,
            }),
            MultiStageEvent::Verifying => {
                self.apply_multi_stage_state(vauchi_core::exchange::ProtocolState::Verifying)
            }
            MultiStageEvent::Confirming => {
                self.apply_multi_stage_state(vauchi_core::exchange::ProtocolState::Confirming)
            }
            MultiStageEvent::Finalized { peer_name } => {
                let state_applied =
                    self.apply_multi_stage_state(vauchi_core::exchange::ProtocolState::Finalized);
                // Persist the exchanged contact now (atomic — both sides
                // confirmed). Was the cycle thread's `on_finalized` job;
                // the poll path dropped it (Part B of the stall bug).
                self.persist_exchanged_contact();
                if !peer_name.is_empty() {
                    let _ = self.apply_multi_stage_finalized(peer_name);
                }
                state_applied
            }
            MultiStageEvent::Completed => self.apply_multi_stage_session_ended(),
            MultiStageEvent::Failed { reason } => {
                self.apply_multi_stage_state(vauchi_core::exchange::ProtocolState::Failed(reason))
            }
        }
    }

    /// Whether a multi-stage machine is currently held. Used by the
    /// platform-side dedup (T1.2c) to skip its cycle-thread spawn
    /// when the AppEngine already owns the session.
    pub fn multi_stage_session_active(&self) -> bool {
        self.multi_stage_session.is_some()
    }

    /// Read the held machine's current phase, if any. Test seam +
    /// cabi/windows screen-rendering helper.
    pub fn multi_stage_phase(&self) -> Option<MultiStagePhase> {
        self.multi_stage_session.as_ref().map(|h| h.machine.phase())
    }

    /// Drain any commands the machine emitted that aren't routed
    /// through the standard `extend_pending_commands` path. T1.2b /
    /// T1.2c emit all commands via `apply_multi_stage_event`
    /// directly into `AppEngine::pending_commands`; this method is
    /// kept for symmetry with the device-link API and may surface
    /// future fan-out (e.g. test-driven probes) without a new public
    /// drain.
    #[doc(hidden)]
    pub fn drain_multi_stage_commands(&mut self) -> Vec<Command> {
        Vec::new()
    }

    /// Build the local exchange payload (identity public key + own
    /// card) required by [`MultiStageMachine::new_glance`] /
    /// [`MultiStageMachine::new_hover`].
    ///
    /// Returns `None` when no identity exists (the screen falls back
    /// to the engine's default chrome — the user must complete
    /// onboarding before reaching this screen anyway).
    fn build_multi_stage_payload(&self) -> Option<Vec<u8>> {
        let identity = self.vauchi.identity()?;
        let signing_key = *identity.signing_public_key();
        let display_name = identity.display_name().to_string();
        let card = self
            .vauchi
            .own_card()
            .ok()
            .flatten()
            .unwrap_or_else(|| ContactCard::new(&display_name));
        Some(serialize_exchange_payload(&signing_key, &card))
    }

    /// Persist the just-finalized peer as an exchanged contact + ratchet.
    ///
    /// Restores the terminal-side job the retired cycle thread's
    /// `on_finalized` listener used to do. The poll path dropped it, so
    /// the exchange finalized internally but no contact was ever saved
    /// (2026-06-03-multistage-qr-exchange-stalls-init-on-device, Part B).
    ///
    /// Best-effort: a missing payload / malformed card / storage error
    /// leaves the exchange finalized on-screen but uncreated — never
    /// panics on the hot finalize path. `get_received_data` only returns
    /// `Some` in `Finalized` (both sides confirmed), so this is atomic.
    fn persist_exchanged_contact(&mut self) {
        // Pull the session-owned bytes off under a short borrow so the
        // `self.vauchi` persistence calls below don't fight the borrow.
        let Some((payload, transport_key)) = self.multi_stage_session.as_ref().and_then(|h| {
            Some((
                h.machine.received_exchange_payload()?,
                h.machine.transport_key()?,
            ))
        }) else {
            return;
        };
        // `[version: 1][peer_pk: 32][card_json: rest]` — mirrors
        // `serialize_exchange_payload`.
        if payload.len() < 33 || payload[0] != EXCHANGE_PAYLOAD_VERSION {
            return;
        }
        let Ok(peer_pk) = <[u8; 32]>::try_from(&payload[1..33]) else {
            return;
        };
        let Ok(card) = serde_json::from_slice::<ContactCard>(&payload[33..]) else {
            return;
        };
        // Pull the success-screen content off the peer card before it is
        // moved into the contact (2026-06-04-exchange-terminal-screens).
        let peer_name = card.display_name().to_string();
        let received_fields: Vec<(String, String, String)> = card
            .fields()
            .iter()
            .map(|f| {
                (
                    format!("{:?}", f.field_type()),
                    f.label().to_string(),
                    f.value().to_string(),
                )
            })
            .collect();
        let Some(identity) = self.vauchi.identity() else {
            return;
        };
        let our_identity = *identity.signing_public_key();
        let now = self.vauchi.clock().unix_seconds();
        let contact = vauchi_core::Contact::from_exchange(
            peer_pk,
            card,
            vauchi_core::crypto::SymmetricKey::from_bytes(transport_key),
            now,
        );
        let contact_id = contact.id().to_string();
        if self.vauchi.add_contact(contact).is_err() {
            return;
        }
        // Persist the role-correct Double Ratchet so future card updates
        // from this contact decrypt. Returns owned data, so the session
        // borrow ends before the `self.vauchi` save.
        let ratchet = self
            .multi_stage_session
            .as_ref()
            .and_then(|h| h.machine.build_exchange_ratchet(&our_identity, &peer_pk));
        if let Some((ratchet, is_initiator)) = ratchet
            && self
                .vauchi
                .save_exchange_ratchet(&contact_id, &ratchet, is_initiator)
                .is_err()
        {
            log::warn!("multi-stage: failed to persist exchange ratchet");
        }

        // Capture-at-exchange (ADR-051): ask the frontend for the current
        // location so we can record where this contact was met. The reply
        // (Event::LocationResult) is consumed in handle_hardware_event.
        self.request_exchange_location(contact_id.clone());

        // Assemble the rich success screen: what they shared (above),
        // which of our own fields this new contact can now see, and the
        // group(s) they joined (none on the multi-stage path yet).
        let my_visible_fields: Vec<String> = self
            .vauchi
            .own_card()
            .ok()
            .flatten()
            .map(|own| {
                own.fields()
                    .iter()
                    .filter(|f| {
                        self.vauchi
                            .get_effective_field_visibility(&contact_id, f.id())
                            .unwrap_or(false)
                    })
                    .map(|f| f.label().to_string())
                    .collect()
            })
            .unwrap_or_default();
        // Assign the contact to the groups chosen in the preamble and
        // resolve their names for the success screen (best-effort: a
        // group failure doesn't block the exchange).
        let pending_groups = std::mem::take(&mut self.pending_exchange_groups);
        let mut group_names = Vec::new();
        for group_id in &pending_groups {
            if self
                .vauchi
                .add_contact_to_group(group_id, &contact_id)
                .is_ok()
                && let Ok(group) = self.vauchi.get_group(group_id)
            {
                group_names.push(group.name().to_string());
            }
        }
        let summary = crate::ui::exchange::success::ExchangeSuccessSummary {
            peer_name,
            received_fields,
            my_visible_fields,
            group_names,
        };
        self.apply_multi_stage_success_summary(summary);
    }
}
