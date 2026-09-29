// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency shred engine — one screen: what the wipe destroys, a typed
//! WIPE confirmation, and a destructive Shred Everything / Cancel pair.
//! The AppEngine performs the wipe when this engine completes with
//! `Gdpr(Shred)` (#431). Copy resolves through `i18n::get_string` in the
//! locale threaded at construction; the keys live in the `shred.wipe.*`
//! family.

use crate::i18n::{Locale, get_string};
use crate::ui::*;

/// The typed token stays the literal WIPE in every locale — the gate
/// checks the token, the label explains it (see shred_i18n_tests).
const CONFIRMATION_WORD: &str = "WIPE";

/// Emergency data shred workflow engine.
#[derive(Clone, Debug)]
pub struct EmergencyShredEngine {
    typed_confirmation: String,
    confirmation_error: Option<String>,
    wipe_confirmed: bool,
    locale: Locale,
}

impl Default for EmergencyShredEngine {
    fn default() -> Self {
        Self::new(Locale::English)
    }
}

impl EmergencyShredEngine {
    pub fn new(locale: Locale) -> Self {
        Self {
            typed_confirmation: String::new(),
            confirmation_error: None,
            wipe_confirmed: false,
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Case and surrounding spaces are forgiven: keyboards auto-capitalise
    /// and append spaces, and the deliberate act is typing the word.
    fn confirmation_matches(&self) -> bool {
        self.typed_confirmation
            .trim()
            .eq_ignore_ascii_case(CONFIRMATION_WORD)
    }

    fn warning_panel(&self) -> Component {
        Component::InfoPanel {
            id: "warning_info".into(),
            icon: Some("warning".into()),
            title: self.t("shred.wipe.title"),
            items: vec![
                InfoItem {
                    icon: Some("delete".into()),
                    title: self.t("shred.wipe.contacts_title"),
                    detail: self.t("shred.wipe.contacts_detail"),
                },
                InfoItem {
                    icon: Some("key".into()),
                    title: self.t("shred.wipe.keys_title"),
                    detail: self.t("shred.wipe.keys_detail"),
                },
                InfoItem {
                    icon: Some("warning".into()),
                    title: self.t("shred.wipe.irreversible_title"),
                    detail: self.t("shred.wipe.irreversible_detail"),
                },
            ],
            a11y: None,
        }
    }

    fn confirmation_input(&self) -> Component {
        let label = self.t("shred.wipe.type_wipe");
        Component::TextInput {
            id: "confirmation".into(),
            label: label.clone(),
            value: self.typed_confirmation.clone(),
            placeholder: None,
            max_length: None,
            validation_error: self.confirmation_error.clone(),
            input_type: InputType::Text,
            a11y: Some(A11y::labeled(label)),
            info_key: None,
        }
    }

    fn shred_buttons(&self) -> Component {
        Component::InlineConfirm {
            id: "shred".into(),
            warning: self.t("shred.wipe.confirm_warning"),
            confirm_text: self.t("shred.panic_confirm_button"),
            cancel_text: self.t("action.cancel"),
            confirm_action_id: "confirm_shred".into(),
            cancel_action_id: "cancel_shred".into(),
            destructive: true,
            a11y: None,
        }
    }
}

impl WorkflowEngine for EmergencyShredEngine {
    fn current_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "shred_warning".into(),
            title: self.t("shred.wipe.title"),
            subtitle: Some(self.t("shred.wipe.subtitle")),
            components: vec![
                self.warning_panel(),
                self.confirmation_input(),
                self.shred_buttons(),
            ],
            contextual_actions: vec![],
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "cancel_shred" => {
                self.wipe_confirmed = false;
                ActionResult::Complete
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "confirmation" => {
                self.typed_confirmation = value;
                self.confirmation_error = None;
                self.wipe_confirmed = false;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "confirm_shred" => {
                if self.confirmation_matches() {
                    // The AppEngine performs the wipe on completion; if it
                    // fails the user stays here and can retry (#431).
                    self.wipe_confirmed = true;
                    ActionResult::Complete
                } else {
                    let message = self.t("shred.wipe.wrong_word");
                    self.confirmation_error = Some(message.clone());
                    ActionResult::ValidationError {
                        component_id: "confirmation".into(),
                        message,
                    }
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn engine_output(&self) -> Option<EngineOutput> {
        self.wipe_confirmed
            .then_some(EngineOutput::Gdpr(GdprChoice::Shred))
    }
}
