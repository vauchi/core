// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact limit configuration engine.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Engine for configuring the maximum number of contacts.
#[derive(Clone, Debug)]
pub struct ContactLimitEngine {
    current_count: usize,
    current_limit: usize,
    editing: bool,
    limit_input: String,
    locale: Locale,
}

impl ContactLimitEngine {
    pub fn new(current_count: usize, current_limit: usize) -> Self {
        Self {
            current_count,
            current_limit,
            editing: false,
            limit_input: current_limit.to_string(),
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-13).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn build_screen(&self) -> ScreenModel {
        let usage = if self.current_limit > 0 {
            let pct = (self.current_count as f64 / self.current_limit as f64) * 100.0;
            get_string_with_args(
                self.locale,
                "contact_limit.usage_with_limit",
                &[
                    ("count", &self.current_count.to_string()),
                    ("limit", &self.current_limit.to_string()),
                    ("pct", &format!("{pct:.0}")),
                ],
            )
        } else {
            get_string_with_args(
                self.locale,
                "contact_limit.usage_no_limit",
                &[("count", &self.current_count.to_string())],
            )
        };

        let limit_display = if self.editing {
            format!("{}|", self.limit_input)
        } else {
            self.current_limit.to_string()
        };

        let components = vec![
            Component::Text {
                id: "info".into(),
                content: self.t("contact_limit.info"),
                style: TextStyle::Body,
            },
            Component::Text {
                id: "usage".into(),
                content: usage,
                style: TextStyle::Subtitle,
            },
            Component::TextInput {
                id: "limit_input".into(),
                label: self.t("contact_limit.max_contacts_label"),
                value: limit_display,
                placeholder: Some(self.t("contact_limit.enter_limit_placeholder")),
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
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel_edit".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ]
        } else {
            vec![ScreenAction {
                id: "edit".into(),
                label: self.t("contact_limit.edit_limit_button"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }]
        };

        ScreenModel {
            screen_id: "contact_limit".into(),
            title: self.t("contact_limit.title"),
            subtitle: None,
            components,
            contextual_actions: actions,
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
                            message: self.t("contact_limit.invalid_number_error"),
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
