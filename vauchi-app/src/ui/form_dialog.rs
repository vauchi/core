// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generic form dialog engine — handles AddField, EditField, EditName, EditRelayUrl.

use serde::{Deserialize, Serialize};

use crate::ui::*;
use vauchi_core::contact_card::{CatalogEntry, FieldTypeCatalog};
use vauchi_core::social::SocialNetworkRegistry;

/// The type of form dialog, determining which fields are shown.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FormDialogType {
    AddField {
        /// Available groups for visibility selection: (group_id, group_name).
        available_groups: Vec<(String, String)>,
    },
    EditField {
        field_id: String,
        field_label: String,
        current_value: String,
        /// Current private note, if any.
        current_note: Option<String>,
    },
    EditName {
        current_name: String,
    },
    EditRelayUrl {
        current_url: String,
    },
    CreateGroup,
    RenameGroup {
        group_id: String,
        current_name: String,
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
    /// For AddField: which entry type was selected (None = first type in list)
    selected_entry_type: Option<String>,
    /// Cached catalog entries for the type picker.
    catalog_entries: Vec<CatalogEntry>,
    /// For AddField: which groups are selected for visibility.
    selected_groups: Vec<String>,
    /// Set to true when the user presses cancel. `handle_completion` checks this
    /// to skip persistence and just navigate back.
    cancelled: bool,
    /// When true, an InlineConfirm is shown asking the user to confirm discarding
    /// unsaved changes. Set on cancel when form is dirty.
    pending_discard: bool,
}

impl FormDialogEngine {
    pub fn new(dialog_type: FormDialogType) -> Self {
        let values = match &dialog_type {
            FormDialogType::AddField { .. } => vec![
                ("field_value".into(), String::new()),
                ("field_label".into(), String::new()),
                ("field_note".into(), String::new()),
            ],
            FormDialogType::EditField {
                current_value,
                current_note,
                ..
            } => {
                vec![
                    ("field_value".into(), current_value.clone()),
                    (
                        "field_note".into(),
                        current_note.clone().unwrap_or_default(),
                    ),
                ]
            }
            FormDialogType::EditName { current_name } => {
                vec![("display_name".into(), current_name.clone())]
            }
            FormDialogType::EditRelayUrl { current_url } => {
                vec![("relay_url".into(), current_url.clone())]
            }
            FormDialogType::CreateGroup => {
                vec![("group_name".into(), String::new())]
            }
            FormDialogType::RenameGroup { current_name, .. } => {
                vec![("group_name".into(), current_name.clone())]
            }
        };
        let catalog_entries = if matches!(dialog_type, FormDialogType::AddField { .. }) {
            let registry = SocialNetworkRegistry::with_defaults();
            let catalog = FieldTypeCatalog::new(&registry);
            catalog.all().to_vec()
        } else {
            Vec::new()
        };
        Self {
            dialog_type,
            values,
            selected_entry_type: None,
            catalog_entries,
            selected_groups: Vec::new(),
            cancelled: false,
            pending_discard: false,
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

    /// Returns true if the form has user-entered data that differs from the
    /// original values. Used to decide whether to show a discard confirmation.
    fn is_dirty(&self) -> bool {
        match &self.dialog_type {
            FormDialogType::AddField { .. } => {
                let label = self.get_value("field_label").trim();
                let value = self.get_value("field_value").trim();
                !label.is_empty() || !value.is_empty()
            }
            FormDialogType::EditField {
                current_value,
                current_note,
                ..
            } => {
                let value = self.get_value("field_value");
                let note = self.get_value("field_note");
                value != current_value.as_str() || note != current_note.as_deref().unwrap_or("")
            }
            FormDialogType::EditName { current_name } => {
                self.get_value("display_name") != current_name.as_str()
            }
            FormDialogType::EditRelayUrl { current_url } => {
                self.get_value("relay_url") != current_url.as_str()
            }
            FormDialogType::CreateGroup => !self.get_value("group_name").is_empty(),
            FormDialogType::RenameGroup { current_name, .. } => {
                self.get_value("group_name") != current_name.as_str()
            }
        }
    }

    fn available_groups(&self) -> &[(String, String)] {
        match &self.dialog_type {
            FormDialogType::AddField {
                available_groups, ..
            } => available_groups,
            _ => &[],
        }
    }

    /// Build single-page AddField screen matching the design target:
    /// type list + value + display name + comment + groups visibility.
    fn build_add_field_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Type list (flat, all entries from all categories)
        let type_items: Vec<ActionListItem> = self
            .catalog_entries
            .iter()
            .map(|e| ActionListItem {
                id: e.key.clone(),
                label: e.display_name.clone(),
                icon: e.icon.clone(),
                detail: if self.selected_entry_type.as_deref() == Some(&e.key) {
                    Some("selected".into())
                } else {
                    None
                },
                a11y: Some(A11y {
                    label: Some(e.display_name.clone()),
                    hint: Some(format!("Select {} as the field type.", e.display_name)),
                    role: None,
                }),
                info_key: None,
            })
            .collect();

        components.push(Component::ActionList {
            id: "entry_types".into(),
            items: type_items,
        });

        // Value input
        let selected_key = self.selected_entry_type.as_deref().unwrap_or("custom");
        let catalog_entry = self.catalog_entries.iter().find(|e| e.key == selected_key);
        let placeholder = placeholder_for_key(selected_key);

        let input_type = match selected_key {
            "phone" => InputType::Phone,
            "email" => InputType::Email,
            _ => InputType::Text,
        };

        components.push(Component::TextInput {
            id: "field_value".into(),
            label: "Value".into(),
            value: self.get_value("field_value").into(),
            placeholder: Some(placeholder.into()),
            max_length: Some(200),
            validation_error: None,
            input_type,
            a11y: Some(A11y {
                label: Some("Value input".into()),
                hint: Some(placeholder.into()),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        });

        // Display Name (optional) — the label shown next to the value
        components.push(Component::TextInput {
            id: "field_label".into(),
            label: "Display Name (optional)".into(),
            value: self.get_value("field_label").into(),
            placeholder: Some("e.g. Work, Personal, Mobile".into()),
            max_length: Some(50),
            validation_error: None,
            input_type: InputType::Text,
            a11y: Some(A11y {
                label: Some("Display Name (optional) input".into()),
                hint: Some("e.g. Work, Personal, Mobile".into()),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        });

        // Comment (your eyes only, optional) — private note
        components.push(Component::TextInput {
            id: "field_note".into(),
            label: "Comment (your eyes only, optional)".into(),
            value: self.get_value("field_note").into(),
            placeholder: Some("Only visible to you".into()),
            max_length: Some(100),
            validation_error: None,
            input_type: InputType::Text,
            a11y: Some(A11y {
                label: Some("Comment (your eyes only, optional) input".into()),
                hint: Some("Only visible to you".into()),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        });

        // Group visibility toggles
        let groups = self.available_groups();
        if !groups.is_empty() {
            let toggle_items: Vec<ToggleItem> = groups
                .iter()
                .map(|(gid, gname)| ToggleItem {
                    id: gid.clone(),
                    label: gname.clone(),
                    selected: self.selected_groups.contains(gid),
                    subtitle: None,
                    a11y: None,
                    info_key: None,
                })
                .collect();

            components.push(Component::ToggleList {
                id: "group_visibility".into(),
                label: "Groups Visibility".into(),
                items: toggle_items,
                a11y: None,
            });
        }

        // User-facing copy — `MyInfo` is the internal CoreScreenView
        // name and was leaking into the title verbatim. See problem
        // record `2026-05-21-add-entry-form-mixes-picker-and-fields`.
        let title = if self.selected_entry_type.is_some() {
            if let Some(entry) = catalog_entry {
                format!("Add {}", entry.display_name)
            } else {
                "Add to your card".into()
            }
        } else {
            "Add to your card".into()
        };

        // Save is only meaningful once the user has picked a type
        // **and** typed a value. Without these guards Save was
        // enabled from first render and tapping it with no input
        // silently committed an empty `custom` entry — same record §G3.
        let save_enabled =
            self.selected_entry_type.is_some() && !self.get_value("field_value").is_empty();

        ScreenModel {
            screen_id: "form_add_field".into(),
            title,
            subtitle: Some("Select a type".into()),
            components,
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: "Save".into(),
                    style: ActionStyle::Primary,
                    enabled: save_enabled,
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.dialog_type {
            FormDialogType::AddField { .. } => self.build_add_field_screen(),
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
                        a11y: Some(A11y {
                            label: Some("Value input".into()),
                            hint: Some("Enter new value".into()),
                            role: Some(AccessibilityRole::TextField),
                        }),
                        info_key: None,
                    },
                    Component::TextInput {
                        id: "field_note".into(),
                        label: "Comment (your eyes only, optional)".into(),
                        value: self.get_value("field_note").into(),
                        placeholder: Some("Only visible to you".into()),
                        max_length: Some(100),
                        validation_error: None,
                        input_type: InputType::Text,
                        a11y: Some(A11y {
                            label: Some("Comment (your eyes only, optional) input".into()),
                            hint: Some("Only visible to you".into()),
                            role: Some(AccessibilityRole::TextField),
                        }),
                        info_key: None,
                    },
                ],
                actions: vec![
                    ScreenAction {
                        id: "submit".into(),
                        label: "Save".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: None,
                ..Default::default()
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
                    a11y: Some(A11y {
                        label: Some("Display Name input".into()),
                        hint: Some("Your name".into()),
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                }],
                actions: vec![
                    ScreenAction {
                        id: "submit".into(),
                        label: "Save".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: None,
                ..Default::default()
            },
            FormDialogType::EditRelayUrl { .. } => ScreenModel {
                screen_id: "form_edit_relay_url".into(),
                title: "Edit Relay URL".into(),
                subtitle: None,
                components: vec![Component::TextInput {
                    id: "relay_url".into(),
                    label: "Relay URL".into(),
                    value: self.get_value("relay_url").into(),
                    placeholder: Some("https://relay.example.com".into()),
                    max_length: Some(200),
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some("Relay URL input".into()),
                        hint: Some("https://relay.example.com".into()),
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                }],
                actions: vec![
                    ScreenAction {
                        id: "submit".into(),
                        label: "Save".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: None,
                ..Default::default()
            },
            FormDialogType::CreateGroup => {
                self.build_group_name_screen("form_create_group", "New Group", "Create")
            }
            FormDialogType::RenameGroup { .. } => {
                self.build_group_name_screen("form_rename_group", "Rename Group", "Rename")
            }
        }
    }

    fn build_group_name_screen(
        &self,
        screen_id: &str,
        title: &str,
        submit_label: &str,
    ) -> ScreenModel {
        let name = self.get_value("group_name");
        ScreenModel {
            screen_id: screen_id.into(),
            title: title.into(),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "group_name".into(),
                label: "Group Name".into(),
                value: name.into(),
                placeholder: Some("e.g. Family, Work, Friends".into()),
                max_length: Some(50),
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some("Group Name input".into()),
                    hint: Some("e.g. Family, Work, Friends".into()),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: submit_label.into(),
                    style: ActionStyle::Primary,
                    enabled: !name.is_empty(),
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for FormDialogEngine {
    fn current_screen(&self) -> ScreenModel {
        let mut screen = self.build_screen();
        if self.pending_discard {
            screen.components.push(Component::InlineConfirm {
                id: "discard".into(),
                warning: "You have unsaved changes. Discard?".into(),
                confirm_text: "Discard".into(),
                cancel_text: "Keep Editing".into(),
                destructive: false,
                a11y: Some(A11y {
                    label: Some("Confirm discard changes".into()),
                    hint: Some("You have unsaved changes. Confirming will discard them.".into()),
                    role: Some(AccessibilityRole::Alert),
                }),
            });
        }
        screen
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } => {
                self.set_value(&component_id, value);
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected { item_id, .. } => {
                if matches!(self.dialog_type, FormDialogType::AddField { .. }) {
                    // Type selected from flat list
                    self.selected_entry_type = Some(item_id);
                    return ActionResult::UpdateScreen(self.current_screen());
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "group_visibility" => {
                // Toggle group selection
                if let Some(pos) = self.selected_groups.iter().position(|g| g == &item_id) {
                    self.selected_groups.remove(pos);
                } else {
                    self.selected_groups.push(item_id);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                s if s == "submit" || s.starts_with("submit_") => ActionResult::Complete,
                "cancel" => {
                    if self.pending_discard {
                        // Second cancel while InlineConfirm shown → dismiss it
                        self.pending_discard = false;
                        ActionResult::UpdateScreen(self.current_screen())
                    } else if self.is_dirty() {
                        // Dirty form → show InlineConfirm before discarding
                        self.pending_discard = true;
                        ActionResult::UpdateScreen(self.current_screen())
                    } else {
                        // Clean form → cancel immediately
                        self.cancelled = true;
                        ActionResult::Complete
                    }
                }
                "confirm_discard" => {
                    self.pending_discard = false;
                    self.cancelled = true;
                    ActionResult::Complete
                }
                "cancel_discard" => {
                    self.pending_discard = false;
                    ActionResult::UpdateScreen(self.current_screen())
                }
                _ => ActionResult::UpdateScreen(self.current_screen()),
            },
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn collected_input(&self) -> Option<String> {
        // Return the primary input value for the form
        match &self.dialog_type {
            FormDialogType::AddField { .. } => {
                let entry_type = self.selected_entry_type.as_deref().unwrap_or("custom");
                let label = self.get_value("field_label");
                let value = self.get_value("field_value");
                let note = self.get_value("field_note");
                let groups = self.selected_groups.join(",");
                // Format: type\nlabel\nvalue\nnote\ngroups
                Some(format!("{entry_type}\n{label}\n{value}\n{note}\n{groups}"))
            }
            FormDialogType::EditField { .. } => {
                let value = self.get_value("field_value");
                let note = self.get_value("field_note");
                // Format: value\nnote
                Some(format!("{value}\n{note}"))
            }
            FormDialogType::EditName { .. } => Some(self.get_value("display_name").to_string()),
            FormDialogType::EditRelayUrl { .. } => Some(self.get_value("relay_url").to_string()),
            FormDialogType::CreateGroup | FormDialogType::RenameGroup { .. } => {
                Some(self.get_value("group_name").to_string())
            }
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}
