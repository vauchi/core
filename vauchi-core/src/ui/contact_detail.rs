// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact detail engine — read-only view of a single contact's card,
//! and a fallback engine for contacts not found.

use crate::ui::*;

/// Read-only engine that displays a single contact's details.
#[derive(Clone, Debug)]
pub struct ContactDetailEngine {
    contact: ContactItem,
    fields: Vec<FieldDisplay>,
}

impl ContactDetailEngine {
    pub fn new(contact: ContactItem, fields: Vec<FieldDisplay>) -> Self {
        Self { contact, fields }
    }
}

impl WorkflowEngine for ContactDetailEngine {
    fn current_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_detail".into(),
            title: self.contact.name.clone(),
            subtitle: self.contact.subtitle.clone(),
            components: vec![
                Component::InfoPanel {
                    id: "contact_info".into(),
                    icon: None,
                    title: self.contact.name.clone(),
                    items: vec![InfoItem {
                        icon: None,
                        title: "Initials".into(),
                        detail: self.contact.avatar_initials.clone(),
                    }],
                },
                Component::FieldList {
                    id: "fields".into(),
                    fields: self.fields.clone(),
                    visibility_mode: VisibilityMode::ShowHide,
                    available_groups: vec![],
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "edit".into(),
                    label: "Edit".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "edit" => {
                ActionResult::OpenContact {
                    contact_id: self.contact.id.clone(),
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

/// Fallback engine for when a contact is not found.
#[derive(Clone, Debug)]
pub struct ContactNotFoundEngine {
    contact_id: String,
}

impl ContactNotFoundEngine {
    pub fn new(contact_id: String) -> Self {
        Self { contact_id }
    }
}

impl WorkflowEngine for ContactNotFoundEngine {
    fn current_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_not_found".into(),
            title: "Contact Not Found".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "not_found".into(),
                icon: None,
                title: "Not Found".into(),
                items: vec![InfoItem {
                    icon: None,
                    title: "Error".into(),
                    detail: format!("Contact '{}' was not found.", self.contact_id),
                }],
            }],
            actions: vec![ScreenAction {
                id: "back".into(),
                label: "Back".into(),
                style: ActionStyle::Secondary,
                enabled: true,
            }],
            progress: None,
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
