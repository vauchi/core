// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fingerprint verification workflow engine.
//!
//! Displays both fingerprints (ours and theirs) for in-person comparison.
//! The user confirms they match, marking the contact as verified.
//! Verified contacts can also be unverified.

use crate::i18n::{Locale, get_string};
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
    locale: Locale,
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

    pub fn contact_id(&self) -> &str {
        &self.contact_id
    }

    pub fn completion_action(&self) -> VerifyAction {
        self.action.clone()
    }

    fn build_screen(&self) -> ScreenModel {
        let status_text = if self.is_verified {
            self.t("fingerprint_verify.status_verified")
        } else {
            self.t("fingerprint_verify.status_compare")
        };

        let mut components = vec![
            Component::Text {
                a11y: None,
                id: "instructions".into(),
                content: status_text,
                style: if self.is_verified {
                    TextStyle::Subtitle
                } else {
                    TextStyle::Body
                },
            },
            Component::Text {
                a11y: None,
                id: "their_label".into(),
                content: self.t("fingerprint_verify.their_label"),
                style: TextStyle::Caption,
            },
            Component::Text {
                a11y: None,
                id: "their_fingerprint".into(),
                content: self.their_fingerprint.clone(),
                style: TextStyle::Body,
            },
            Component::Text {
                a11y: None,
                id: "our_label".into(),
                content: self.t("fingerprint_verify.our_label"),
                style: TextStyle::Caption,
            },
            Component::Text {
                a11y: None,
                id: "our_fingerprint".into(),
                content: self.our_fingerprint.clone(),
                style: TextStyle::Body,
            },
        ];

        if self.is_verified {
            components.push(Component::Text {
                a11y: None,
                id: "verified_badge".into(),
                content: self.t("contacts.verified"),
                style: TextStyle::Subtitle,
            });
        }

        let mut actions = Vec::new();
        if self.is_verified {
            actions.push(ScreenAction {
                id: "unverify".into(),
                label: self.t("fingerprint_verify.remove_verification_button"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            });
        } else {
            actions.push(ScreenAction {
                id: "confirm_match".into(),
                label: self.t("fingerprint_verify.confirm_match_button"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            });
        }
        // Back is the frontend's core-driven back chrome now (gated on
        // `can_go_back`); no footer "Back" — see 2026-06-05-core-driven-back-chrome.

        ScreenModel {
            screen_id: "fingerprint_verify".into(),
            title: self.t("contact_detail.verify_fingerprint_button"),
            subtitle: None,
            components,
            contextual_actions: actions,
            progress: None,
            ..Default::default()
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
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn was_cancelled(&self) -> bool {
        self.action == VerifyAction::None
    }

    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::FingerprintVerify(self.action.clone()))
    }
}
