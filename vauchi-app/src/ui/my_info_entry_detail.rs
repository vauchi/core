// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry detail screen — edit value, modify group visibility, see which contacts can see it.

use crate::ui::*;

/// Info about a contact that can see this entry (for the read-only contact list).
#[derive(Clone, Debug)]
pub struct EntryContactInfo {
    pub contact_id: String,
    pub name: String,
    pub via_group: String,
}

/// Engine for the MyInfo entry detail screen.
#[derive(Clone, Debug)]
pub struct MyInfoEntryDetailEngine {
    pub field_id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
    /// Private per-field note (never shared).
    pub note: Option<String>,
    /// All groups with their visibility state for this field.
    pub groups: Vec<(String, String, bool)>, // (group_id, group_name, is_visible)
    /// Contacts who can see this field (derived from group membership).
    pub visible_contacts: Vec<EntryContactInfo>,
}

impl MyInfoEntryDetailEngine {
    pub fn new(
        field_id: String,
        field_type: String,
        label: String,
        value: String,
        note: Option<String>,
        groups: Vec<(String, String, bool)>,
        visible_contacts: Vec<EntryContactInfo>,
    ) -> Self {
        Self {
            field_id,
            field_type,
            label,
            value,
            note,
            groups,
            visible_contacts,
        }
    }
}

impl MyInfoEntryDetailEngine {
    /// Rebuild current_screen after external mutation of fields.
    pub fn refresh_screen(&self) -> ScreenModel {
        self.current_screen()
    }
}

impl WorkflowEngine for MyInfoEntryDetailEngine {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn current_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Field info
        components.push(Component::Text {
            id: "field_info".into(),
            content: format!("{} ({})", self.value, self.label),
            style: TextStyle::Title,
            accessible_label: None,
            accessible_hint: None,
        });

        components.push(Component::Divider);

        // Group visibility toggles
        if !self.groups.is_empty() {
            let toggle_items: Vec<ToggleItem> = self
                .groups
                .iter()
                .map(|(gid, gname, visible)| ToggleItem {
                    id: gid.clone(),
                    label: gname.clone(),
                    selected: *visible,
                    subtitle: None,
                    accessible_label: None,
                    accessible_hint: None,
                })
                .collect();

            components.push(Component::ToggleList {
                id: "group_visibility".into(),
                label: "Visible to groups".into(),
                items: toggle_items,
                accessible_label: None,
                accessible_hint: None,
            });
        }

        components.push(Component::Divider);

        // Contacts who can see this entry (read-only list)
        if self.visible_contacts.is_empty() {
            components.push(Component::Text {
                id: "no_contacts".into(),
                content: "No contacts can see this entry.".into(),
                style: TextStyle::Caption,
                accessible_label: None,
                accessible_hint: None,
            });
        } else {
            components.push(Component::Text {
                id: "contacts_header".into(),
                content: format!("Visible to {} contacts", self.visible_contacts.len()),
                style: TextStyle::Subtitle,
                accessible_label: None,
                accessible_hint: None,
            });

            let contact_items: Vec<ActionListItem> = self
                .visible_contacts
                .iter()
                .map(|c| ActionListItem {
                    id: c.contact_id.clone(),
                    label: c.name.clone(),
                    icon: None,
                    detail: Some(format!("via {}", c.via_group)),
                    accessible_label: None,
                    accessible_hint: None,
                })
                .collect();

            components.push(Component::ActionList {
                id: "visible_contacts".into(),
                items: contact_items,
                accessible_label: None,
                accessible_hint: None,
            });
        }

        ScreenModel {
            screen_id: "my_info_entry_detail".into(),
            title: self.label.clone(),
            subtitle: Some(self.field_type.clone()),
            components,
            actions: vec![
                ScreenAction {
                    id: "edit".into(),
                    label: "Edit".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "delete".into(),
                    label: "Delete".into(),
                    style: ActionStyle::Destructive,
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
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "group_visibility" => {
                // Toggle group visibility — return a signal so AppEngine can persist
                ActionResult::NavigateTo(self.current_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "edit" => ActionResult::NavigateTo(self.current_screen()),
                "delete" => ActionResult::Complete,
                "back" => ActionResult::NavigateTo(self.current_screen()),
                _ => ActionResult::UpdateScreen(self.current_screen()),
            },
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
