// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine for the post-grant link-mode responder flow.
//!
//! Renders a single waiting screen while the cycle thread (in
//! `vauchi-platform`) drives `LinkResponderSession` through
//! Polling → Retrieving → Finalized. The engine itself is a humble
//! object: it forwards a `cancel` user action through `was_cancelled`
//! so `AppEngine::handle_completion` can route back to the default
//! screen, but it never blocks on or polls the cycle thread.
//!
//! Mirrors `DeepLinkConsentEngine` shape — single-screen, single
//! decision, terminal on action.
//!
//! See `_private/docs/problems/2026-04-27-deep-link-responder-flow/`
//! for the full design (Q4 — single-screen choice).

use crate::ui::*;
use vauchi_core::exchange::link_mode::DeepLinkPayload;

/// Action id for the Cancel button on the waiting screen.
pub const ACTION_CANCEL: &str = "cancel";

/// Engine for the post-grant link-mode responder flow.
///
/// Holds the parsed payload so the cycle thread (Phase 1 T6) can
/// retrieve it via `payload()`. The `cancelled` flag flips on the
/// Cancel action; the cycle thread observes via the platform-side
/// session's `cancel()` method.
#[derive(Clone, Debug)]
pub struct LinkResponderEngine {
    payload: DeepLinkPayload,
    cancelled: bool,
}

impl LinkResponderEngine {
    /// Build a fresh engine holding `payload` (received from the
    /// `DeepLinkConsentEngine` grant action via
    /// `ActionResult::NavigateTo(AppScreen::DeepLinkResponder { payload })`).
    pub fn new(payload: DeepLinkPayload) -> Self {
        Self {
            payload,
            cancelled: false,
        }
    }

    /// Borrow the parsed payload. Used by the cycle thread to drive
    /// `link_mode::responder_*` once the engine is created. Not
    /// surfaced via UniFFI.
    pub fn payload(&self) -> &DeepLinkPayload {
        &self.payload
    }

    fn build_screen(&self) -> ScreenModel {
        // One screen for the entire grant→completion window. The
        // cycle thread's three internal states (Depositing / Polling /
        // Retrieving) are surfaced via the listener trait but do not
        // branch this ScreenModel — Depositing and Retrieving are
        // sub-second flashes, only Polling is user-perceptible.
        ScreenModel {
            screen_id: "link_responder_waiting".into(),
            title: "Waiting for Response".into(),
            subtitle: Some("Connecting via the shared link...".into()),
            components: vec![Component::StatusIndicator {
                id: "waiting_status".into(),
                icon: None,
                title: "Waiting...".into(),
                detail: Some(
                    "The sender's contact card will appear here once the relay \
                     has matched both deposits."
                        .into(),
                ),
                status: Status::InProgress,
                a11y: Some(A11y {
                    label: Some("Waiting for response".into()),
                    hint: Some(
                        "The sender's encrypted card will be retrieved when both \
                         sides' deposits have matched on the relay"
                            .into(),
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
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for LinkResponderEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // Once cancelled, further actions are inert — the cycle thread
        // is winding down and re-firing cancel on a re-press would race
        // the listener's on_session_ended callback.
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
