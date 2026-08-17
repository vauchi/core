// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine for the link-mode initiator flow (slice 32l Phase 2).
//!
//! Renders the initiator screens for the engine-owned
//! `LinkInitiatorSession` state machine (in `vauchi-core`): a share-url
//! screen, a waiting screen while the responder opens the link, a
//! retrieving screen once ECDH has completed, then a terminal
//! success / failed screen. The engine is a humble object — it forwards
//! `cancel` via `was_cancelled`, emits `Command::ShowShareSheet` when the
//! user shares, and exposes `transition_to_*` setters for the AppEngine
//! link-initiator lifecycle to drive on relay-event transitions. It never
//! owns the `LinkInitiatorSession` and never blocks on or polls the relay.
//!
//! Mirrors `LinkResponderEngine`'s terminal-transition pattern
//! (`transition_to_completed` / `transition_to_failed`), extended with the
//! extra initiator screens (share-url, waiting, retrieving). The share-url
//! and waiting/retrieving builders are lifted from the retired
//! `ui/exchange/link.rs` sub-flow.
//!
//! See `_private/docs/problems/2026-05-11-link-exchange-engine-graduation/`.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use vauchi_core::Command;
use vauchi_core::Event;

/// Action id for the Share button on the share-url screen.
pub const ACTION_SHARE: &str = "share";
/// Action id for the Cancel button (share-url / waiting / failed screens).
pub const ACTION_CANCEL: &str = "cancel";
/// Action id for the Done button on the success screen.
pub const ACTION_DONE: &str = "done";
/// Action id for the Retry button on the failed screen.
pub const ACTION_RETRY: &str = "retry";

/// Presentation state of the initiator flow. Drives `current_screen`.
#[derive(Clone, Debug)]
enum InitiatorScreen {
    /// URL ready; share sheet pending.
    ShareUrl,
    /// Link shared; waiting for the responder to open it and for the
    /// handshake gate to cross.
    WaitingForResponse,
    /// ECDH complete, our card deposited; retrieving + decrypting the
    /// responder's card.
    Retrieving,
    /// Responder's card retrieved + persisted by core.
    Success,
    /// Terminal failure. `reason` is the stable `LinkInitiator` failure
    /// id (`polling_timed_out` / `handshake_failed` / `deposit_rejected` /
    /// `decrypt_error` / `cancelled`).
    Failed { reason: String },
}

impl InitiatorScreen {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Success | Self::Failed { .. })
    }
}

/// Engine for the link-mode initiator flow.
#[derive(Clone, Debug)]
pub struct LinkExchangeEngine {
    state: InitiatorScreen,
    /// The URL to share. Empty until the AppEngine link-initiator
    /// lifecycle builds the `LinkInitiatorSession` and calls
    /// `set_share_url`.
    share_url: String,
    cancelled: bool,
    /// Rich success-screen content (received card + visibility + groups),
    /// attached by the link-initiator lifecycle once the peer's card is
    /// persisted. When `None` the success screen falls back to minimal
    /// completion chrome.
    success_summary: Option<crate::ui::exchange::success::ExchangeSuccessSummary>,
    locale: Locale,
}

impl Default for LinkExchangeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LinkExchangeEngine {
    /// Build a fresh engine in the share-url state with no URL yet. The
    /// AppEngine link-initiator lifecycle sets the URL via
    /// `set_share_url` once the engine-owned `LinkInitiatorSession` has
    /// generated it.
    pub fn new() -> Self {
        Self {
            state: InitiatorScreen::ShareUrl,
            share_url: String::new(),
            cancelled: false,
            success_summary: None,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-11).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Attach the rich success summary the terminal screen renders. Inert
    /// once cancelled or terminal so a late setter can't resurrect torn-down
    /// state; call before `transition_to_success`.
    pub fn set_success_summary(
        &mut self,
        summary: crate::ui::exchange::success::ExchangeSuccessSummary,
    ) {
        if self.cancelled || self.state.is_terminal() {
            return;
        }
        self.success_summary = Some(summary);
    }

    /// Set the URL the share-url screen renders. Inert once cancelled or
    /// once a terminal screen has been rendered (the lifecycle may race a
    /// late setter against teardown — it must not resurrect the flow).
    pub fn set_share_url(&mut self, url: String) {
        if self.cancelled || self.state.is_terminal() {
            return;
        }
        self.share_url = url;
    }

    /// Transition to the waiting screen. Inert once cancelled or terminal.
    pub fn transition_to_waiting(&mut self) {
        if self.cancelled || self.state.is_terminal() {
            return;
        }
        self.state = InitiatorScreen::WaitingForResponse;
    }

    /// Transition to the retrieving screen. Inert once cancelled or
    /// terminal.
    pub fn transition_to_retrieving(&mut self) {
        if self.cancelled || self.state.is_terminal() {
            return;
        }
        self.state = InitiatorScreen::Retrieving;
    }

    /// Terminal success — the responder's card was retrieved and
    /// persisted by core. Renders `exchange_link_success`. Inert once
    /// cancelled; first terminal transition wins.
    pub fn transition_to_success(&mut self) {
        if self.cancelled || self.state.is_terminal() {
            return;
        }
        self.state = InitiatorScreen::Success;
    }

    /// Terminal failure. `reason` is the stable `LinkInitiator` failure
    /// id. Renders `exchange_link_failed`. Inert once cancelled; first
    /// terminal transition wins.
    pub fn transition_to_failed(&mut self, reason: String) {
        if self.cancelled || self.state.is_terminal() {
            return;
        }
        self.state = InitiatorScreen::Failed { reason };
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.state {
            InitiatorScreen::ShareUrl => self.build_share_url_screen(),
            InitiatorScreen::WaitingForResponse => self.build_waiting_screen(),
            InitiatorScreen::Retrieving => self.build_retrieving_screen(),
            InitiatorScreen::Success => self.build_success_screen(),
            InitiatorScreen::Failed { reason } => self.build_failed_screen(reason),
        }
    }

    fn build_share_url_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_share_url".into(),
            title: self.t("link_exchange.share_link_title"),
            subtitle: Some(self.t("link_exchange.share_link_subtitle")),
            components: vec![Component::Text {
                id: "link_url".into(),
                content: self.share_url.clone(),
                style: TextStyle::Body,
                // The URL carries a public key. Rendering it is necessary
                // — the user has to share it — but reciting 43 characters
                // of base64 to everyone in earshot is not, and
                // `logging-rules.md` already forbids the same material
                // reaching a log.
                a11y: Some(A11y {
                    label: Some(self.t("link_exchange.share_link_a11y")),
                    hint: Some(self.t("link_exchange.share_link_a11y_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: ACTION_SHARE.into(),
                    label: self.t("action.share"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.share"))),
                },
                ScreenAction {
                    id: ACTION_CANCEL.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            ..Default::default()
        }
    }

    fn build_waiting_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_link_waiting".into(),
            title: self.t("link_exchange.waiting_title"),
            subtitle: Some(self.t("link_exchange.waiting_subtitle")),
            components: vec![Component::StatusIndicator {
                id: "waiting_status".into(),
                icon: None,
                title: self.t("link_exchange.waiting_status"),
                detail: Some(self.t("link_exchange.waiting_detail")),
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("link_exchange.waiting_a11y")),
                    hint: Some(self.t("link_exchange.waiting_a11y_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![ScreenAction {
                id: ACTION_CANCEL.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            ..Default::default()
        }
    }

    fn build_retrieving_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_link_retrieving".into(),
            title: self.t("link_exchange.completing_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "retrieving_status".into(),
                icon: None,
                title: self.t("link_exchange.retrieving_status"),
                detail: None,
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("link_exchange.retrieving_a11y")),
                    hint: Some(self.t("link_exchange.retrieving_a11y_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![],
            ..Default::default()
        }
    }

    fn build_success_screen(&self) -> ScreenModel {
        // Rich, core-driven success screen (received card + visibility +
        // groups) when the lifecycle attached a summary; otherwise the
        // minimal completion chrome below.
        if let Some(summary) = &self.success_summary {
            return crate::ui::exchange::success::build_exchange_success_screen(
                "exchange_link_success",
                self.t(crate::ui::exchange::success::completion_title_key(
                    summary.is_reconnection,
                )),
                ACTION_DONE,
                summary,
                self.locale,
            );
        }
        ScreenModel {
            screen_id: "exchange_link_success".into(),
            title: self.t("link_exchange.contact_added_title"),
            subtitle: Some(self.t("link_exchange.contact_added_subtitle")),
            components: vec![Component::StatusIndicator {
                id: "success_status".into(),
                icon: None,
                title: self.t("link_exchange.success_status"),
                detail: Some(self.t("link_exchange.success_detail")),
                status: Status::Success,
                status_label: self.t(Status::Success.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("link_exchange.contact_added_a11y")),
                    hint: None,
                    role: None,
                }),
            }],
            contextual_actions: vec![ScreenAction {
                id: ACTION_DONE.into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
            ..Default::default()
        }
    }

    fn build_failed_screen(&self, reason: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_link_failed".into(),
            title: self.t("link_exchange.link_failed_title"),
            subtitle: Some(self.t("link_exchange.link_failed_subtitle")),
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: self.t("exchange.terminal.failed"),
                detail: Some(failure_detail(reason, self.locale)),
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("link_exchange.link_failed_a11y")),
                    hint: None,
                    role: None,
                }),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: ACTION_RETRY.into(),
                    label: self.t("action.try_again"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.try_again"))),
                },
                ScreenAction {
                    id: ACTION_CANCEL.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            ..Default::default()
        }
    }
}

/// Map the stable `LinkInitiator` failure id to a user-facing detail.
fn failure_detail(reason: &str, locale: Locale) -> String {
    let key = match reason {
        "polling_timed_out" => "link_exchange.reason_polling_timed_out",
        "handshake_failed" => "link_exchange.reason_handshake_failed",
        "deposit_rejected" => "link_exchange.reason_deposit_rejected",
        "decrypt_error" => "link_exchange.reason_decrypt_error",
        "self_exchange" => "link_exchange.reason_self_exchange",
        "contact_limit" => "link_exchange.reason_contact_limit",
        "cancelled" => "link_exchange.reason_cancelled",
        _ => "link_exchange.reason_generic",
    };
    get_string(locale, key)
}

impl WorkflowEngine for LinkExchangeEngine {
    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        use crate::ui::LinkExchangeUpdate as U;
        let crate::ui::EngineUpdate::LinkExchange(update) = update else {
            return false;
        };
        match update {
            U::ShareUrl(url) => self.set_share_url(url),
            U::Waiting => self.transition_to_waiting(),
            U::Retrieving => self.transition_to_retrieving(),
            U::Succeeded(summary) => {
                self.set_success_summary(summary);
                self.transition_to_success();
            }
            U::Failed(id) => self.transition_to_failed(id),
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // On the success screen, Done ends the flow. On the failed screen,
        // Retry restarts a fresh link exchange and Cancel ends the flow.
        match &self.state {
            InitiatorScreen::Success => {
                return match action {
                    UserAction::ActionPressed { action_id } if action_id == ACTION_DONE => {
                        ActionResult::Complete
                    }
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                };
            }
            InitiatorScreen::Failed { .. } => {
                return match action {
                    UserAction::ActionPressed { action_id } if action_id == ACTION_RETRY => {
                        ActionResult::StartLinkExchange
                    }
                    UserAction::ActionPressed { action_id } if action_id == ACTION_CANCEL => {
                        self.cancelled = true;
                        ActionResult::Complete
                    }
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                };
            }
            _ => {}
        }

        // Once cancelled, further actions are inert — the lifecycle is
        // winding the session down.
        if self.cancelled {
            return ActionResult::UpdateScreen(self.build_screen());
        }

        match (&self.state, action) {
            (InitiatorScreen::ShareUrl, UserAction::ActionPressed { action_id })
                if action_id == ACTION_SHARE =>
            {
                self.state = InitiatorScreen::WaitingForResponse;
                ActionResult::Commands {
                    commands: vec![Command::ShowShareSheet {
                        url: self.share_url.clone(),
                    }],
                }
            }
            (_, UserAction::ActionPressed { action_id }) if action_id == ACTION_CANCEL => {
                self.cancelled = true;
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    /// The relay escrow lifecycle is core/event-driven (no platform
    /// bridge): hardware events reach the engine-owned
    /// `LinkInitiatorSession` through the AppEngine link-initiator
    /// routing, not through this renderer. The renderer ignores them.
    fn handle_hardware_event(&mut self, _event: Event) -> Option<ActionResult> {
        None
    }
}

// INLINE_TEST_REQUIRED: exercises the success-screen rich/minimal branch
// against the crate-internal `ExchangeSuccessSummary` (pub(crate)); the
// engine's public-API behavior is covered in `tests/it/link_exchange_tests.rs`.
#[cfg(test)]
mod success_summary_tests {
    use super::*;
    use crate::ui::exchange::success::ExchangeSuccessSummary;

    // @internal
    #[test]
    fn success_screen_renders_rich_summary_when_attached() {
        let mut engine = LinkExchangeEngine::new();
        engine.set_success_summary(ExchangeSuccessSummary {
            peer_name: "Bob".into(),
            received_fields: vec![("email".into(), "Email".into(), "bob@example.com".into())],
            my_visible_fields: vec!["Phone".into()],
            group_names: Vec::new(),
            is_reconnection: false,
        });
        engine.transition_to_success();
        let screen = engine.build_screen();
        assert_eq!(screen.screen_id, "exchange_link_success");
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::FieldList { id, .. } if id == "received_fields"
            )),
            "rich success screen must render the received card fields",
        );
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::InfoPanel { id, .. } if id == "my_visibility"
            )),
            "rich success screen must render the visibility section",
        );
    }

    // @internal
    #[test]
    fn success_screen_without_summary_falls_back_to_minimal_chrome() {
        let mut engine = LinkExchangeEngine::new();
        engine.transition_to_success();
        let screen = engine.build_screen();
        assert_eq!(screen.screen_id, "exchange_link_success");
        // No summary attached → the minimal StatusIndicator chrome, no
        // received-fields section.
        assert!(
            !screen.components.iter().any(|c| matches!(
                c,
                Component::FieldList { id, .. } if id == "received_fields"
            )),
            "minimal success screen must not render a received-fields section",
        );
    }
}
