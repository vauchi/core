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
        }
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

    fn progress(&self) -> Progress {
        let current = match self.state {
            InitiatorScreen::ShareUrl => 1,
            InitiatorScreen::WaitingForResponse => 2,
            InitiatorScreen::Retrieving
            | InitiatorScreen::Success
            | InitiatorScreen::Failed { .. } => 3,
        };
        Progress {
            current_step: current,
            total_steps: 3,
            label: None,
        }
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
            title: "Share Link".into(),
            subtitle: Some("Send this link to exchange contacts".into()),
            components: vec![Component::Text {
                id: "link_url".into(),
                content: self.share_url.clone(),
                style: TextStyle::Body,
            }],
            actions: vec![
                ScreenAction {
                    id: ACTION_SHARE.into(),
                    label: "Share".into(),
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
            progress: Some(self.progress()),
            ..Default::default()
        }
    }

    fn build_waiting_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_link_waiting".into(),
            title: "Waiting for Response".into(),
            subtitle: Some("The link has been shared. Waiting for the other person...".into()),
            components: vec![Component::StatusIndicator {
                id: "waiting_status".into(),
                icon: None,
                title: "Waiting...".into(),
                detail: Some("They need to open the link to complete the exchange.".into()),
                status: Status::InProgress,
                a11y: Some(A11y {
                    label: Some("Waiting for response".into()),
                    hint: Some(
                        "The other person needs to open the link to complete the exchange".into(),
                    ),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: ACTION_CANCEL.into(),
                label: "Cancel".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            progress: Some(self.progress()),
            ..Default::default()
        }
    }

    fn build_retrieving_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_link_retrieving".into(),
            title: "Completing Exchange".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "retrieving_status".into(),
                icon: None,
                title: "Retrieving contact...".into(),
                detail: None,
                status: Status::InProgress,
                a11y: Some(A11y {
                    label: Some("Retrieving contact".into()),
                    hint: Some("Fetching and decrypting the contact card".into()),
                    role: None,
                }),
            }],
            actions: vec![],
            progress: Some(self.progress()),
            ..Default::default()
        }
    }

    fn build_success_screen(&self) -> ScreenModel {
        // Rich, core-driven success screen (received card + visibility +
        // groups) when the lifecycle attached a summary; otherwise the
        // minimal completion chrome below.
        if let Some(summary) = &self.success_summary {
            let mut screen = crate::ui::exchange::success::build_exchange_success_screen(
                "exchange_link_success",
                "Contact Added",
                ACTION_DONE,
                summary,
            );
            screen.progress = Some(self.progress());
            return screen;
        }
        ScreenModel {
            screen_id: "exchange_link_success".into(),
            title: "Contact Added".into(),
            subtitle: Some("The other person's contact card has been saved.".into()),
            components: vec![Component::StatusIndicator {
                id: "success_status".into(),
                icon: None,
                title: "Done".into(),
                detail: Some("You can find the new contact in your contacts list.".into()),
                status: Status::Success,
                a11y: Some(A11y {
                    label: Some("Contact added".into()),
                    hint: None,
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
            progress: Some(self.progress()),
            ..Default::default()
        }
    }

    fn build_failed_screen(&self, reason: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "exchange_link_failed".into(),
            title: "Link Failed".into(),
            subtitle: Some("The contact exchange could not be completed.".into()),
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: "Failed".into(),
                detail: Some(failure_detail(reason).into()),
                status: Status::Failed,
                a11y: Some(A11y {
                    label: Some("Link failed".into()),
                    hint: None,
                    role: None,
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: ACTION_RETRY.into(),
                    label: "Try Again".into(),
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
            progress: Some(self.progress()),
            ..Default::default()
        }
    }
}

/// Map the stable `LinkInitiator` failure id to a user-facing detail.
fn failure_detail(reason: &str) -> &'static str {
    match reason {
        "polling_timed_out" => {
            "The other device did not respond in time. Share the link again to retry."
        }
        "handshake_failed" => "The secure handshake failed. Please share a fresh link.",
        "deposit_rejected" => "The relay rejected the exchange. Please try sharing the link again.",
        "decrypt_error" => "The received card could not be decrypted. Please try again.",
        "cancelled" => "The exchange was cancelled.",
        _ => "Something went wrong completing the exchange. Please try again.",
    }
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
