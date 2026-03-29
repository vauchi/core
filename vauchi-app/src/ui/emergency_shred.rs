// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency shred engine — full data wipe workflow with confirmation.

use crate::ui::*;

/// Emergency data shred workflow engine.
#[derive(Clone, Debug)]
pub struct EmergencyShredEngine {
    step: ShredStep,
    typed_confirmation: String,
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
        Self::new()
    }
}

impl EmergencyShredEngine {
    pub fn new() -> Self {
        Self {
            step: ShredStep::Warning,
            typed_confirmation: String::new(),
        }
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
            title: "Emergency Data Wipe".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "warning_info".into(),
                icon: Some("warning".into()),
                title: "Emergency Data Wipe".into(),
                items: vec![
                    InfoItem {
                        icon: Some("delete".into()),
                        title: "All contacts will be deleted".into(),
                        detail:
                            "Your contact cards and exchange history will be permanently removed."
                                .into(),
                    },
                    InfoItem {
                        icon: Some("key".into()),
                        title: "All keys will be destroyed".into(),
                        detail:
                            "Encryption keys will be securely shredded and cannot be recovered."
                                .into(),
                    },
                    InfoItem {
                        icon: Some("warning".into()),
                        title: "This action is irreversible".into(),
                        detail: "There is no way to undo this operation.".into(),
                    },
                ],
            }],
            actions: vec![
                ScreenAction {
                    id: "continue".into(),
                    label: "I Understand".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
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
            title: "Confirm Wipe".into(),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "confirmation".into(),
                label: "Type DELETE to confirm".into(),
                value: self.typed_confirmation.clone(),
                placeholder: None,
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
            }],
            actions: vec![
                ScreenAction {
                    id: "wipe".into(),
                    label: "Wipe All Data".into(),
                    style: ActionStyle::Destructive,
                    enabled: self.typed_confirmation == "DELETE",
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
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
            title: "Wiping Data".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "wipe_status".into(),
                icon: None,
                title: "Wiping data...".into(),
                detail: None,
                status: Status::InProgress,
            }],
            actions: vec![],
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
            title: "Data Wiped".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "wipe_status".into(),
                icon: None,
                title: "Data Wiped".into(),
                detail: None,
                status: Status::Success,
            }],
            actions: vec![ScreenAction {
                id: "done".into(),
                label: "Done".into(),
                style: ActionStyle::Primary,
                enabled: true,
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
                    if self.typed_confirmation != "DELETE" {
                        ActionResult::ValidationError {
                            component_id: "confirmation".into(),
                            message: "Type DELETE to confirm".into(),
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
