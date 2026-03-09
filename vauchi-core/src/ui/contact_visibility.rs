// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact visibility engine — per-field visibility toggles for a specific contact.

use crate::ui::*;

/// Engine that displays per-field visibility toggles for a contact.
#[derive(Clone, Debug)]
pub struct ContactVisibilityEngine {
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
                },
            ],
            actions: vec![ScreenAction {
                id: "save".into(),
                label: "Save".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }],
            progress: None,
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
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
