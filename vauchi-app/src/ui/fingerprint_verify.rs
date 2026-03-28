// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fingerprint verification workflow engine.
//!
//! Displays both fingerprints (ours and theirs) for in-person comparison.
//! The user confirms they match, marking the contact as verified.

use crate::ui::*;

/// Engine for the fingerprint verification screen.
#[derive(Clone, Debug)]
pub struct FingerprintVerifyEngine {
    contact_id: String,
    their_fingerprint: String,
    our_fingerprint: String,
    is_verified: bool,
    cancelled: bool,
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
            cancelled: false,
        }
    }

    pub fn contact_id(&self) -> &str {
        &self.contact_id
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
        if !self.is_verified {
            actions.push(ScreenAction {
                id: "confirm_match".into(),
                label: "I've verified in person".into(),
                style: ActionStyle::Primary,
                enabled: true,
            });
        }
        actions.push(ScreenAction {
            id: "back".into(),
            label: "Back".into(),
            style: ActionStyle::Secondary,
            enabled: true,
        });

        ScreenModel {
            screen_id: "fingerprint_verify".into(),
            title: "Verify Fingerprint".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
        }
    }
}

impl WorkflowEngine for FingerprintVerifyEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "confirm_match" => {
                    self.is_verified = true;
                    ActionResult::Complete
                }
                "back" => {
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
}
