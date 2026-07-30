// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency shred engine — full data wipe workflow with confirmation.
//! Copy resolves through `i18n::get_string` in the locale threaded at
//! construction (M3 S3 of `2026-07-03-core-screens-bypass-i18n`); the
//! keys live in the `shred.wipe.*` + shared `action.*` families.

use crate::i18n::{Locale, get_string};
use crate::ui::*;

/// Emergency data shred workflow engine.
#[derive(Clone, Debug)]
pub struct EmergencyShredEngine {
    step: ShredStep,
    typed_confirmation: String,
    locale: Locale,
}

#[derive(Clone, Debug, PartialEq)]
enum ShredStep {
    Warning,
    Confirm,
    Wiping,
    Complete,
}

impl Default for EmergencyShredEngine {
    fn default() -> Self {
        Self::new(Locale::English)
    }
}

impl EmergencyShredEngine {
    pub fn new(locale: Locale) -> Self {
        Self {
            step: ShredStep::Warning,
            typed_confirmation: String::new(),
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Signal that the wipe operation has finished. Moves from Wiping to Complete.
    pub fn wipe_complete(&mut self) {
        if self.step == ShredStep::Wiping {
            self.step = ShredStep::Complete;
        }
    }

    fn warning_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "shred_warning".into(),
            title: self.t("shred.wipe.title"),
            subtitle: None,
            components: vec![Component::InfoPanel {
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
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "continue".into(),
                    label: self.t("shred.wipe.understand"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("shred.wipe.understand"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: Some(Progress {
                current_step: 1,
                total_steps: 3,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn confirm_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "shred_confirm".into(),
            title: self.t("shred.wipe.confirm_title"),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "confirmation".into(),
                label: self.t("shred.wipe.type_delete"),
                value: self.typed_confirmation.clone(),
                placeholder: None,
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y::labeled(self.t("shred.wipe.type_delete"))),
                info_key: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "wipe".into(),
                    label: self.t("shred.wipe.wipe_all"),
                    style: ActionStyle::Destructive,
                    enabled: self.typed_confirmation == "DELETE",
                    a11y: Some(A11y::labeled(self.t("shred.wipe.wipe_all"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: Some(Progress {
                current_step: 2,
                total_steps: 3,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn wiping_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "shred_wiping".into(),
            title: self.t("shred.wipe.wiping_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "wipe_status".into(),
                icon: None,
                title: self.t("shred.wipe.wiping_status"),
                detail: None,
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: None,
            }],
            contextual_actions: vec![],
            progress: Some(Progress {
                current_step: 3,
                total_steps: 3,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn complete_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "shred_complete".into(),
            title: self.t("shred.wipe.complete_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "wipe_status".into(),
                icon: None,
                title: self.t("shred.wipe.complete_title"),
                detail: None,
                status: Status::Success,
                status_label: self.t(Status::Success.label_key()),
                a11y: None,
            }],
            contextual_actions: vec![ScreenAction {
                id: "done".into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
            progress: Some(Progress {
                current_step: 3,
                total_steps: 3,
                label: None,
            }),
            ..Default::default()
        }
    }
}

impl WorkflowEngine for EmergencyShredEngine {
    fn current_screen(&self) -> ScreenModel {
        match self.step {
            ShredStep::Warning => self.warning_screen(),
            ShredStep::Confirm => self.confirm_screen(),
            ShredStep::Wiping => self.wiping_screen(),
            ShredStep::Complete => self.complete_screen(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id }
                if action_id == "cancel"
                    && (self.step == ShredStep::Warning || self.step == ShredStep::Confirm) =>
            {
                ActionResult::Complete
            }
            UserAction::ActionPressed { action_id } if action_id == "continue" => {
                if self.step == ShredStep::Warning {
                    self.step = ShredStep::Confirm;
                    ActionResult::NavigateTo(self.current_screen())
                } else {
                    ActionResult::UpdateScreen(self.current_screen())
                }
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "confirmation" => {
                self.typed_confirmation = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "wipe" => {
                if self.step == ShredStep::Confirm {
                    // The typed token stays the literal DELETE in every
                    // locale — the gate checks the token, the label
                    // explains it (see shred_i18n_tests).
                    if self.typed_confirmation != "DELETE" {
                        ActionResult::ValidationError {
                            component_id: "confirmation".into(),
                            message: self.t("shred.wipe.type_delete"),
                        }
                    } else {
                        self.step = ShredStep::Wiping;
                        ActionResult::NavigateTo(self.current_screen())
                    }
                } else {
                    ActionResult::UpdateScreen(self.current_screen())
                }
            }
            UserAction::ActionPressed { action_id }
                if action_id == "done" && self.step == ShredStep::Complete =>
            {
                ActionResult::WipeComplete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
