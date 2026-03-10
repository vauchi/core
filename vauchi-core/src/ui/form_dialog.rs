// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generic form dialog engine — handles AddField, EditField, EditName, EditRelayUrl.

use crate::ui::*;

/// The type of form dialog, determining which fields are shown.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FormDialogType {
    AddField,
    EditField {
        field_id: String,
        field_label: String,
    },
    EditName {
        current_name: String,
    },
    EditRelayUrl {
        current_url: String,
    },
}

/// Available entry types for the Add Entry type selector.
const ENTRY_TYPES: &[(&str, &str, &str)] = &[
    ("phone", "Phone", "+1 555 123 4567"),
    ("email", "Email", "name@example.com"),
    ("social", "Social", "@handle or profile URL"),
    ("address", "Address", "123 Main St, City"),
    ("website", "Website", "https://example.com"),
    ("birthday", "Birthday", "YYYY-MM-DD"),
    ("custom", "Other", "Any custom info"),
];

/// Engine that manages a generic form dialog with text inputs.
#[derive(Clone, Debug)]
pub struct FormDialogEngine {
    dialog_type: FormDialogType,
    values: Vec<(String, String)>, // (component_id, current_value)
    /// For AddField: which entry type was selected (None = type picker step)
    selected_entry_type: Option<String>,
}

impl FormDialogEngine {
    pub fn new(dialog_type: FormDialogType) -> Self {
        let values = match &dialog_type {
            FormDialogType::AddField => vec![
                ("field_note".into(), String::new()),
                ("field_value".into(), String::new()),
            ],
            FormDialogType::EditField { .. } => {
                vec![("field_value".into(), String::new())]
            }
            FormDialogType::EditName { current_name } => {
                vec![("display_name".into(), current_name.clone())]
            }
            FormDialogType::EditRelayUrl { current_url } => {
                vec![("relay_url".into(), current_url.clone())]
            }
        };
        Self {
            dialog_type,
            values,
            selected_entry_type: None,
        }
    }

    fn get_value(&self, id: &str) -> &str {
        self.values
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }

    fn set_value(&mut self, id: &str, value: String) {
        if let Some(entry) = self.values.iter_mut().find(|(k, _)| k == id) {
            entry.1 = value;
        }
    }

    fn build_add_field_type_picker(&self) -> ScreenModel {
        let items: Vec<ActionListItem> = ENTRY_TYPES
            .iter()
            .map(|(id, label, hint)| ActionListItem {
                id: (*id).into(),
                label: (*label).into(),
                icon: None,
                detail: Some((*hint).into()),
            })
            .collect();

        ScreenModel {
            screen_id: "form_add_field_type".into(),
            title: "Add Entry".into(),
            subtitle: Some("What type of information?".into()),
            components: vec![Component::ActionList {
                id: "entry_types".into(),
                items,
            }],
            actions: vec![ScreenAction {
                id: "cancel".into(),
                label: "Cancel".into(),
                style: ActionStyle::Secondary,
                enabled: true,
            }],
            progress: None,
        }
    }

    fn build_add_field_value(&self) -> ScreenModel {
        let type_label = self
            .selected_entry_type
            .as_ref()
            .and_then(|t| ENTRY_TYPES.iter().find(|(id, _, _)| *id == t.as_str()))
            .map(|(_, label, _)| *label)
            .unwrap_or("Entry");

        let placeholder = self
            .selected_entry_type
            .as_ref()
            .and_then(|t| ENTRY_TYPES.iter().find(|(id, _, _)| *id == t.as_str()))
            .map(|(_, _, hint)| *hint)
            .unwrap_or("Enter value");

        let input_type = match self.selected_entry_type.as_deref() {
            Some("phone") => InputType::Phone,
            Some("email") => InputType::Email,
            _ => InputType::Text,
        };

        ScreenModel {
            screen_id: "form_add_field".into(),
            title: format!("Add {type_label}"),
            subtitle: None,
            components: vec![
                Component::TextInput {
                    id: "field_value".into(),
                    label: type_label.into(),
                    value: self.get_value("field_value").into(),
                    placeholder: Some(placeholder.into()),
                    max_length: Some(200),
                    validation_error: None,
                    input_type,
                },
                Component::TextInput {
                    id: "field_note".into(),
                    label: "Note (for yourself)".into(),
                    value: self.get_value("field_note").into(),
                    placeholder: Some("e.g. work phone, personal email".into()),
                    max_length: Some(50),
                    validation_error: None,
                    input_type: InputType::Text,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: "Save".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.dialog_type {
            FormDialogType::AddField => {
                if self.selected_entry_type.is_none() {
                    return self.build_add_field_type_picker();
                }
                self.build_add_field_value()
            }
            FormDialogType::EditField { field_label, .. } => ScreenModel {
                screen_id: "form_edit_field".into(),
                title: "Edit Field".into(),
                subtitle: None,
                components: vec![
                    Component::Text {
                        id: "field_info".into(),
                        content: format!("Editing: {field_label}"),
                        style: TextStyle::Subtitle,
                    },
                    Component::TextInput {
                        id: "field_value".into(),
                        label: "Value".into(),
                        value: self.get_value("field_value").into(),
                        placeholder: Some("Enter new value".into()),
                        max_length: Some(200),
                        validation_error: None,
                        input_type: InputType::Text,
                    },
                ],
                actions: vec![
                    ScreenAction {
                        id: "submit".into(),
                        label: "Save".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    },
                ],
                progress: None,
            },
            FormDialogType::EditName { .. } => ScreenModel {
                screen_id: "form_edit_name".into(),
                title: "Edit Display Name".into(),
                subtitle: None,
                components: vec![Component::TextInput {
                    id: "display_name".into(),
                    label: "Display Name".into(),
                    value: self.get_value("display_name").into(),
                    placeholder: Some("Your name".into()),
                    max_length: Some(50),
                    validation_error: None,
                    input_type: InputType::Text,
                }],
                actions: vec![
                    ScreenAction {
                        id: "submit".into(),
                        label: "Save".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    },
                ],
                progress: None,
            },
            FormDialogType::EditRelayUrl { .. } => ScreenModel {
                screen_id: "form_edit_relay_url".into(),
                title: "Edit Relay URL".into(),
                subtitle: None,
                components: vec![Component::TextInput {
                    id: "relay_url".into(),
                    label: "Relay URL".into(),
                    value: self.get_value("relay_url").into(),
                    placeholder: Some("wss://relay.example.com".into()),
                    max_length: Some(200),
                    validation_error: None,
                    input_type: InputType::Text,
                }],
                actions: vec![
                    ScreenAction {
                        id: "submit".into(),
                        label: "Save".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    },
                ],
                progress: None,
            },
        }
    }
}

impl WorkflowEngine for FormDialogEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } => {
                self.set_value(&component_id, value);
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ListItemSelected { item_id, .. } => {
                // Entry type selected in AddField type picker
                if self.dialog_type == FormDialogType::AddField
                    && self.selected_entry_type.is_none()
                {
                    self.selected_entry_type = Some(item_id);
                    return ActionResult::UpdateScreen(self.build_screen());
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                s if s == "submit" || s.starts_with("submit_") => ActionResult::Complete,
                "cancel" => {
                    // In AddField value step, go back to type picker
                    if self.dialog_type == FormDialogType::AddField
                        && self.selected_entry_type.is_some()
                    {
                        self.selected_entry_type = None;
                        self.set_value("field_value", String::new());
                        self.set_value("field_note", String::new());
                        return ActionResult::UpdateScreen(self.build_screen());
                    }
                    ActionResult::NavigateTo(self.build_screen())
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn collected_input(&self) -> Option<String> {
        // Return the primary input value for the form
        match &self.dialog_type {
            FormDialogType::AddField => {
                let entry_type = self.selected_entry_type.as_deref().unwrap_or("custom");
                let note = self.get_value("field_note");
                let value = self.get_value("field_value");
                // Format: type\nnote\nvalue
                Some(format!("{entry_type}\n{note}\n{value}"))
            }
            FormDialogType::EditField { .. } => Some(self.get_value("field_value").to_string()),
            FormDialogType::EditName { .. } => Some(self.get_value("display_name").to_string()),
            FormDialogType::EditRelayUrl { .. } => Some(self.get_value("relay_url").to_string()),
        }
    }
}
