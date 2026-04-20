// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact visibility engine — per-field visibility toggles for a specific contact.

use crate::ui::*;

/// Engine that displays per-field visibility toggles for a contact.
#[derive(Clone, Debug)]
pub struct ContactVisibilityEngine {
    #[allow(dead_code)] // Used by future save logic (set_field_shown per contact)
    contact_id: String,
    contact_name: String,
    fields: Vec<ToggleItem>,
}

impl ContactVisibilityEngine {
    pub fn new(contact_id: String, contact_name: String, fields: Vec<ToggleItem>) -> Self {
        Self {
            contact_id,
            contact_name,
            fields,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_visibility".into(),
            title: format!("Visibility: {}", self.contact_name),
            subtitle: None,
            components: vec![
                Component::Text {
                    id: "visibility_info".into(),
                    content: "Toggle which fields are visible to this contact.".into(),
                    style: TextStyle::Body,
                },
                Component::ToggleList {
                    id: "field_toggles".into(),
                    label: "Field Visibility".into(),
                    items: self.fields.clone(),
                    a11y: Some(A11y {
                        label: Some("Field Visibility options".into()),
                        hint: Some("Select items to include".into()),
                        role: None,
                    }),
                },
            ],
            actions: vec![ScreenAction {
                id: "save".into(),
                label: "Save".into(),
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

    fn collected_input(&self) -> Option<String> {
        // Return visibility state as "field_id:visible,field_id:hidden,..."
        let parts: Vec<String> = self
            .fields
            .iter()
            .map(|f| {
                let state = if f.selected { "visible" } else { "hidden" };
                format!("{}:{}", f.id, state)
            })
            .collect();
        Some(parts.join(","))
    }
}
