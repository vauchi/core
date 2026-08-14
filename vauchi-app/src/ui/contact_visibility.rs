// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact visibility engine — per-field visibility toggles for a specific contact.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Engine that displays per-field visibility toggles for a contact.
#[derive(Clone, Debug)]
pub struct ContactVisibilityEngine {
    contact_name: String,
    fields: Vec<ToggleItem>,
    locale: Locale,
}

impl ContactVisibilityEngine {
    pub fn new(contact_name: String, fields: Vec<ToggleItem>) -> Self {
        Self {
            contact_name,
            fields,
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

    fn build_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_visibility".into(),
            title: get_string_with_args(
                self.locale,
                "contact_visibility.title",
                &[("name", &self.contact_name)],
            ),
            subtitle: None,
            components: vec![
                Component::Text {
                    a11y: None,
                    id: "visibility_info".into(),
                    content: self.t("contact_visibility.info"),
                    style: TextStyle::Body,
                },
                Component::ToggleList {
                    id: "field_toggles".into(),
                    label: self.t("group_detail.field_visibility_label"),
                    items: self.fields.clone(),
                    a11y: Some(A11y {
                        label: Some(self.t("contact_visibility.field_visibility_options_a11y")),
                        hint: Some(self.t("contact_detail.select_items_hint")),
                        role: None,
                    }),
                },
            ],
            contextual_actions: vec![ScreenAction {
                id: "save".into(),
                label: self.t("action.save"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for ContactVisibilityEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ItemToggled {
                component_id: _,
                item_id,
            } => {
                if let Some(item) = self.fields.iter_mut().find(|f| f.id == item_id) {
                    item.selected = !item.selected;
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "save" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::ContactVisibility {
            toggles: self
                .fields
                .iter()
                .map(|f| (f.id.clone(), f.selected))
                .collect(),
        })
    }
}
