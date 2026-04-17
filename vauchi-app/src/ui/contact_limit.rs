// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact limit configuration engine.

use crate::ui::*;

/// Engine for configuring the maximum number of contacts.
#[derive(Clone, Debug)]
pub struct ContactLimitEngine {
    current_count: usize,
    current_limit: usize,
    editing: bool,
    limit_input: String,
}

impl ContactLimitEngine {
    pub fn new(current_count: usize, current_limit: usize) -> Self {
        Self {
            current_count,
            current_limit,
            editing: false,
            limit_input: current_limit.to_string(),
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let usage = if self.current_limit > 0 {
            let pct = (self.current_count as f64 / self.current_limit as f64) * 100.0;
            format!(
                "{} / {} contacts ({:.0}%)",
                self.current_count, self.current_limit, pct
            )
        } else {
            format!("{} contacts (no limit)", self.current_count)
        };

        let limit_display = if self.editing {
            format!("{}|", self.limit_input)
        } else {
            self.current_limit.to_string()
        };

        let components = vec![
            Component::Text {
                id: "info".into(),
                content: "Set a maximum number of contacts to manage storage.".into(),
                style: TextStyle::Body,
            },
            Component::Text {
                id: "usage".into(),
                content: usage,
                style: TextStyle::Subtitle,
            },
            Component::TextInput {
                id: "limit_input".into(),
                label: "Max Contacts".into(),
                value: limit_display,
                placeholder: Some("Enter limit".into()),
                max_length: Some(6),
                validation_error: None,
                input_type: InputType::Text,
                a11y: None,
                info_key: None,
            },
        ];

        let actions = if self.editing {
            vec![
                ScreenAction {
                    id: "save".into(),
                    label: "Save".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "cancel_edit".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ]
        } else {
            vec![ScreenAction {
                id: "edit".into(),
                label: "Edit Limit".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }]
        };

        ScreenModel {
            screen_id: "contact_limit".into(),
            title: "Contact Limit".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for ContactLimitEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "edit" => {
                    self.editing = true;
                    self.limit_input = self.current_limit.to_string();
                    ActionResult::UpdateScreen(self.build_screen())
                }
                "save" => {
                    if let Ok(new_limit) = self.limit_input.parse::<usize>() {
                        self.current_limit = new_limit;
                        self.editing = false;
                        ActionResult::Complete
                    } else {
                        ActionResult::ValidationError {
                            component_id: "limit_input".into(),
                            message: "Please enter a valid number".into(),
                        }
                    }
                }
                "cancel_edit" => {
                    self.editing = false;
                    self.limit_input = self.current_limit.to_string();
                    ActionResult::UpdateScreen(self.build_screen())
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            UserAction::TextChanged {
                component_id,
                value,
            } => {
                if component_id == "limit_input" && self.editing {
                    self.limit_input = value;
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
