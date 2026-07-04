// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dedicated core-driven Humble engine for `ExchangeMode::Cable` (USB cable /
//! direct-TCP exchange).
//!
//! Graduation Slice 1–2 (`_private/docs/problems/2026-05-11-direct-transport-engine-graduation/`).
//! Unlike BLE/NFC (which wrap a self-contained flow struct), Cable has none —
//! it owns an [`ExchangeSession`] (`new_usb`) directly and drives the
//! security-critical completion machinery the legacy `ExchangeEngine` used to
//! host: the USB auto-advance, the reciprocity-escrow routing, the success
//! summary, and the Complete→Verifying/Success/Failed state sync (moved
//! verbatim from `super`'s `handle_hardware_event`, `mod.rs:777-867`).
//!
//! ## The USB ceremony (two event-driven phases — see the implementation plan)
//!
//! - **Phase A** — on [`Event::DirectPayloadReceived`]: the session parses the
//!   peer's `ExchangeQR` (→ `AwaitingKeyAgreement`), then the engine
//!   auto-advances `PerformKeyAgreement`. For USB that derives the shared key,
//!   sets proximity High (the cable IS the proximity proof), transitions to
//!   `AwaitingCardExchange`, and emits `Command::DirectSendCard` (our encrypted
//!   card). No separate `ProximityCheckCompleted` — key agreement sets it.
//! - **Phase B** — on [`Event::DirectCardReceived`]: the session decrypts the
//!   peer's card and completes the exchange internally. The engine then runs the
//!   shared completion machinery (reciprocity confirmer + success summary +
//!   state sync).
//!
//! The session is `Option`: the factory degrades gracefully to a Failed screen
//! when no identity/own-card is available (mirrors the legacy `ExchangeEngine`
//! factory contract, `screens.rs:300` — a missing identity is a should-never-
//! happen contract violation, not a panic). The legacy parent retirement lands
//! in slice 3.

use std::sync::Arc;

use crate::ui::reciprocity_confirmer::ReciprocityConfirmer;
use crate::ui::*;
use vauchi_core::clock::Clock;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::reciprocity::Reciprocity;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeSession, ExchangeState, ManualConfirmationVerifier, UsbRole,
};
use vauchi_core::identity::Identity;
use vauchi_core::types::ExchangeTransport;
use vauchi_core::{Command, Contact, Event};

/// Action id for the Cancel button (any non-terminal screen + the failed screen).
pub const ACTION_CANCEL: &str = "cancel";
/// Action id for the Retry button on the failed screen.
pub const ACTION_RETRY: &str = "retry";
/// Action id for the Done button on the success screen.
pub const ACTION_DONE: &str = "done";

/// How long the cable `Waiting` screen may wait for the peer device to
/// connect over USB and report a payload before failing to the
/// retry/cancel screen (unix-seconds; the engine's clock domain). Driven
/// by the `poll_notifications` pump via `WorkflowEngine::tick`
/// (`2026-06-11-exchange-waits-forever-without-capabilities`, T1.3 —
/// ADR-021: core owns the timer). A connected cable handshakes in
/// seconds; this is the no-response backstop, not a tight bound.
pub const DIRECT_WAITING_TIMEOUT_SECS: u64 = 60;

/// Presentation state of the Cable engine.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DirectScreen {
    /// Waiting for the phone to connect over USB and report a payload.
    Waiting,
    /// Payloads swapped; key agreement + card exchange in flight.
    Exchanging,
    /// Exchange complete; awaiting reciprocity confirmation via relay escrow.
    Verifying,
    /// Terminal success.
    Success,
    /// Terminal failure.
    Failed { reason: Option<String> },
}

/// What the session state implies for the engine, snapshotted out of the
/// session borrow so the completion machinery can mutate `self` freely.
enum StateSync {
    Complete(Box<Contact>),
    Failed(String),
    Progressing,
    Other,
}

/// Dedicated Cable (USB / direct-TCP) exchange engine — owns its
/// [`ExchangeSession`] and the completion machinery.
pub struct DirectTransportEngine {
    /// `None` only when the factory could not provide an identity + own card;
    /// the engine then starts on the Failed screen. Always `Some` on a
    /// non-terminal screen.
    session: Option<ExchangeSession>,
    reciprocity_confirmer: Option<ReciprocityConfirmer>,
    success_summary: Option<super::success::ExchangeSuccessSummary>,
    clock: Arc<dyn Clock>,
    screen: DirectScreen,
    /// `true` once the initial `DirectSend` command has been emitted, so
    /// `screen_entered` is idempotent across re-renders.
    started: bool,
    cancelled: bool,
    /// Unix-seconds when the engine entered `Waiting` (construction). The
    /// `tick`-driven stall deadline ([`DIRECT_WAITING_TIMEOUT_SECS`]) is
    /// measured from it. `Waiting` is only entered at construction (retry
    /// re-provisions a fresh engine), so this is never re-stamped.
    waiting_entered_unix: u64,
}

impl DirectTransportEngine {
    /// Build a fresh Cable engine. `identity` + `card` are consumed by the
    /// `new_usb` session (the identity is not cloneable — retry re-provisions a
    /// fresh engine via the factory, mirroring NFC/Link). When either is
    /// `None` (a should-never-happen contract violation post-onboarding) the
    /// engine degrades to the Failed screen rather than panicking.
    pub fn new(
        identity: Option<Identity>,
        card: Option<ContactCard>,
        role: UsbRole,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let session = match (identity, card) {
            (Some(identity), Some(card)) => Some(ExchangeSession::new_usb(
                identity,
                card,
                ManualConfirmationVerifier::new(),
                role,
                clock.clone(),
            )),
            _ => None,
        };
        let screen = if session.is_some() {
            DirectScreen::Waiting
        } else {
            DirectScreen::Failed {
                reason: Some("Identity unavailable — cannot start a USB exchange".into()),
            }
        };
        // Only meaningful while `Waiting`; the degraded Failed-at-construction
        // path leaves it 0 (tick guards on `screen == Waiting` regardless).
        let waiting_entered_unix = if session.is_some() {
            clock.unix_seconds()
        } else {
            0
        };
        Self {
            session,
            reciprocity_confirmer: None,
            success_summary: None,
            clock,
            screen,
            started: false,
            cancelled: false,
            waiting_entered_unix,
        }
    }

    /// The card this engine will transmit to the peer (the session's frozen
    /// own card). `None` when the engine degraded to Failed. Test seam for the
    /// group-filter wiring (`2026-06-08-exchange-card-not-group-filtered`).
    #[cfg(test)]
    pub(crate) fn outgoing_card(&self) -> Option<&ContactCard> {
        self.session.as_ref().map(|s| s.our_card())
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.screen {
            DirectScreen::Waiting => self.build_waiting_screen(),
            DirectScreen::Exchanging => self.build_exchanging_screen(),
            DirectScreen::Verifying => super::verifying::build_verifying_screen(),
            DirectScreen::Success => self.build_success_screen(),
            DirectScreen::Failed { reason } => self.build_failed_screen(reason.clone()),
        }
    }

    fn build_waiting_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_direct_waiting".into(),
            title: "USB Exchange".into(),
            subtitle: Some("Connect your phone via USB cable".into()),
            components: vec![Component::Text {
                id: "instructions".into(),
                content: "1. Connect your phone with a USB cable\n2. Enable USB tethering (Android) or trust this computer (iOS)\n3. Open Vauchi on your phone and start an exchange".into(),
                style: TextStyle::Body,
            }],
            actions: vec![ScreenAction {
                id: ACTION_CANCEL.into(),
                label: "Cancel".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            ..Default::default()
        }
    }

    fn build_exchanging_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_direct_exchanging".into(),
            title: "USB Exchange".into(),
            subtitle: Some("Exchanging contact cards...".into()),
            components: vec![Component::Text {
                id: "status".into(),
                content: "Connected. Exchanging encrypted data...".into(),
                style: TextStyle::Body,
            }],
            actions: vec![],
            ..Default::default()
        }
    }

    fn build_success_screen(&self) -> ScreenModel {
        if let Some(summary) = self.success_summary.as_ref() {
            return super::success::build_exchange_success_screen(
                "exchange_success",
                "Success",
                ACTION_DONE,
                summary,
            );
        }
        ScreenModel {
            screen_id: "exchange_success".into(),
            title: "Success".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "success_status".into(),
                icon: None,
                title: "Exchange Complete".into(),
                detail: None,
                status: Status::Success,
                a11y: Some(A11y {
                    label: Some("Exchange complete".into()),
                    hint: Some("Contact cards have been exchanged successfully".into()),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: ACTION_DONE.into(),
                label: "Done".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            ..Default::default()
        }
    }

    fn build_failed_screen(&self, detail: Option<String>) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_failed".into(),
            title: "Failed".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: "Exchange Failed".into(),
                detail,
                status: Status::Failed,
                a11y: Some(A11y {
                    label: Some("Exchange failed".into()),
                    hint: Some("The exchange did not complete. Retry or cancel.".into()),
                    role: None,
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: ACTION_RETRY.into(),
                    label: "Retry".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: ACTION_CANCEL.into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            ..Default::default()
        }
    }

    /// Snapshot the session state into an owned [`StateSync`], dropping the
    /// session borrow so the completion machinery can mutate `self`.
    fn snapshot_state(&self) -> StateSync {
        match self.session.as_ref().map(|s| s.state()) {
            Some(ExchangeState::Complete { contact }) => StateSync::Complete(contact.clone()),
            Some(ExchangeState::Failed { error }) => {
                StateSync::Failed(error.user_message().to_string())
            }
            Some(ExchangeState::AwaitingKeyAgreement { .. })
            | Some(ExchangeState::AwaitingCardExchange { .. }) => StateSync::Progressing,
            _ => StateSync::Other,
        }
    }

    /// The shared Complete-state machinery: build the success summary, then
    /// route reciprocity escrow (stay on Verifying until the peer confirms,
    /// preventing asymmetric exchanges). Moved verbatim from the legacy
    /// `ExchangeEngine::handle_hardware_event` (`mod.rs:792-867`).
    fn sync_completed(
        &mut self,
        contact: &Contact,
        reciprocity_result: Option<Reciprocity>,
        commands: &mut Vec<Command>,
    ) {
        if self.success_summary.is_none() {
            self.success_summary = Some(super::build_legacy_success_summary(contact, None));
        }

        // Create the reciprocity confirmer from the session's tokens + escrow
        // keys. Don't transition to Success until reciprocity is confirmed.
        let our_token = self
            .session
            .as_ref()
            .and_then(|s| s.our_confirmation_token().copied());
        let their_token = self
            .session
            .as_ref()
            .and_then(|s| s.expected_their_token().copied());
        let escrow = self.session.as_ref().and_then(|s| {
            s.confirmation_escrow().map(|(gate, our_slot, their_slot)| {
                (
                    gate.to_string(),
                    our_slot.to_string(),
                    their_slot.to_string(),
                )
            })
        });

        if self.reciprocity_confirmer.is_none()
            && let (Some(our_token), Some(their_token)) = (our_token, their_token)
            && let Some((gate, our_slot, their_slot)) = escrow
        {
            let mut confirmer = ReciprocityConfirmer::new(
                our_token,
                their_token,
                gate,
                our_slot,
                their_slot,
                self.clock.unix_seconds(),
                true,
            );
            commands.extend(confirmer.start());
            self.reciprocity_confirmer = Some(confirmer);
            self.screen = DirectScreen::Verifying;
        } else if let Some(result) = reciprocity_result {
            match result {
                Reciprocity::Confirmed => self.screen = DirectScreen::Success,
                _ => {
                    self.screen = DirectScreen::Failed {
                        reason: Some("Exchange not confirmed by the other device".into()),
                    }
                }
            }
        } else if self.reciprocity_confirmer.is_some() {
            self.screen = DirectScreen::Verifying;
        } else {
            // No confirmation tokens (e.g. no relay configured) — complete.
            self.screen = DirectScreen::Success;
        }
    }
}

impl WorkflowEngine for DirectTransportEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    /// Fail a peerless cable `Waiting` once it exceeds
    /// [`DIRECT_WAITING_TIMEOUT_SECS`] (T1.3, ADR-021). Driven by the
    /// `poll_notifications` pump. Only `Waiting` has a wall-clock bound —
    /// `Exchanging`/`Verifying` are peer-progress states and the rest are
    /// terminal.
    fn tick(&mut self, now: u64) {
        if self.cancelled || self.screen != DirectScreen::Waiting {
            return;
        }
        if now.saturating_sub(self.waiting_entered_unix) >= DIRECT_WAITING_TIMEOUT_SECS {
            self.screen = DirectScreen::Failed {
                reason: Some("No response over USB — the other device didn't connect.".into()),
            };
        }
    }

    /// Emit the initial `DirectSend` command once, on first screen entry.
    fn screen_entered(&mut self) -> Vec<Command> {
        if self.started || self.cancelled || self.screen != DirectScreen::Waiting {
            return Vec::new();
        }
        self.started = true;
        if let Some(session) = self.session.as_mut() {
            session.emit_initial_commands();
            return session.drain_commands();
        }
        Vec::new()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match &self.screen {
            DirectScreen::Success => match action {
                UserAction::ActionPressed { action_id } if action_id == ACTION_DONE => {
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            DirectScreen::Failed { .. } => match action {
                UserAction::ActionPressed { action_id } if action_id == ACTION_RETRY => {
                    // The session + identity are consumed; a fresh engine is
                    // re-provisioned by the factory (mirror NFC/Link). Not a
                    // cancel — persistence is skipped only on cancel.
                    self.cancelled = false;
                    ActionResult::StartDirectTransport
                }
                _ => {
                    self.cancelled = true;
                    ActionResult::Complete
                }
            },
            DirectScreen::Waiting | DirectScreen::Exchanging | DirectScreen::Verifying => {
                match action {
                    UserAction::ActionPressed { action_id } if action_id == ACTION_CANCEL => {
                        self.cancelled = true;
                        ActionResult::Complete
                    }
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                }
            }
        }
    }

    fn handle_hardware_event(&mut self, event: Event) -> Option<ActionResult> {
        if matches!(
            self.screen,
            DirectScreen::Success | DirectScreen::Failed { .. }
        ) {
            return None;
        }

        // Clone the event for confirmer routing (the session consumes it).
        let event_for_confirmer = if self.reciprocity_confirmer.is_some() {
            Some(event.clone())
        } else {
            None
        };

        // Apply the event to the session. Each access is a short `as_mut`/
        // `as_ref` so the borrow drops before we mutate `self.screen`.
        match self.session.as_mut().map(|s| s.apply_hardware_event(event)) {
            None => return None, // no session (degenerate Failed-at-construction)
            Some(Err(e)) => {
                self.screen = DirectScreen::Failed {
                    reason: Some(e.user_message().to_string()),
                };
                return Some(ActionResult::UpdateScreen(self.build_screen()));
            }
            Some(Ok(())) => {}
        }
        let mut commands = self
            .session
            .as_mut()
            .map(|s| s.drain_commands())
            .unwrap_or_default();

        // Phase A: USB auto-advance. After DirectPayloadReceived →
        // AwaitingKeyAgreement, drive PerformKeyAgreement (emits DirectSendCard,
        // sets proximity High internally for USB). The peer's card arrives later
        // as DirectCardReceived (Phase B), which completes the session.
        let needs_key_agreement = self.session.as_ref().is_some_and(|s| {
            matches!(s.state(), ExchangeState::AwaitingKeyAgreement { .. })
                && s.transport() == ExchangeTransport::Usb
        });
        if needs_key_agreement {
            if let Some(Err(e)) = self
                .session
                .as_mut()
                .map(|s| s.apply(ExchangeEvent::PerformKeyAgreement))
            {
                self.screen = DirectScreen::Failed {
                    reason: Some(e.user_message().to_string()),
                };
                return Some(ActionResult::UpdateScreen(self.build_screen()));
            }
            if let Some(session) = self.session.as_mut() {
                commands.extend(session.drain_commands());
            }
        }

        // Route escrow events to the reciprocity confirmer if active.
        let mut reciprocity_result = None;
        if let Some(ref mut confirmer) = self.reciprocity_confirmer {
            if let Some(ref evt) = event_for_confirmer {
                commands.extend(confirmer.handle_event(evt));
            }
            if confirmer.is_done() {
                reciprocity_result = Some(confirmer.reciprocity());
                self.reciprocity_confirmer = None;
            }
        }

        // Sync the engine screen from the session state.
        match self.snapshot_state() {
            StateSync::Complete(contact) => {
                self.sync_completed(&contact, reciprocity_result, &mut commands)
            }
            StateSync::Failed(reason) => {
                self.screen = DirectScreen::Failed {
                    reason: Some(reason),
                }
            }
            StateSync::Progressing => self.screen = DirectScreen::Exchanging,
            StateSync::Other => {}
        }

        if commands.is_empty() {
            Some(ActionResult::UpdateScreen(self.build_screen()))
        } else {
            Some(ActionResult::Commands { commands })
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

// INLINE_TEST_REQUIRED: the engine owns private session/screen state; tests
// drive it through the public WorkflowEngine surface with real crypto (two
// `new_usb` engines exchanging payloads + encrypted cards) — never mocked
// (ADR-002).
#[cfg(test)]
mod tests {
    use super::*;
    use vauchi_core::clock::SystemClock;

    fn identity(name: &str) -> Identity {
        Identity::create(name, 0)
    }

    fn engine(name: &str) -> DirectTransportEngine {
        let id = identity(name);
        let c = ContactCard::new(id.display_name());
        DirectTransportEngine::new(Some(id), Some(c), UsbRole::Initiator, SystemClock::shared())
    }

    /// Pull the `DirectSend` payload out of an engine's `screen_entered` output.
    fn direct_send_payload(cmds: &[Command]) -> Vec<u8> {
        cmds.iter()
            .find_map(|c| match c {
                Command::DirectSend { payload, .. } => Some(payload.clone()),
                _ => None,
            })
            .expect("a DirectSend command")
    }

    /// Pull the `DirectSendCard` ciphertext out of an `ActionResult::Commands`.
    fn direct_send_card(result: &ActionResult) -> Vec<u8> {
        let ActionResult::Commands { commands } = result else {
            panic!("expected Commands, got {result:?}");
        };
        commands
            .iter()
            .find_map(|c| match c {
                Command::DirectSendCard { ciphertext, .. } => Some(ciphertext.clone()),
                _ => None,
            })
            .expect("a DirectSendCard command")
    }

    // @internal
    #[test]
    fn new_engine_renders_waiting_and_not_cancelled() {
        let e = engine("Alice");
        assert_eq!(e.current_screen().screen_id, "exchange_direct_waiting");
        assert!(!e.was_cancelled());
    }

    // @internal
    #[test]
    fn no_identity_degrades_to_failed_screen() {
        let e = DirectTransportEngine::new(None, None, UsbRole::Initiator, SystemClock::shared());
        assert_eq!(e.current_screen().screen_id, "exchange_failed");
        assert!(!e.was_cancelled());
    }

    // @internal
    #[test]
    fn waiting_past_timeout_ticks_to_failed() {
        let mut e = engine("Alice");
        // Read the clock just after construction: `entered` is >= the
        // engine's stamped waiting-entry second, so `+ budget + 1` is
        // unambiguously past the deadline (CC-06 — explicit `now`, no wait).
        let entered = SystemClock::shared().unix_seconds();
        assert_eq!(e.current_screen().screen_id, "exchange_direct_waiting");

        e.tick(entered + DIRECT_WAITING_TIMEOUT_SECS + 1);

        assert_eq!(
            e.current_screen().screen_id,
            "exchange_failed",
            "a peerless cable Waiting past its budget must fail to retry/cancel"
        );
    }

    // @internal
    #[test]
    fn waiting_within_timeout_stays_waiting() {
        let mut e = engine("Alice");
        let entered = SystemClock::shared().unix_seconds();

        // `entered` is at most one second past the stamped waiting-entry,
        // so ticking at `entered` is well within the budget.
        e.tick(entered);

        assert_eq!(
            e.current_screen().screen_id,
            "exchange_direct_waiting",
            "must not fail before the Waiting budget elapses"
        );
    }

    // @internal
    #[test]
    fn tick_on_degraded_failed_engine_is_inert() {
        // No identity → the engine constructs straight into Failed. A tick
        // far past any budget must not re-fail it or change the reason
        // (the `screen != Waiting` guard, CC-14 adversarial case).
        let mut e =
            DirectTransportEngine::new(None, None, UsbRole::Initiator, SystemClock::shared());
        assert_eq!(e.current_screen().screen_id, "exchange_failed");
        let before = e.current_screen().components;

        e.tick(u64::MAX);

        assert_eq!(e.current_screen().screen_id, "exchange_failed");
        assert_eq!(
            e.current_screen().components,
            before,
            "tick must not mutate a degraded-Failed engine"
        );
    }

    // @internal
    #[test]
    fn screen_entered_emits_direct_send_once() {
        let mut e = engine("Alice");
        let cmds = e.screen_entered();
        assert!(
            cmds.iter().any(|c| matches!(c, Command::DirectSend { .. })),
            "first entry emits DirectSend"
        );
        assert!(
            e.screen_entered().is_empty(),
            "idempotent — no re-emit on re-render"
        );
    }

    // @internal
    #[test]
    fn cancel_completes_and_marks_cancelled() {
        let mut e = engine("Alice");
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: ACTION_CANCEL.into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        assert!(e.was_cancelled());
    }

    // @internal
    #[test]
    fn invalid_payload_transitions_to_failed() {
        let mut e = engine("Alice");
        e.screen_entered();
        let result = e
            .handle_hardware_event(Event::DirectPayloadReceived {
                data: b"not-a-valid-qr".to_vec(),
            })
            .expect("active engine handles the event");
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
        assert_eq!(e.current_screen().screen_id, "exchange_failed");
    }

    // @internal
    #[test]
    fn retry_from_failed_emits_start_direct_transport() {
        let mut e = engine("Alice");
        e.screen_entered();
        let _ = e.handle_hardware_event(Event::DirectPayloadReceived {
            data: b"garbage".to_vec(),
        });
        assert_eq!(e.current_screen().screen_id, "exchange_failed");
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: ACTION_RETRY.into(),
        });
        assert!(matches!(result, ActionResult::StartDirectTransport));
        assert!(!e.was_cancelled());
    }

    /// Full real-crypto round-trip between two engines: payload leg (Phase A,
    /// emits DirectSendCard) then card leg (Phase B, completes the session).
    /// Each engine ends with the exchange complete and a success summary naming
    /// the *peer* — proving the USB ceremony drives a genuine exchange (C-1).
    // @internal
    #[test]
    fn round_trip_completes_with_peer_card_and_summary() {
        let mut alice = engine("Alice");
        let mut bob = engine("Bob");

        let a_payload = direct_send_payload(&alice.screen_entered());
        let b_payload = direct_send_payload(&bob.screen_entered());

        // Phase A: swap payloads → each auto-advances key agreement + emits its
        // encrypted card.
        let a_card = direct_send_card(
            &alice
                .handle_hardware_event(Event::DirectPayloadReceived { data: b_payload })
                .expect("alice phase A"),
        );
        let b_card = direct_send_card(
            &bob.handle_hardware_event(Event::DirectPayloadReceived { data: a_payload })
                .expect("bob phase A"),
        );
        assert_eq!(
            alice.current_screen().screen_id,
            "exchange_direct_exchanging"
        );

        // Phase B: deliver the peer's encrypted card → session completes.
        let _ = alice
            .handle_hardware_event(Event::DirectCardReceived { ciphertext: b_card })
            .expect("alice phase B");
        let _ = bob
            .handle_hardware_event(Event::DirectCardReceived { ciphertext: a_card })
            .expect("bob phase B");

        // After completion the engine either confirms reciprocity (Verifying)
        // or, with no relay escrow, lands on Success — never still Exchanging.
        let alice_screen = alice.current_screen().screen_id;
        assert!(
            alice_screen == "exchange_verifying" || alice_screen == "exchange_success",
            "alice completed the exchange, got {alice_screen}"
        );
        // The success summary names the peer (Bob), proving Alice consumed
        // *Bob's* card — not a synthetic self-completion.
        assert_eq!(
            alice
                .success_summary
                .as_ref()
                .expect("summary built")
                .peer_name,
            "Bob"
        );
        assert_eq!(
            bob.success_summary
                .as_ref()
                .expect("summary built")
                .peer_name,
            "Alice"
        );
    }
}
