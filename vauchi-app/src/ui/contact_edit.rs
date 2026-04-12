// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact-edit workflow engine — a pure state machine for the 3-step
//! contact editing flow (EditFields → EditVisibility → Preview).
//! No Storage or Vauchi dependency; the caller persists results when
//! [`ActionResult::Complete`] is returned.

use crate::ui::*;

// ── Public data types ───────────────────────────────────────────────

/// Data to edit — mirrors the contact card structure but in UI-friendly form.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EditableContact {
    pub display_name: String,
    pub fields: Vec<EditableField>,
}

/// A single editable field on a contact card.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EditableField {
    pub id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
    pub visible_to_groups: Vec<String>,
    pub shown: bool,
}

// ── ContactEditEngine ───────────────────────────────────────────────

/// Pure state-machine driving the 3-step contact edit flow.
#[derive(Clone, Debug)]
pub struct ContactEditEngine {
    step: ContactEditStep,
    contact: EditableContact,
    available_groups: Vec<String>,
    selected_preview_group: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum ContactEditStep {
    EditFields,
    EditVisibility,
    Preview,
}

impl ContactEditEngine {
    /// Creates a new engine pre-populated with the given contact data.
    pub fn new(contact: EditableContact, available_groups: Vec<String>) -> Self {
        Self {
            step: ContactEditStep::EditFields,
            contact,
            available_groups,
            selected_preview_group: None,
        }
    }

    /// Returns a reference to the edited contact data.
    pub fn edited_contact(&self) -> &EditableContact {
        &self.contact
    }

    // ── Private helpers ─────────────────────────────────────────────

    fn field_to_display(&self, field: &EditableField) -> FieldDisplay {
        let visibility = if !field.visible_to_groups.is_empty() {
            UiFieldVisibility::Groups(field.visible_to_groups.clone())
        } else if field.shown {
            UiFieldVisibility::Shown
        } else {
            UiFieldVisibility::Hidden
        };

        FieldDisplay {
            id: field.id.clone(),
            field_type: field.field_type.clone(),
            label: field.label.clone(),
            value: field.value.clone(),
            visibility,
            a11y: None,
        }
    }

    fn build_group_views(&self) -> Vec<GroupCardView> {
        self.available_groups
            .iter()
            .map(|group| {
                let visible_fields: Vec<FieldDisplay> = self
                    .contact
                    .fields
                    .iter()
                    .filter(|f| f.visible_to_groups.contains(group))
                    .map(|f| self.field_to_display(f))
                    .collect();

                GroupCardView {
                    group_name: group.clone(),
                    display_name: self.contact.display_name.clone(),
                    visible_fields,
                }
            })
            .collect()
    }

    fn build_edit_fields_screen(&self) -> ScreenModel {
        let fields: Vec<FieldDisplay> = self
            .contact
            .fields
            .iter()
            .map(|f| self.field_to_display(f))
            .collect();

        let name_not_empty = !self.contact.display_name.trim().is_empty();

        ScreenModel {
            screen_id: "edit_fields".into(),
            title: "Edit Contact".into(),
            subtitle: None,
            components: vec![
                Component::TextInput {
                    id: "display_name".into(),
                    label: "Display Name".into(),
                    value: self.contact.display_name.clone(),
                    placeholder: Some("Enter name".into()),
                    max_length: None,
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: None,
                },
                Component::Divider,
                Component::FieldList {
                    id: "fields".into(),
                    fields,
                    visibility_mode: VisibilityMode::ShowHide,
                    available_groups: self.available_groups.clone(),
                    a11y: None,
                },
            ],
            actions: vec![ScreenAction {
                id: "continue".into(),
                label: "Continue".into(),
                style: ActionStyle::Primary,
                enabled: name_not_empty,
            }],
            progress: Some(Progress {
                current_step: 1,
                total_steps: 3,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn build_edit_visibility_screen(&self) -> ScreenModel {
        let toggle_lists: Vec<Component> = self
            .contact
            .fields
            .iter()
            .map(|field| {
                let items: Vec<ToggleItem> = self
                    .available_groups
                    .iter()
                    .map(|group| ToggleItem {
                        id: group.clone(),
                        label: group.clone(),
                        selected: field.visible_to_groups.contains(group),
                        subtitle: None,
                        a11y: None,
                    })
                    .collect();

                Component::ToggleList {
                    id: format!("vis_{}", field.id),
                    label: field.label.clone(),
                    items,
                    a11y: None,
                }
            })
            .collect();

        ScreenModel {
            screen_id: "edit_visibility".into(),
            title: "Field Visibility".into(),
            subtitle: None,
            components: toggle_lists,
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "continue".into(),
                    label: "Preview".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
            ],
            progress: Some(Progress {
                current_step: 2,
                total_steps: 3,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn build_preview_screen(&self) -> ScreenModel {
        let fields: Vec<FieldDisplay> = self
            .contact
            .fields
            .iter()
            .map(|f| self.field_to_display(f))
            .collect();

        ScreenModel {
            screen_id: "edit_preview".into(),
            title: "Preview Card".into(),
            subtitle: None,
            components: vec![Component::CardPreview {
                name: self.contact.display_name.clone(),
                fields,
                group_views: self.build_group_views(),
                selected_group: self.selected_preview_group.clone(),
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "save".into(),
                    label: "Save Changes".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
            ],
            progress: Some(Progress {
                current_step: 3,
                total_steps: 3,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn toggle_group_for_field(&mut self, field_id: &str, group: &str) {
        if let Some(field) = self.contact.fields.iter_mut().find(|f| f.id == field_id) {
            if let Some(pos) = field.visible_to_groups.iter().position(|g| g == group) {
                field.visible_to_groups.remove(pos);
            } else {
                field.visible_to_groups.push(group.to_string());
            }
        }
    }
}

impl WorkflowEngine for ContactEditEngine {
    fn current_screen(&self) -> ScreenModel {
        match self.step {
            ContactEditStep::EditFields => self.build_edit_fields_screen(),
            ContactEditStep::EditVisibility => self.build_edit_visibility_screen(),
            ContactEditStep::Preview => self.build_preview_screen(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "display_name" => {
                self.contact.display_name = value;
                ActionResult::UpdateScreen(self.current_screen())
            }

            UserAction::ActionPressed { action_id } if action_id == "continue" => match self.step {
                ContactEditStep::EditFields => {
                    if self.contact.display_name.trim().is_empty() {
                        return ActionResult::ValidationError {
                            component_id: "display_name".into(),
                            message: "Name is required".into(),
                        };
                    }
                    self.step = ContactEditStep::EditVisibility;
                    ActionResult::NavigateTo(self.current_screen())
                }
                ContactEditStep::EditVisibility => {
                    self.step = ContactEditStep::Preview;
                    ActionResult::NavigateTo(self.current_screen())
                }
                ContactEditStep::Preview => ActionResult::UpdateScreen(self.current_screen()),
            },

            UserAction::ActionPressed { action_id } if action_id == "back" => match self.step {
                ContactEditStep::EditVisibility => {
                    self.step = ContactEditStep::EditFields;
                    ActionResult::NavigateTo(self.current_screen())
                }
                ContactEditStep::Preview => {
                    self.step = ContactEditStep::EditVisibility;
                    ActionResult::NavigateTo(self.current_screen())
                }
                ContactEditStep::EditFields => ActionResult::UpdateScreen(self.current_screen()),
            },

            UserAction::ActionPressed { action_id } if action_id == "save" => {
                if self.step == ContactEditStep::Preview {
                    ActionResult::Complete
                } else {
                    ActionResult::UpdateScreen(self.current_screen())
                }
            }

            UserAction::FieldVisibilityChanged {
                field_id,
                group_id: Some(group),
                visible,
            } => {
                if let Some(field) = self.contact.fields.iter_mut().find(|f| f.id == field_id) {
                    if visible {
                        if !field.visible_to_groups.contains(&group) {
                            field.visible_to_groups.push(group);
                        }
                    } else {
                        field.visible_to_groups.retain(|g| g != &group);
                    }
                }
                ActionResult::UpdateScreen(self.current_screen())
            }

            UserAction::FieldVisibilityChanged {
                field_id,
                group_id: None,
                visible,
            } => {
                if let Some(field) = self.contact.fields.iter_mut().find(|f| f.id == field_id) {
                    field.shown = visible;
                }
                ActionResult::UpdateScreen(self.current_screen())
            }

            UserAction::ItemToggled {
                component_id,
                item_id,
            } => {
                // component_id is "vis_{field_id}"
                if let Some(field_id) = component_id.strip_prefix("vis_") {
                    self.toggle_group_for_field(field_id, &item_id);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }

            UserAction::GroupViewSelected { group_name } => {
                self.selected_preview_group = group_name;
                ActionResult::UpdateScreen(self.current_screen())
            }

            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
