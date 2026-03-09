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

/// Engine that manages a generic form dialog with text inputs.
#[derive(Clone, Debug)]
pub struct FormDialogEngine {
    dialog_type: FormDialogType,
    values: Vec<(String, String)>, // (component_id, current_value)
}

impl FormDialogEngine {
    pub fn new(dialog_type: FormDialogType) -> Self {
        let values = match &dialog_type {
            FormDialogType::AddField => vec![
                ("field_label".into(), String::new()),
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

    fn build_screen(&self) -> ScreenModel {
        let (screen_id, title, components) = match &self.dialog_type {
            FormDialogType::AddField => (
                "form_add_field",
                "Add Field",
                vec![
                    Component::TextInput {
                        id: "field_label".into(),
                        label: "Label".into(),
                        value: self.get_value("field_label").into(),
                        placeholder: Some("e.g. Phone, Email".into()),
                        max_length: Some(50),
                        validation_error: None,
                        input_type: InputType::Text,
                    },
                    Component::TextInput {
                        id: "field_value".into(),
                        label: "Value".into(),
                        value: self.get_value("field_value").into(),
                        placeholder: Some("Enter value".into()),
                        max_length: Some(200),
                        validation_error: None,
                        input_type: InputType::Text,
                    },
                ],
            ),
            FormDialogType::EditField { field_label, .. } => (
                "form_edit_field",
                "Edit Field",
                vec![
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
            ),
            FormDialogType::EditName { .. } => (
                "form_edit_name",
                "Edit Display Name",
                vec![Component::TextInput {
                    id: "display_name".into(),
                    label: "Display Name".into(),
                    value: self.get_value("display_name").into(),
                    placeholder: Some("Your name".into()),
                    max_length: Some(50),
                    validation_error: None,
                    input_type: InputType::Text,
                }],
            ),
            FormDialogType::EditRelayUrl { .. } => (
                "form_edit_relay_url",
                "Edit Relay URL",
                vec![Component::TextInput {
                    id: "relay_url".into(),
                    label: "Relay URL".into(),
                    value: self.get_value("relay_url").into(),
                    placeholder: Some("wss://relay.example.com".into()),
                    max_length: Some(200),
                    validation_error: None,
                    input_type: InputType::Text,
                }],
            ),
        };

        ScreenModel {
            screen_id: screen_id.into(),
            title: title.into(),
            subtitle: None,
            components,
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
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "submit" => ActionResult::Complete,
                "cancel" => ActionResult::NavigateTo(self.build_screen()),
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn collected_input(&self) -> Option<String> {
        // Return the primary input value for the form
        match &self.dialog_type {
            FormDialogType::AddField => {
                let label = self.get_value("field_label");
                let value = self.get_value("field_value");
                Some(format!("{label}\n{value}"))
            }
            FormDialogType::EditField { .. } => Some(self.get_value("field_value").to_string()),
            FormDialogType::EditName { .. } => Some(self.get_value("display_name").to_string()),
            FormDialogType::EditRelayUrl { .. } => Some(self.get_value("relay_url").to_string()),
        }
    }
}
