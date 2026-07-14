// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Generic form dialog engine — handles AddField, EditField, EditName, EditRelayUrl.

use serde::{Deserialize, Serialize};

use crate::i18n::{Locale, get_string, get_string_with_args};
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
    locale: Locale,
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
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-7).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
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

    /// Build the AddField screen. Two-step flow per problem record
    /// `2026-05-21-add-entry-form-mixes-picker-and-fields` §G1:
    ///
    ///   step 1 (picker) — `selected_entry_type.is_none()`
    ///     ActionList of the catalog entries; Cancel only. The user
    ///     picks a type and the engine flips into the form step.
    ///
    ///   step 2 (form) — `selected_entry_type.is_some()`
    ///     Value / Display Name / Comment / group visibility inputs.
    ///     Save (gated on a non-empty value) + `change_type` (returns
    ///     to picker) + Cancel.
    fn build_add_field_screen(&self) -> ScreenModel {
        if self.selected_entry_type.is_none() {
            self.build_add_field_picker_step()
        } else {
            self.build_add_field_form_step()
        }
    }

    /// Picker step: catalog list only, no value inputs, no Save.
    fn build_add_field_picker_step(&self) -> ScreenModel {
        let type_items: Vec<ActionListItem> = self
            .catalog_entries
            .iter()
            .map(|e| ActionListItem {
                id: e.key.clone(),
                label: e.display_name.clone(),
                icon: e.icon.clone(),
                detail: None,
                a11y: Some(A11y {
                    label: Some(e.display_name.clone()),
                    hint: Some(get_string_with_args(
                        self.locale,
                        "form.field_type_select_hint",
                        &[("name", &e.display_name)],
                    )),
                    role: None,
                }),
                info_key: None,
            })
            .collect();

        let components = vec![Component::ActionList {
            id: "entry_types".into(),
            items: type_items,
        }];

        ScreenModel {
            screen_id: "form_add_field".into(),
            title: self.t("form.add_to_card_title"),
            subtitle: Some(self.t("form.select_type_subtitle")),
            components,
            actions: vec![ScreenAction {
                id: "cancel".into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    /// Form step: Value / Display Name / Comment / group visibility +
    /// Save (gated) / `change_type` (back to picker) / Cancel.
    fn build_add_field_form_step(&self) -> ScreenModel {
        let selected_key = self
            .selected_entry_type
            .as_deref()
            .expect("form step requires selected_entry_type");
        let catalog_entry = self.catalog_entries.iter().find(|e| e.key == selected_key);
        let placeholder = placeholder_for_key(selected_key);

        let input_type = match selected_key {
            "phone" => InputType::Phone,
            "email" => InputType::Email,
            _ => InputType::Text,
        };

        let mut components = self.add_field_inputs(placeholder, input_type);

        // Group visibility toggles (form step only — picking groups
        // before picking a type would be confusing).
        let groups = self.available_groups();
        if !groups.is_empty() {
            let toggle_items: Vec<ToggleItem> = groups
                .iter()
                .map(|(gid, gname)| ToggleItem {
                    id: gid.clone(),
                    label: gname.clone(),
                    selected: self.selected_groups.contains(gid),
                    subtitle: None,
                    a11y: Some(A11y::labeled(gname.clone())),
                    info_key: None,
                })
                .collect();
            components.push(Component::ToggleList {
                id: "group_visibility".into(),
                label: self.t("form.groups_visibility_label"),
                items: toggle_items,
                a11y: None,
            });
        }

        let title = if let Some(entry) = catalog_entry {
            get_string_with_args(
                self.locale,
                "form.add_entry_title",
                &[("name", &entry.display_name)],
            )
        } else {
            self.t("form.add_to_card_title")
        };

        // Save is gated on a non-empty value — same record §G3. The
        // type-selected check is now structural (we only build this
        // ScreenModel when `selected_entry_type.is_some()`).
        let save_enabled = !self.get_value("field_value").is_empty();

        ScreenModel {
            screen_id: "form_add_field".into(),
            title,
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: save_enabled,
                    a11y: Some(A11y::labeled(self.t("action.save"))),
                },
                ScreenAction {
                    id: "change_type".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y {
                        label: Some(self.t("form.pick_different_type_a11y")),
                        hint: Some(self.t("form.pick_different_type_hint")),
                        role: None,
                    }),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn add_field_inputs(&self, placeholder: &str, input_type: InputType) -> Vec<Component> {
        vec![
            Component::TextInput {
                id: "field_value".into(),
                label: self.t("card.value"),
                value: self.get_value("field_value").into(),
                placeholder: Some(placeholder.into()),
                max_length: Some(200),
                validation_error: None,
                input_type,
                a11y: Some(A11y {
                    label: Some(self.t("form.value_input_a11y")),
                    hint: Some(placeholder.into()),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            },
            Component::TextInput {
                id: "field_label".into(),
                label: self.t("form.display_name_optional_label"),
                value: self.get_value("field_label").into(),
                placeholder: Some(self.t("form.display_name_optional_placeholder")),
                max_length: Some(50),
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some(self.t("form.display_name_optional_a11y")),
                    hint: Some(self.t("form.display_name_optional_placeholder")),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            },
            Component::TextInput {
                id: "field_note".into(),
                label: self.t("form.comment_label"),
                value: self.get_value("field_note").into(),
                placeholder: Some(self.t("form.comment_placeholder")),
                max_length: Some(100),
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some(self.t("form.comment_a11y")),
                    hint: Some(self.t("form.comment_placeholder")),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            },
        ]
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.dialog_type {
            FormDialogType::AddField { .. } => self.build_add_field_screen(),
            FormDialogType::EditField { field_label, .. } => {
                self.build_edit_field_screen(field_label)
            }
            FormDialogType::EditName { .. } => self.build_edit_name_screen(),
            FormDialogType::EditRelayUrl { .. } => self.build_edit_relay_url_screen(),
            FormDialogType::CreateGroup => self.build_group_name_screen(
                "form_create_group",
                self.t("form.new_group_title"),
                self.t("form.create_button"),
            ),
            FormDialogType::RenameGroup { .. } => self.build_group_name_screen(
                "form_rename_group",
                self.t("form.rename_group_title"),
                self.t("form.rename_button"),
            ),
        }
    }

    fn build_edit_field_screen(&self, field_label: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "form_edit_field".into(),
            title: self.t("form.edit_field_title"),
            subtitle: None,
            components: vec![
                Component::Text {
                    id: "field_info".into(),
                    content: get_string_with_args(
                        self.locale,
                        "form.editing_field_content",
                        &[("label", field_label)],
                    ),
                    style: TextStyle::Subtitle,
                },
                Component::TextInput {
                    id: "field_value".into(),
                    label: self.t("card.value"),
                    value: self.get_value("field_value").into(),
                    placeholder: Some(self.t("form.enter_new_value_placeholder")),
                    max_length: Some(200),
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some(self.t("form.value_input_a11y")),
                        hint: Some(self.t("form.enter_new_value_placeholder")),
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                },
                Component::TextInput {
                    id: "field_note".into(),
                    label: self.t("form.comment_label"),
                    value: self.get_value("field_note").into(),
                    placeholder: Some(self.t("form.comment_placeholder")),
                    max_length: Some(100),
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some(self.t("form.comment_a11y")),
                        hint: Some(self.t("form.comment_placeholder")),
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.save"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_edit_name_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "form_edit_name".into(),
            title: self.t("form.edit_display_name_title"),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "display_name".into(),
                label: self.t("settings.display_name"),
                value: self.get_value("display_name").into(),
                placeholder: Some(self.t("form.your_name_placeholder")),
                max_length: Some(50),
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some(self.t("form.display_name_input_a11y")),
                    hint: Some(self.t("form.your_name_placeholder")),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.save"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_edit_relay_url_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "form_edit_relay_url".into(),
            title: self.t("form.edit_relay_url_title"),
            subtitle: None,
            components: vec![Component::TextInput {
                id: "relay_url".into(),
                label: self.t("settings.relay_url"),
                value: self.get_value("relay_url").into(),
                placeholder: Some(self.t("settings.relay_placeholder")),
                max_length: Some(200),
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some(self.t("form.relay_url_input_a11y")),
                    hint: Some(self.t("settings.relay_placeholder")),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.save"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_group_name_screen(
        &self,
        screen_id: &str,
        title: String,
        submit_label: String,
    ) -> ScreenModel {
        let name = self.get_value("group_name");
        ScreenModel {
            screen_id: screen_id.into(),
            title,
            subtitle: None,
            components: vec![Component::TextInput {
                id: "group_name".into(),
                label: self.t("form.group_name_label"),
                value: name.into(),
                placeholder: Some(self.t("form.group_name_placeholder")),
                max_length: Some(50),
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y {
                    label: Some(self.t("form.group_name_input_a11y")),
                    hint: Some(self.t("form.group_name_placeholder")),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: submit_label.clone(),
                    style: ActionStyle::Primary,
                    enabled: !name.is_empty(),
                    a11y: Some(A11y::labeled(submit_label)),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
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
                warning: self.t("form.discard_warning"),
                confirm_text: self.t("form.discard_button"),
                cancel_text: self.t("form.keep_editing_button"),
                confirm_action_id: "confirm_discard".into(),
                cancel_action_id: "cancel_discard".into(),
                destructive: false,
                a11y: Some(A11y {
                    label: Some(self.t("form.confirm_discard_a11y")),
                    hint: Some(self.t("form.confirm_discard_hint")),
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
                "change_type" => {
                    // G1 back-from-form: clear the selected type so
                    // `build_add_field_screen` flips back to the
                    // picker step. Field values persist so the user
                    // can pick a different type without re-typing.
                    self.selected_entry_type = None;
                    ActionResult::UpdateScreen(self.current_screen())
                }
                _ => ActionResult::UpdateScreen(self.current_screen()),
            },
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        use crate::ui::FormInput;
        let input = match &self.dialog_type {
            FormDialogType::AddField { .. } => FormInput::AddField {
                entry_type: self
                    .selected_entry_type
                    .clone()
                    .unwrap_or_else(|| "custom".into()),
                label: self.get_value("field_label").to_string(),
                value: self.get_value("field_value").to_string(),
                note: self.get_value("field_note").to_string(),
                groups: self.selected_groups.clone(),
            },
            FormDialogType::EditField { .. } => FormInput::EditField {
                value: self.get_value("field_value").to_string(),
                note: self.get_value("field_note").to_string(),
            },
            FormDialogType::EditName { .. } => FormInput::EditName {
                name: self.get_value("display_name").to_string(),
            },
            FormDialogType::EditRelayUrl { .. } => FormInput::EditRelayUrl {
                url: self.get_value("relay_url").to_string(),
            },
            FormDialogType::CreateGroup => FormInput::CreateGroup {
                name: self.get_value("group_name").to_string(),
            },
            FormDialogType::RenameGroup { .. } => FormInput::RenameGroup {
                name: self.get_value("group_name").to_string(),
            },
        };
        Some(crate::ui::EngineOutput::Form(input))
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}
