// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact-edit workflow engine — a pure state machine for the 3-step
//! contact editing flow (EditFields → EditVisibility → Preview).
//! No Storage or Vauchi dependency; the caller persists results when
//! [`ActionResult::Complete`] is returned.

use crate::i18n::{Locale, get_string, get_string_with_args};
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
    /// Avatar image bytes (WebP) for the Preview component.
    avatar_data: Option<Vec<u8>>,
    locale: Locale,
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
            avatar_data: None,
            locale: Locale::English,
        }
    }

    /// Set the avatar image data for the Preview component.
    pub fn with_avatar_data(mut self, data: Option<Vec<u8>>) -> Self {
        self.avatar_data = data;
        self
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-13).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Returns a reference to the edited contact data.
    pub fn edited_contact(&self) -> &EditableContact {
        &self.contact
    }

    // ── Private helpers ─────────────────────────────────────────────

    fn field_to_display(&self, field: &EditableField) -> Field {
        let visibility = if !field.visible_to_groups.is_empty() {
            UiFieldVisibility::Scopes(field.visible_to_groups.clone())
        } else if field.shown {
            UiFieldVisibility::Shown
        } else {
            UiFieldVisibility::Hidden
        };

        Field {
            id: field.id.clone(),
            field_type: field.field_type.clone(),
            label: field.label.clone(),
            value: field.value.clone(),
            icon: crate::ui::component::icon_for_field_type(&field.field_type).into(),
            a11y: Some(A11y {
                label: Some(format!("{}: {}", field.label, field.value)),
                hint: match visibility {
                    UiFieldVisibility::Shown => None,
                    UiFieldVisibility::Hidden => Some(self.t("contact_edit.field_hidden_hint")),
                    UiFieldVisibility::Scopes(_) => Some(self.t("contact_edit.field_groups_hint")),
                },
                role: None,
            }),
            visibility,
        }
    }

    fn build_variants(&self) -> Vec<PreviewVariant> {
        self.available_groups
            .iter()
            .map(|group| {
                let visible_fields: Vec<Field> = self
                    .contact
                    .fields
                    .iter()
                    .filter(|f| f.visible_to_groups.contains(group))
                    .map(|f| self.field_to_display(f))
                    .collect();

                PreviewVariant {
                    variant_id: group.clone(),
                    display_name: self.contact.display_name.clone(),
                    visible_fields,
                }
            })
            .collect()
    }

    fn build_edit_fields_screen(&self) -> ScreenModel {
        let fields: Vec<Field> = self
            .contact
            .fields
            .iter()
            .map(|f| self.field_to_display(f))
            .collect();

        let name_not_empty = !self.contact.display_name.trim().is_empty();

        ScreenModel {
            screen_id: "edit_fields".into(),
            title: self.t("contact_edit.edit_contact_title"),
            subtitle: None,
            components: vec![
                Component::TextInput {
                    id: "display_name".into(),
                    label: self.t("settings.display_name"),
                    value: self.contact.display_name.clone(),
                    placeholder: Some(self.t("contact_edit.enter_name_placeholder")),
                    max_length: None,
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: None,
                    info_key: None,
                },
                Component::Divider,
                Component::FieldList {
                    id: "fields".into(),
                    fields,
                    visibility_mode: VisibilityMode::ShowHide,
                    available_scopes: self.available_groups.clone(),
                    a11y: Some(A11y {
                        label: Some(self.t("fields.a11y_contact_fields")),
                        hint: Some(self.t("fields.a11y_toggle_hint")),
                        role: None,
                    }),
                },
            ],
            actions: vec![ScreenAction {
                id: "continue".into(),
                label: self.t("action.continue"),
                style: ActionStyle::Primary,
                enabled: name_not_empty,
                a11y: None,
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
        let toggle_hint = self.t("onboarding.a11y_toggle_hint");
        let toggle_lists: Vec<Component> = self
            .contact
            .fields
            .iter()
            .map(|field| {
                let items: Vec<ToggleItem> = self
                    .available_groups
                    .iter()
                    .map(|group| {
                        let selected = field.visible_to_groups.contains(group);
                        ToggleItem {
                            id: group.clone(),
                            label: group.clone(),
                            selected,
                            subtitle: None,
                            a11y: Some(A11y {
                                label: Some(format!(
                                    "{}, {}",
                                    group,
                                    if selected {
                                        self.t("onboarding.a11y_selected")
                                    } else {
                                        self.t("onboarding.a11y_not_selected")
                                    }
                                )),
                                hint: Some(toggle_hint.clone()),
                                role: Some(AccessibilityRole::Toggle),
                            }),
                            info_key: None,
                        }
                    })
                    .collect();

                Component::ToggleList {
                    id: format!("vis_{}", field.id),
                    label: field.label.clone(),
                    items,
                    a11y: Some(A11y {
                        label: Some(get_string_with_args(
                            self.locale,
                            "contact_edit.field_options_a11y",
                            &[("label", &field.label)],
                        )),
                        hint: Some(self.t("contact_detail.select_items_hint")),
                        role: None,
                    }),
                }
            })
            .collect();

        ScreenModel {
            screen_id: "edit_visibility".into(),
            title: self.t("group_detail.field_visibility_label"),
            subtitle: None,
            components: toggle_lists,
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "continue".into(),
                    label: self.t("contact_edit.preview_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
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
        let fields: Vec<Field> = self
            .contact
            .fields
            .iter()
            .map(|f| self.field_to_display(f))
            .collect();
        let variants = self.build_variants();
        let selected_variant = self.selected_preview_group.clone();
        let visible_fields =
            crate::ui::component::build_visible_fields(&fields, &variants, &selected_variant);

        ScreenModel {
            screen_id: "edit_preview".into(),
            title: self.t("contact_edit.preview_card_title"),
            subtitle: None,
            components: vec![Component::Preview {
                name: self.contact.display_name.clone(),
                initials: crate::ui::component::initials(&self.contact.display_name),
                image_data: self.avatar_data.clone(),
                fields,
                variants,
                selected_variant,
                visible_fields,
                a11y: Some(A11y {
                    label: Some(get_string_with_args(
                        self.locale,
                        "contact_edit.card_preview_a11y",
                        &[("name", &self.contact.display_name)],
                    )),
                    hint: Some(self.t("contact_edit.card_preview_hint")),
                    role: None,
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "save".into(),
                    label: self.t("contact_edit.save_changes_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
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
    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::ContactEdit {
            display_name: self.edited_contact().display_name.clone(),
        })
    }

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
                            message: self.t("contact_edit.name_required_error"),
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

            UserAction::VariantSelected { variant_id } => {
                self.selected_preview_group = variant_id;
                ActionResult::UpdateScreen(self.current_screen())
            }

            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
