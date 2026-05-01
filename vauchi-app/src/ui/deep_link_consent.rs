// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deep-link consent gate engine.
//!
//! Owns the `pending → granted/denied` state machine for incoming
//! `vauchi://exchange?pk=<b64>&n=<b64>` URIs, replacing the duplicated
//! `DeepLinkHandler` state machines that previously lived in
//! `ios/Vauchi/Services/DeepLinkHandler.swift` and
//! `android/app/src/main/kotlin/app/vauchi/deeplink/DeepLinkHandler.kt`.
//!
//! Per ADR-021 (Humble UI): the consent decision is policy ("never
//! auto-process a deep link"), not native presentation; it lives in
//! core. Frontends render the `ScreenModel` natively and forward
//! `UserAction`s. Per ADR-022: the gate is a dedicated screen with
//! Primary "Accept Exchange" + Secondary "Decline" actions, not an
//! `InlineConfirm` component (it is the confirmation, not an inline
//! prompt within another screen).
//!
//! Phase 1 (this commit): grant terminates the engine via
//! `ActionResult::Complete`, mirroring today's no-op grant behaviour
//! on both frontends. Phase 3 (sibling record
//! `2026-04-27-deep-link-responder-flow`) replaces the grant path
//! with `ActionResult::NavigateTo(...)` toward the responder cycle.

use crate::ui::*;
use vauchi_core::exchange::link_mode::DeepLinkPayload;

/// Action id for the "Accept Exchange" button (grant consent).
pub const ACTION_GRANT: &str = "grant";

/// Action id for the "Decline" button (deny consent).
pub const ACTION_DENY: &str = "deny";

/// Decision recorded by the consent gate. Pending until the user
/// presses one of the two top-level actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsentDecision {
    Pending,
    Granted,
    Denied,
}

/// Engine that owns the deep-link consent gate.
///
/// Holds the parsed payload until the user grants or denies. On
/// either decision, returns `ActionResult::Complete`; further
/// actions after a decision are inert (returns
/// `ActionResult::UpdateScreen` with no state change).
#[derive(Clone, Debug)]
pub struct DeepLinkConsentEngine {
    payload: DeepLinkPayload,
    decision: ConsentDecision,
    cancelled: bool,
}

impl DeepLinkConsentEngine {
    /// Build a fresh engine holding `payload`. The payload stays
    /// private until the user grants — `current_screen()` does not
    /// expose any of its bytes (privacy: don't echo unverified
    /// key material).
    pub fn new(payload: DeepLinkPayload) -> Self {
        Self {
            payload,
            decision: ConsentDecision::Pending,
            cancelled: false,
        }
    }

    /// The current decision. Public for AppEngine's grant-path
    /// dispatch (Phase 3) and for tests.
    pub fn decision(&self) -> ConsentDecision {
        self.decision
    }

    /// Borrow the parsed payload. Used by Phase 3 to drive
    /// `link_mode::responder_*` once consent is granted; not
    /// surfaced via UniFFI.
    pub fn payload(&self) -> &DeepLinkPayload {
        &self.payload
    }

    fn build_screen(&self) -> ScreenModel {
        let components = vec![
            Component::Banner {
                text: "Someone shared an exchange link with you. Only accept if \
                       you trust the source."
                    .into(),
                action_label: String::new(),
                action_id: String::new(),
                a11y: None,
            },
            Component::Divider,
        ];

        let actions = vec![
            ScreenAction {
                id: ACTION_GRANT.into(),
                label: "Accept Exchange".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            },
            ScreenAction {
                id: ACTION_DENY.into(),
                label: "Decline".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
        ];

        ScreenModel {
            screen_id: "deep_link_consent".into(),
            title: "Exchange Request".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for DeepLinkConsentEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // Once a decision is made, further actions are inert. This
        // closes the "tap-twice" race the original frontend handlers
        // had, where a fast double-tap could process the grant twice.
        if self.decision != ConsentDecision::Pending {
            return ActionResult::UpdateScreen(self.build_screen());
        }

        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                ACTION_GRANT => {
                    self.decision = ConsentDecision::Granted;
                    ActionResult::Complete
                }
                ACTION_DENY => {
                    self.decision = ConsentDecision::Denied;
                    self.cancelled = true;
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
