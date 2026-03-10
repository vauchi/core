// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generic form dialog engine — handles AddField, EditField, EditName, EditRelayUrl.

use crate::contact_card::{CatalogEntry, FieldCategory, FieldTypeCatalog};
use crate::social::SocialNetworkRegistry;
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

/// Returns a placeholder hint for a given catalog entry key.
fn placeholder_for_key(key: &str) -> &'static str {
    match key {
        "phone" => "+1 555 123 4567",
        "email" => "name@example.com",
        "address" => "123 Main St, City",
        "website" => "https://example.com",
        "birthday" => "YYYY-MM-DD",
        "custom" => "Any custom info",
        k if k.starts_with("social:") => "@handle or profile URL",
        _ => "Enter value",
    }
}

/// Engine that manages a generic form dialog with text inputs.
#[derive(Clone, Debug)]
pub struct FormDialogEngine {
    dialog_type: FormDialogType,
    values: Vec<(String, String)>, // (component_id, current_value)
    /// For AddField: which entry type was selected (None = type picker step)
    selected_entry_type: Option<String>,
    /// For AddField: optional category filter (None = show all categories)
    selected_category: Option<FieldCategory>,
    /// Cached catalog entries for the type picker.
    catalog_entries: Vec<CatalogEntry>,
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
        let catalog_entries = if dialog_type == FormDialogType::AddField {
            let registry = SocialNetworkRegistry::new();
            let catalog = FieldTypeCatalog::new(&registry);
            catalog.all().to_vec()
        } else {
            Vec::new()
        };
        Self {
            dialog_type,
            values,
            selected_entry_type: None,
            selected_category: None,
            catalog_entries,
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
        // If a category is selected, show entries in that category
        if let Some(category) = &self.selected_category {
            let items: Vec<ActionListItem> = self
                .catalog_entries
                .iter()
                .filter(|e| &e.category == category)
                .map(|e| ActionListItem {
                    id: e.key.clone(),
                    label: e.display_name.clone(),
                    icon: e.icon.clone(),
                    detail: Some(placeholder_for_key(&e.key).into()),
                })
                .collect();

            return ScreenModel {
                screen_id: "form_add_field_type".into(),
                title: "Add Entry".into(),
                subtitle: Some(format!("{} types", category.display_name())),
                components: vec![Component::ActionList {
                    id: "entry_types".into(),
                    items,
                }],
                actions: vec![
                    ScreenAction {
                        id: "back_to_categories".into(),
                        label: "Back".into(),
                        style: ActionStyle::Secondary,
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
            };
        }

        // Show category picker
        let items: Vec<ActionListItem> = FieldCategory::all()
            .iter()
            .map(|cat| {
                let count = self
                    .catalog_entries
                    .iter()
                    .filter(|e| &e.category == cat)
                    .count();
                ActionListItem {
                    id: cat.display_name().to_lowercase(),
                    label: cat.display_name().into(),
                    icon: None,
                    detail: Some(format!("{count} types")),
                }
            })
            .collect();

        ScreenModel {
            screen_id: "form_add_field_type".into(),
            title: "Add Entry".into(),
            subtitle: Some("What type of information?".into()),
            components: vec![Component::ActionList {
                id: "entry_categories".into(),
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
        let selected_key = self.selected_entry_type.as_deref().unwrap_or("custom");
        let catalog_entry = self.catalog_entries.iter().find(|e| e.key == selected_key);
        let type_label = catalog_entry
            .map(|e| e.display_name.as_str())
            .unwrap_or("Entry");
        let placeholder = placeholder_for_key(selected_key);

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
                if self.dialog_type == FormDialogType::AddField
                    && self.selected_entry_type.is_none()
                {
                    if self.selected_category.is_none() {
                        // Category selected — filter by it
                        let category = match item_id.as_str() {
                            "contact" => Some(FieldCategory::Contact),
                            "social" => Some(FieldCategory::Social),
                            "personal" => Some(FieldCategory::Personal),
                            "custom" => Some(FieldCategory::Custom),
                            _ => None,
                        };
                        if let Some(cat) = category {
                            self.selected_category = Some(cat);
                            return ActionResult::UpdateScreen(self.build_screen());
                        }
                    } else {
                        // Entry type selected within a category
                        self.selected_entry_type = Some(item_id);
                        return ActionResult::UpdateScreen(self.build_screen());
                    }
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                s if s == "submit" || s.starts_with("submit_") => ActionResult::Complete,
                "back_to_categories" => {
                    // Go back from type list to category picker
                    self.selected_category = None;
                    ActionResult::UpdateScreen(self.build_screen())
                }
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
                    // In AddField category step, go back to categories
                    if self.dialog_type == FormDialogType::AddField
                        && self.selected_category.is_some()
                    {
                        self.selected_category = None;
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
