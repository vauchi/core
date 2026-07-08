// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dedicated core-driven Humble engine for the NFC exchange mode
//! (`ExchangeMode::TapTap`).
//!
//! NFC graduation (exchange-engine graduation program): a `WorkflowEngine`
//! that **wraps** the existing, well-tested [`super::nfc::NfcExchangeFlow`]
//! 3-phase handshake state machine rather than rewriting it. The engine owns
//! the flow, renders the role-chooser + sub-flow screens, maps the flow's
//! `NfcHardwareOutcome`s to `ActionResult`s (lifted from the legacy
//! `ExchangeEngine::apply_nfc_outcome`, including the relay-escrow handoff on
//! a dropped tap), and performs the lazy HCE-responder bootstrap on the peer's
//! first tap.
//!
//! Mirrors [`super::ble_engine::BleExchangeEngine`] /
//! `LinkExchangeEngine` / `MultiStageExchangeEngine`. Two NFC-specific
//! wrinkles vs BLE:
//! - **Role selection.** The engine opens on a Send/Receive chooser
//!   (`exchange_nfc_role`); Send starts an initiator flow up-front, Receive
//!   defers flow creation to the lazy bootstrap.
//! - **Retry re-creates the engine.** The signing `Identity` is consumed
//!   (un-cloneable) when a flow is built, so the failed screen's Retry emits
//!   `ActionResult::StartNfcExchange` (a fresh engine re-provisions it),
//!   mirroring `LinkExchangeEngine` rather than BLE's in-place reset.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use std::sync::Arc;
use vauchi_core::clock::Clock;
use vauchi_core::identity::Identity;
use vauchi_core::{Command, Event};

use super::nfc::{NfcExchangeFlow, NfcHardwareOutcome, NfcStep, build_nfc_screen};

/// Action id for the Cancel button (any screen).
pub const ACTION_CANCEL: &str = "cancel";
/// Action id for the Retry button on the failed screen.
pub const ACTION_RETRY: &str = "retry";
/// Action id for the Done button on the success screen.
pub const ACTION_DONE: &str = "done";
/// Role-chooser item id: act as the initiator ("Send").
pub const ROLE_SEND: &str = "nfc_role:send";
/// Role-chooser item id: act as the responder ("Receive").
pub const ROLE_RECEIVE: &str = "nfc_role:receive";

/// Relay-escrow TTL when an NFC tap drops after the shared key is established.
/// Mirrors Link-mode's 7-day default (`link_mode.rs` `DEFAULT_TTL_SECONDS`).
const NFC_RELAY_TTL_SECONDS: u32 = 604_800;

/// How long a non-terminal NFC step (`AwaitingTap`/`PayloadSent`/`AckSent`)
/// may persist with no inbound progress before the engine fails to the
/// retry/cancel screen. Measured from `Active` entry (role pick) and
/// re-stamped on every step advance / consumed in-step event. This is the
/// human tap-window backstop — NOT the OS ~125 ms HCE APDU budget, which the
/// platform enforces. RoleSelection (a user choice) never times out
/// (`2026-06-11-exchange-waits-forever-without-capabilities`, T1.3; ADR-021:
/// core owns the timer). Unix-seconds.
pub const NFC_STEP_TIMEOUT_SECS: u64 = 60;

/// Presentation state of the NFC engine. The active sub-flow screen is derived
/// from the wrapped flow's `NfcStep`; `Success`/`Failed` are terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NfcScreen {
    /// Send/Receive chooser, rendered before any flow exists.
    RoleSelection,
    /// A role was picked; render the sub-flow (or the AwaitingTap holding
    /// screen for the responder before its lazy bootstrap fires).
    Active,
    Success,
    Failed {
        reason: Option<String>,
    },
}

/// Dedicated NFC exchange engine — wraps [`NfcExchangeFlow`].
pub struct NfcExchangeEngine {
    /// The signing identity, consumed when a flow is built (initiator on Send,
    /// responder on the lazy bootstrap). `None` after consumption or if the
    /// host could not reconstruct it.
    identity: Option<Identity>,
    display_name: String,
    /// QR fallback is offered on failure only when this device has a camera.
    has_camera: bool,
    /// The wrapped handshake flow. `None` until a role builds it (Send builds
    /// it up-front; Receive defers to the first-tap lazy bootstrap).
    flow: Option<NfcExchangeFlow>,
    screen: NfcScreen,
    cancelled: bool,
    clock: Arc<dyn Clock>,
    /// Unix-seconds when the current step was entered — stamped on `Active`
    /// entry (role pick) and re-stamped on step progress. The `tick` stall
    /// deadline ([`NFC_STEP_TIMEOUT_SECS`]) is measured from it.
    step_entered_unix: u64,
    locale: Locale,
}

impl NfcExchangeEngine {
    /// Build a fresh NFC engine. `identity` is the reconstructed signing
    /// identity (None if the host could not provide one — the role pick then
    /// fails gracefully); `has_camera` gates the QR fallback on the failed
    /// screen.
    pub fn new(
        identity: Option<Identity>,
        display_name: String,
        has_camera: bool,
        clock: Arc<dyn Clock>,
        locale: Locale,
    ) -> Self {
        let step_entered_unix = clock.unix_seconds();
        Self {
            identity,
            display_name,
            has_camera,
            flow: None,
            screen: NfcScreen::RoleSelection,
            cancelled: false,
            clock,
            step_entered_unix,
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// The sub-flow step currently driving the Active screen — the flow's step
    /// if it exists, else `AwaitingTap` (the responder holding screen before
    /// its lazy bootstrap).
    fn active_step(&self) -> NfcStep {
        self.flow
            .as_ref()
            .map(|f| f.step().clone())
            .unwrap_or(NfcStep::AwaitingTap)
    }

    /// Send: build the initiator flow now and emit its key-offer activation.
    fn start_send(&mut self) -> ActionResult {
        let identity = match self.identity.take() {
            Some(id) => id,
            None => return self.fail("no active identity for NFC exchange".into()),
        };
        let mut flow = NfcExchangeFlow::new_initiator(identity, self.display_name.clone());
        match flow.activate() {
            Ok(commands) => {
                self.flow = Some(flow);
                self.screen = NfcScreen::Active;
                self.step_entered_unix = self.clock.unix_seconds();
                ActionResult::Commands { commands }
            }
            Err(e) => self.fail(format!("NFC activation failed: {e:?}")),
        }
    }

    /// Receive: defer flow creation to the lazy bootstrap, but emit an empty
    /// `NfcActivate` now so the frontend registers its HCE context before the
    /// peer's first tap (empty payload = responder; non-empty = initiator).
    fn start_receive(&mut self) -> ActionResult {
        if self.identity.is_none() {
            return self.fail("no active identity for NFC exchange".into());
        }
        self.screen = NfcScreen::Active;
        self.step_entered_unix = self.clock.unix_seconds();
        ActionResult::Commands {
            commands: vec![Command::NfcActivate {
                payload: Vec::new(),
            }],
        }
    }

    fn fail(&mut self, reason: String) -> ActionResult {
        self.screen = NfcScreen::Failed {
            reason: Some(reason),
        };
        ActionResult::UpdateScreen(self.build_screen())
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.screen {
            NfcScreen::RoleSelection => build_nfc_role_screen(self.locale),
            NfcScreen::Active => build_nfc_screen(&self.active_step(), self.locale),
            NfcScreen::Success => self.build_success_screen(),
            NfcScreen::Failed { reason } => self.build_failed_screen(reason.clone()),
        }
    }

    fn build_success_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_success".into(),
            title: self.t("exchange.terminal.success"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "success_status".into(),
                icon: None,
                title: self.t("exchange.terminal.complete"),
                detail: None,
                status: Status::Success,
                a11y: Some(A11y {
                    label: Some(self.t("exchange.terminal.complete")),
                    hint: Some(self.t("exchange.terminal.complete_hint")),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: ACTION_DONE.into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            ..Default::default()
        }
    }

    fn build_failed_screen(&self, detail: Option<String>) -> ScreenModel {
        let mut actions = vec![ScreenAction {
            id: ACTION_RETRY.into(),
            label: self.t("action.retry"),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: None,
        }];
        if self.has_camera {
            actions.push(ScreenAction {
                id: "fallback_qr".into(),
                label: self.t("exchange.terminal.switch_qr"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y {
                    label: None,
                    hint: Some(self.t("exchange.terminal.switch_qr_hint")),
                    role: None,
                }),
            });
        }
        actions.push(ScreenAction {
            id: "fallback_relay".into(),
            label: self.t("exchange.terminal.switch_relay"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y {
                label: None,
                hint: Some(self.t("exchange.terminal.switch_relay_hint")),
                role: None,
            }),
        });
        actions.push(ScreenAction {
            id: ACTION_CANCEL.into(),
            label: self.t("action.cancel"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });
        ScreenModel {
            screen_id: "exchange_failed".into(),
            title: self.t("exchange.terminal.failed"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: self.t("exchange.terminal.failed_status"),
                detail,
                status: Status::Failed,
                a11y: Some(A11y {
                    label: Some(self.t("exchange.terminal.failed_status")),
                    hint: Some(self.t("exchange.terminal.failed_hint")),
                    role: None,
                }),
            }],
            actions,
            ..Default::default()
        }
    }

    /// Translate an `NfcHardwareOutcome` to an `ActionResult` + state change.
    /// Lifted from the legacy `ExchangeEngine::apply_nfc_outcome`.
    fn apply_outcome(&mut self, outcome: NfcHardwareOutcome) -> ActionResult {
        match outcome {
            NfcHardwareOutcome::StepAdvanced { commands }
            | NfcHardwareOutcome::Consumed { commands } => {
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            NfcHardwareOutcome::Complete {
                card_bytes: _,
                commands,
            } => {
                // Card persistence is core-owned via the completion path; the
                // engine only flips to the terminal success screen.
                self.screen = NfcScreen::Success;
                if commands.is_empty() {
                    ActionResult::UpdateScreen(self.build_screen())
                } else {
                    ActionResult::Commands { commands }
                }
            }
            NfcHardwareOutcome::FailedWithFallback {
                reason,
                relay_handoff,
            } => {
                self.screen = NfcScreen::Failed {
                    reason: Some(reason),
                };
                // A tap that drops after the shared key is established can still
                // complete over the relay: deposit the encrypted card into
                // escrow rather than just showing the failed screen.
                if let Some(handoff) = relay_handoff {
                    ActionResult::Commands {
                        commands: vec![Command::RelayEscrowDeposit {
                            gate_hash: handoff.gate_hash,
                            slot_hash: handoff.slot_hash,
                            encrypted_card: handoff.encrypted_card,
                            ttl_seconds: NFC_RELAY_TTL_SECONDS,
                        }],
                    }
                } else {
                    ActionResult::UpdateScreen(self.build_screen())
                }
            }
            NfcHardwareOutcome::Ignored => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

impl WorkflowEngine for NfcExchangeEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match &self.screen {
            NfcScreen::RoleSelection => match action {
                UserAction::ListItemSelected { item_id, .. } if item_id == ROLE_SEND => {
                    self.start_send()
                }
                UserAction::ListItemSelected { item_id, .. } if item_id == ROLE_RECEIVE => {
                    self.start_receive()
                }
                UserAction::ActionPressed { action_id } if action_id == ACTION_CANCEL => {
                    self.cancelled = true;
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            NfcScreen::Active => match action {
                UserAction::ActionPressed { action_id } if action_id == ACTION_CANCEL => {
                    self.cancelled = true;
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            NfcScreen::Success => match action {
                UserAction::ActionPressed { action_id } if action_id == ACTION_DONE => {
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            NfcScreen::Failed { .. } => match action {
                UserAction::ActionPressed { action_id } if action_id == ACTION_RETRY => {
                    // The consumed `Identity` cannot be re-cloned, so retry asks
                    // the AppEngine for a fresh engine (mirrors Link).
                    ActionResult::StartNfcExchange
                }
                // `cancel` and the `fallback_*` transport switches all end this
                // attempt; the relay/QR switch is a router concern, treated as
                // cancel here so the buttons never dead-end silently.
                _ => {
                    self.cancelled = true;
                    ActionResult::Complete
                }
            },
        }
    }

    fn handle_hardware_event(&mut self, event: Event) -> Option<ActionResult> {
        if !matches!(self.screen, NfcScreen::Active) {
            return None;
        }

        // Lazy HCE-responder bootstrap: the responder has no flow until the
        // peer's first tap lands as `NfcDataReceived`. Spin it up, then let the
        // same event fall through to the flow below
        // (`2026-05-29-nfc-exchange-mode-entry-wiring`).
        if self.flow.is_none()
            && matches!(event, Event::NfcDataReceived { .. })
            && let Some(identity) = self.identity.take()
        {
            let mut flow = NfcExchangeFlow::new_responder(identity, self.display_name.clone());
            // activate() emits an empty NfcActivate (already listening via HCE);
            // discard it — the tap already happened and is processed below.
            if flow.activate().is_ok() {
                self.flow = Some(flow);
                // First peer contact restarts the stall budget, so the
                // handshake gets a full window regardless of how long the
                // responder waited on the empty Active screen for the
                // initiator to approach (T1.3-NFC; adversarial-review W1).
                self.step_entered_unix = self.clock.unix_seconds();
            }
        }

        let flow = self.flow.as_mut()?;
        let outcome = flow.handle_event(&event);
        // Forward progress refreshes the stall deadline (T1.2): a step advance
        // or a consumed in-step event (e.g. a mid-handshake APDU). Only a
        // genuinely silent step trips it; `Ignored`/terminal do not re-stamp.
        if matches!(
            outcome,
            NfcHardwareOutcome::StepAdvanced { .. } | NfcHardwareOutcome::Consumed { .. }
        ) {
            self.step_entered_unix = self.clock.unix_seconds();
        }
        Some(self.apply_outcome(outcome))
    }

    /// Fail a stalled non-terminal NFC step past [`NFC_STEP_TIMEOUT_SECS`]
    /// (T1.3, ADR-021). Driven by the `poll_notifications` pump. Only the
    /// `Active` step states wait on hardware; `RoleSelection` is a user choice
    /// and `Success`/`Failed` are terminal — none of those time out.
    fn tick(&mut self, now: u64) {
        if self.cancelled || !matches!(self.screen, NfcScreen::Active) {
            return;
        }
        if now.saturating_sub(self.step_entered_unix) >= NFC_STEP_TIMEOUT_SECS {
            self.screen = NfcScreen::Failed {
                reason: Some("NFC tap timed out — no response from the other device.".into()),
            };
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Core-driven NFC role chooser (Send/Receive), the engine's opening screen.
/// Per ADR-043/044 the renderer is humble: a generic `ActionList` whose
/// item/title strings are i18n keys the frontend resolves (ADR-038). The item
/// ids route in `handle_action` to the initiator / responder entry.
fn build_nfc_role_screen(locale: Locale) -> ScreenModel {
    let t = |key: &str| get_string(locale, key);
    ScreenModel {
        screen_id: "exchange_nfc_role".into(),
        title: t("exchange.nfc.choose_role"),
        subtitle: Some(t("exchange.nfc.choose_role_subtitle")),
        components: vec![Component::ActionList {
            id: "nfc_role".into(),
            items: vec![
                ActionListItem {
                    id: ROLE_SEND.into(),
                    label: t("exchange.mode.nfc_send"),
                    icon: None,
                    detail: Some(t("exchange.mode.nfc_send_description")),
                    a11y: None,
                    info_key: None,
                },
                ActionListItem {
                    id: ROLE_RECEIVE.into(),
                    label: t("exchange.mode.nfc_receive"),
                    icon: None,
                    detail: Some(t("exchange.mode.nfc_receive_description")),
                    a11y: None,
                    info_key: None,
                },
            ],
        }],
        actions: vec![ScreenAction {
            id: ACTION_CANCEL.into(),
            label: t("action.cancel"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        }],
        ..Default::default()
    }
}

// INLINE_TEST_REQUIRED: the engine wraps private flow state; tests drive it via
// the public WorkflowEngine surface + the screen/action ids.
#[cfg(test)]
mod tests {
    use super::*;
    use vauchi_core::clock::SystemClock;

    fn identity() -> Identity {
        Identity::create("Alice", SystemClock::shared().unix_seconds())
    }

    fn engine() -> NfcExchangeEngine {
        NfcExchangeEngine::new(
            Some(identity()),
            "Alice".into(),
            true,
            SystemClock::shared(),
            Locale::English,
        )
    }

    fn select(item: &str) -> UserAction {
        UserAction::ListItemSelected {
            component_id: "nfc_role".into(),
            item_id: item.into(),
        }
    }

    fn press(id: &str) -> UserAction {
        UserAction::ActionPressed {
            action_id: id.into(),
        }
    }

    // @internal
    #[test]
    fn active_step_past_timeout_ticks_to_failed() {
        let mut e = engine();
        let _ = e.handle_action(select(ROLE_SEND));
        // `entered` read just after entering Active is >= the stamped
        // step-entry second (CC-06 — explicit now, no FakeClock, no sleep).
        let entered = SystemClock::shared().unix_seconds();
        assert_ne!(e.current_screen().screen_id, "exchange_failed");

        e.tick(entered + NFC_STEP_TIMEOUT_SECS + 1);

        assert_eq!(
            e.current_screen().screen_id,
            "exchange_failed",
            "a stalled NFC step past its budget must fail to retry/cancel"
        );
    }

    // @internal
    #[test]
    fn role_selection_does_not_time_out() {
        // RoleSelection is a user choice — it must never time out, even far
        // past the step budget.
        let mut e = engine();
        assert_eq!(e.current_screen().screen_id, "exchange_nfc_role");

        e.tick(u64::MAX);

        assert_eq!(
            e.current_screen().screen_id,
            "exchange_nfc_role",
            "the role chooser must not be timed out"
        );
    }

    // @internal
    #[test]
    fn tick_on_terminal_screen_is_inert() {
        // No identity → role pick fails straight to the terminal Failed
        // screen; a tick far past any budget must not mutate it (CC-14).
        let mut e = NfcExchangeEngine::new(
            None,
            "A".into(),
            true,
            SystemClock::shared(),
            Locale::English,
        );
        let _ = e.handle_action(select(ROLE_SEND));
        assert_eq!(e.current_screen().screen_id, "exchange_failed");
        let before = e.current_screen();

        e.tick(u64::MAX);

        assert_eq!(e.current_screen().screen_id, before.screen_id);
        assert_eq!(
            e.current_screen().components,
            before.components,
            "tick must not mutate a terminal NFC screen"
        );
    }

    // @internal
    #[test]
    fn new_engine_renders_role_chooser() {
        let e = engine();
        assert_eq!(e.current_screen().screen_id, "exchange_nfc_role");
        assert!(!e.was_cancelled());
    }

    // @internal
    #[test]
    fn send_starts_initiator_and_emits_nfc_activate_with_payload() {
        let mut e = engine();
        let result = e.handle_action(select(ROLE_SEND));
        match result {
            ActionResult::Commands { commands } => match &commands[0] {
                Command::NfcActivate { payload } => {
                    assert!(!payload.is_empty(), "initiator sends a non-empty key offer")
                }
                other => panic!("expected NfcActivate, got {other:?}"),
            },
            other => panic!("expected Commands, got {other:?}"),
        }
        assert_eq!(
            e.current_screen().screen_id,
            "exchange_nfc_awaiting_tap",
            "after Send the engine awaits the tap"
        );
    }

    // @internal
    #[test]
    fn receive_emits_empty_nfc_activate_and_defers_flow() {
        let mut e = engine();
        let result = e.handle_action(select(ROLE_RECEIVE));
        match result {
            ActionResult::Commands { commands } => match &commands[0] {
                Command::NfcActivate { payload } => {
                    assert!(
                        payload.is_empty(),
                        "responder registers HCE with empty payload"
                    )
                }
                other => panic!("expected NfcActivate, got {other:?}"),
            },
            other => panic!("expected Commands, got {other:?}"),
        }
        assert_eq!(e.current_screen().screen_id, "exchange_nfc_awaiting_tap");
        assert!(
            e.flow.is_none(),
            "responder flow is built lazily on first tap"
        );
    }

    // @internal
    #[test]
    fn send_without_identity_fails_gracefully() {
        let mut e = NfcExchangeEngine::new(
            None,
            "Alice".into(),
            true,
            SystemClock::shared(),
            Locale::English,
        );
        let _ = e.handle_action(select(ROLE_SEND));
        assert_eq!(e.current_screen().screen_id, "exchange_failed");
    }

    // @internal
    #[test]
    fn cancel_on_role_chooser_completes_and_marks_cancelled() {
        let mut e = engine();
        let result = e.handle_action(press(ACTION_CANCEL));
        assert!(matches!(result, ActionResult::Complete));
        assert!(e.was_cancelled());
    }

    // @internal
    #[test]
    fn cancel_during_active_flow_completes() {
        let mut e = engine();
        let _ = e.handle_action(select(ROLE_SEND));
        let result = e.handle_action(press(ACTION_CANCEL));
        assert!(matches!(result, ActionResult::Complete));
        assert!(e.was_cancelled());
    }

    // @internal
    #[test]
    fn retry_from_failed_requests_a_fresh_engine() {
        let mut e = NfcExchangeEngine::new(
            None,
            "Alice".into(),
            true,
            SystemClock::shared(),
            Locale::English,
        );
        let _ = e.handle_action(select(ROLE_SEND)); // -> Failed (no identity)
        assert_eq!(e.current_screen().screen_id, "exchange_failed");
        let result = e.handle_action(press(ACTION_RETRY));
        assert!(
            matches!(result, ActionResult::StartNfcExchange),
            "retry re-creates the engine to re-provision the identity, got {result:?}"
        );
    }

    // @internal
    #[test]
    fn failed_screen_offers_qr_fallback_only_with_camera() {
        let mut with = NfcExchangeEngine::new(
            None,
            "A".into(),
            true,
            SystemClock::shared(),
            Locale::English,
        );
        let _ = with.handle_action(select(ROLE_SEND));
        let with_ids: Vec<String> = with
            .current_screen()
            .actions
            .iter()
            .map(|a| a.id.clone())
            .collect();
        assert!(with_ids.iter().any(|i| i == "fallback_qr"));

        let mut without = NfcExchangeEngine::new(
            None,
            "A".into(),
            false,
            SystemClock::shared(),
            Locale::English,
        );
        let _ = without.handle_action(select(ROLE_SEND));
        let without_ids: Vec<String> = without
            .current_screen()
            .actions
            .iter()
            .map(|a| a.id.clone())
            .collect();
        assert!(!without_ids.iter().any(|i| i == "fallback_qr"));
        assert!(without_ids.iter().any(|i| i == "retry"));
    }

    // @internal
    #[test]
    fn receive_then_first_tap_lazily_bootstraps_the_responder_flow() {
        let mut e = engine();
        let _ = e.handle_action(select(ROLE_RECEIVE));
        assert!(
            e.flow.is_none(),
            "responder flow is not built until the first tap"
        );

        // The peer's first tap arrives. Even with a payload the handshake will
        // reject, the bootstrap must build the flow and consume the event
        // (returning Some), rather than dropping it on the floor (None).
        let result = e.handle_hardware_event(Event::NfcDataReceived { data: vec![0u8; 8] });
        assert!(
            result.is_some(),
            "the lazy-bootstrapped responder flow must handle the first tap"
        );
        assert!(
            e.flow.is_some(),
            "the responder flow exists after the first tap"
        );
    }

    // @internal
    #[test]
    fn hardware_event_ignored_off_active_screen() {
        let mut e = engine();
        // On the role chooser, a stray NFC event is a no-op.
        let result = e.handle_hardware_event(Event::NfcDataReceived {
            data: vec![1, 2, 3],
        });
        assert!(result.is_none());
    }
}
