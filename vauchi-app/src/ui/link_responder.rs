// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine for the post-grant link-mode responder flow.
//!
//! Renders the responder screens for the engine-owned `LinkResponder`
//! state machine (in `vauchi-platform`): a waiting screen while
//! Polling/Retrieving, then a terminal completed / failed screen. The
//! engine is a humble object — it forwards a `cancel` user action via
//! `was_cancelled` and exposes `transition_to_completed` /
//! `transition_to_failed` for the platform layer to drive on terminal
//! transitions. It never blocks on or polls the relay.
//!
//! Mirrors `DeviceLinkingEngine`'s terminal transitions
//! (`transition_to_link_success` / `transition_to_link_failed`).
//!
//! See `_private/docs/problems/2026-04-27-deep-link-responder-flow/`
//! and `_private/docs/designs/2026-05-25-slice-32l-phase-2-responder-screen-driven-design.md`.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use vauchi_core::exchange::link_mode::DeepLinkPayload;

/// Action id for the Cancel button on the waiting screen.
pub const ACTION_CANCEL: &str = "cancel";
/// Action id for the Done button on the terminal screens.
pub const ACTION_DONE: &str = "done";

/// Terminal outcome of the responder flow. `None` while waiting.
#[derive(Clone, Debug)]
enum ResponderTerminal {
    /// The sender's card was retrieved and persisted by core.
    Completed,
    /// The flow failed. `reason` is the stable `LinkResponder` failure
    /// id (`polling_timed_out` / `deposit_rejected` / `decrypt_error` /
    /// `cancelled`).
    Failed { reason: String },
}

/// Engine for the post-grant link-mode responder flow.
#[derive(Clone, Debug)]
pub struct LinkResponderEngine {
    payload: DeepLinkPayload,
    cancelled: bool,
    terminal: Option<ResponderTerminal>,
    /// Rich success-screen content, attached by the responder lifecycle
    /// once the sender's card is persisted. `None` → minimal chrome.
    success_summary: Option<crate::ui::exchange::success::ExchangeSuccessSummary>,
    locale: Locale,
}

impl LinkResponderEngine {
    /// Build a fresh engine holding `payload` (received from the
    /// `DeepLinkConsentEngine` grant action via
    /// `ActionResult::NavigateTo(AppScreen::DeepLinkResponder { payload })`).
    pub fn new(payload: DeepLinkPayload) -> Self {
        Self {
            payload,
            cancelled: false,
            terminal: None,
            success_summary: None,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Attach the rich success summary the completed screen renders. Inert
    /// once a terminal transition has occurred; call before
    /// `transition_to_completed`.
    pub fn set_success_summary(
        &mut self,
        summary: crate::ui::exchange::success::ExchangeSuccessSummary,
    ) {
        if self.cancelled || self.terminal.is_some() {
            return;
        }
        self.success_summary = Some(summary);
    }

    /// Borrow the parsed payload. Used by the platform layer to build the
    /// engine-owned `LinkResponder`. Not surfaced via UniFFI.
    pub fn payload(&self) -> &DeepLinkPayload {
        &self.payload
    }

    /// Terminal success — the sender's card was retrieved and persisted
    /// by core. Renders `link_responder_completed`. Idempotent.
    pub fn transition_to_completed(&mut self) {
        if self.terminal.is_none() {
            self.terminal = Some(ResponderTerminal::Completed);
        }
    }

    /// Terminal failure. `reason` is the stable `LinkResponder` failure
    /// id. Renders `link_responder_failed`. Idempotent (first terminal
    /// transition wins).
    pub fn transition_to_failed(&mut self, reason: String) {
        if self.terminal.is_none() {
            self.terminal = Some(ResponderTerminal::Failed { reason });
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.terminal {
            None => self.build_waiting_screen(),
            Some(ResponderTerminal::Completed) => self.build_completed_screen(),
            Some(ResponderTerminal::Failed { reason }) => self.build_failed_screen(reason),
        }
    }

    fn build_waiting_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_responder_waiting".into(),
            title: self.t("link_responder.waiting_title"),
            subtitle: Some(self.t("link_responder.waiting_subtitle")),
            components: vec![Component::StatusIndicator {
                id: "waiting_status".into(),
                icon: None,
                title: self.t("link_exchange.waiting_status"),
                detail: Some(self.t("link_responder.waiting_detail")),
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("link_responder.waiting_a11y")),
                    hint: Some(self.t("link_responder.waiting_a11y_hint")),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: ACTION_CANCEL.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn build_completed_screen(&self) -> ScreenModel {
        // Rich, core-driven success screen when the lifecycle attached a
        // summary; otherwise the minimal completion chrome below.
        if let Some(summary) = &self.success_summary {
            return crate::ui::exchange::success::build_exchange_success_screen(
                "link_responder_completed",
                self.t("link_exchange.contact_added_title"),
                ACTION_DONE,
                summary,
                self.locale,
            );
        }
        ScreenModel {
            screen_id: "link_responder_completed".into(),
            title: self.t("link_exchange.contact_added_title"),
            subtitle: Some(self.t("link_responder.completed_subtitle")),
            components: vec![Component::StatusIndicator {
                id: "completed_status".into(),
                icon: None,
                title: self.t("action.done"),
                detail: Some(self.t("link_responder.completed_detail")),
                status: Status::Success,
                status_label: self.t(Status::Success.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("link_responder.completed_a11y")),
                    hint: None,
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
            progress: None,
            ..Default::default()
        }
    }

    fn build_failed_screen(&self, reason: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "link_responder_failed".into(),
            title: self.t("link_responder.failed_title"),
            subtitle: Some(self.t("link_responder.failed_subtitle")),
            components: vec![Component::StatusIndicator {
                id: "failed_status".into(),
                icon: None,
                title: self.t("status.failed"),
                detail: Some(failure_detail(reason, self.locale)),
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("link_responder.failed_a11y")),
                    hint: None,
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
            progress: None,
            ..Default::default()
        }
    }
}

/// Map the stable `LinkResponder` failure id to a user-facing detail.
fn failure_detail(reason: &str, locale: Locale) -> String {
    let key = match reason {
        "polling_timed_out" => "link_responder.failure_timed_out",
        "deposit_rejected" => "link_responder.failure_rejected",
        "decrypt_error" => "link_responder.failure_decrypt_error",
        "cancelled" => "link_responder.failure_cancelled",
        _ => "link_responder.failure_generic",
    };
    get_string(locale, key)
}

impl WorkflowEngine for LinkResponderEngine {
    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        let crate::ui::EngineUpdate::LinkResponder(update) = update else {
            return false;
        };
        match update {
            crate::ui::LinkResponderUpdate::Completed(summary) => {
                self.set_success_summary(summary);
                self.transition_to_completed();
            }
            crate::ui::LinkResponderUpdate::Failed(reason) => self.transition_to_failed(reason),
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // On a terminal screen, the only action is Done → back to the
        // default screen. While waiting, Cancel ends the flow.
        if self.terminal.is_some() {
            return match action {
                UserAction::ActionPressed { action_id } if action_id == ACTION_DONE => {
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            };
        }

        // Once cancelled, further actions are inert — the platform layer
        // is winding the responder down.
        if self.cancelled {
            return ActionResult::UpdateScreen(self.build_screen());
        }

        match action {
            UserAction::ActionPressed { action_id } if action_id == ACTION_CANCEL => {
                self.cancelled = true;
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}

// INLINE_TEST_REQUIRED: exercises the completed-screen rich/minimal branch
// against the crate-internal `ExchangeSuccessSummary` (pub(crate)); the
// engine's public-API behavior is covered in `tests/it/`.
#[cfg(test)]
mod success_summary_tests {
    use super::*;
    use crate::ui::exchange::success::ExchangeSuccessSummary;
    use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};

    fn payload() -> DeepLinkPayload {
        let (init, _) = initiator_generate();
        parse_exchange_deep_link(&init.url).expect("canonical URL parses")
    }

    // @internal
    #[test]
    fn completed_screen_renders_rich_summary_when_attached() {
        let mut engine = LinkResponderEngine::new(payload());
        engine.set_success_summary(ExchangeSuccessSummary {
            peer_name: "Bob".into(),
            received_fields: vec![("email".into(), "Email".into(), "bob@example.com".into())],
            my_visible_fields: vec!["Phone".into()],
            group_names: Vec::new(),
        });
        engine.transition_to_completed();
        let screen = engine.build_screen();
        assert_eq!(screen.screen_id, "link_responder_completed");
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::FieldList { id, .. } if id == "received_fields"
            )),
            "rich completed screen must render the received card fields",
        );
    }

    // @internal
    #[test]
    fn completed_screen_without_summary_falls_back_to_minimal_chrome() {
        let mut engine = LinkResponderEngine::new(payload());
        engine.transition_to_completed();
        let screen = engine.build_screen();
        assert_eq!(screen.screen_id, "link_responder_completed");
        assert!(
            !screen.components.iter().any(|c| matches!(
                c,
                Component::FieldList { id, .. } if id == "received_fields"
            )),
            "minimal completed screen must not render a received-fields section",
        );
    }
}
