// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fingerprint verification workflow engine.
//!
//! Displays both fingerprints (ours and theirs) for in-person comparison.
//! The user confirms they match, marking the contact as verified.
//! Verified contacts can also be unverified.

use crate::ui::*;

/// What action the user took on the fingerprint verification screen.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VerifyAction {
    /// No action — user pressed Back.
    None,
    /// User confirmed fingerprint match.
    Verified,
    /// User removed fingerprint verification.
    Unverified,
}

/// Engine for the fingerprint verification screen.
#[derive(Clone, Debug)]
pub struct FingerprintVerifyEngine {
    contact_id: String,
    their_fingerprint: String,
    our_fingerprint: String,
    is_verified: bool,
    action: VerifyAction,
}

impl FingerprintVerifyEngine {
    pub fn new(
        contact_id: &str,
        their_fingerprint: &str,
        our_fingerprint: &str,
        is_verified: bool,
    ) -> Self {
        Self {
            contact_id: contact_id.to_string(),
            their_fingerprint: their_fingerprint.to_string(),
            our_fingerprint: our_fingerprint.to_string(),
            is_verified,
            action: VerifyAction::None,
        }
    }

    pub fn contact_id(&self) -> &str {
        &self.contact_id
    }

    pub fn completion_action(&self) -> VerifyAction {
        self.action.clone()
    }

    fn build_screen(&self) -> ScreenModel {
        let status_text = if self.is_verified {
            "Verified — fingerprints have been compared in person."
        } else {
            "Compare these fingerprints with your contact in person."
        };

        let mut components = vec![
            Component::Text {
                id: "instructions".into(),
                content: status_text.into(),
                style: if self.is_verified {
                    TextStyle::Subtitle
                } else {
                    TextStyle::Body
                },
            },
            Component::Text {
                id: "their_label".into(),
                content: "Their fingerprint".into(),
                style: TextStyle::Caption,
            },
            Component::Text {
                id: "their_fingerprint".into(),
                content: self.their_fingerprint.clone(),
                style: TextStyle::Body,
            },
            Component::Text {
                id: "our_label".into(),
                content: "Your fingerprint".into(),
                style: TextStyle::Caption,
            },
            Component::Text {
                id: "our_fingerprint".into(),
                content: self.our_fingerprint.clone(),
                style: TextStyle::Body,
            },
        ];

        if self.is_verified {
            components.push(Component::Text {
                id: "verified_badge".into(),
                content: "Verified".into(),
                style: TextStyle::Subtitle,
            });
        }

        let mut actions = Vec::new();
        if self.is_verified {
            actions.push(ScreenAction {
                id: "unverify".into(),
                label: "Remove verification".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            });
        } else {
            actions.push(ScreenAction {
                id: "confirm_match".into(),
                label: "I've verified in person".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            });
        }
        actions.push(ScreenAction {
            id: "back".into(),
            label: "Back".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });

        ScreenModel {
            screen_id: "fingerprint_verify".into(),
            title: "Verify Fingerprint".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for FingerprintVerifyEngine {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "confirm_match" if !self.is_verified => {
                    self.is_verified = true;
                    self.action = VerifyAction::Verified;
                    ActionResult::Complete
                }
                "unverify" if self.is_verified => {
                    self.is_verified = false;
                    self.action = VerifyAction::Unverified;
                    ActionResult::Complete
                }
                "back" => ActionResult::Complete,
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn was_cancelled(&self) -> bool {
        self.action == VerifyAction::None
    }
}
