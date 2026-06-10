// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Form-dialog completion handlers for `AppEngine` — split from
//! `completion.rs` (file-size hard limit) along the existing seam:
//! `complete_form_dialog` + the per-`FormDialogType` helpers and the
//! shared `form_saved` tail.

use super::{AppEngine, AppScreen};
use crate::ui::action::ActionResult;
use crate::ui::form_dialog::FormDialogType;
use vauchi_core::contact_card::FieldType;

impl AppEngine {
    /// Form dialog complete: dispatch by dialog type, then the common
    /// save-and-navigate-back tail (`form_saved`). Cancel navigates back.
    pub(super) fn complete_form_dialog(&mut self, dialog_type: &FormDialogType) -> ActionResult {
        // Cancel navigates back without saving
        if self.engine.was_cancelled() {
            let screen = self.navigate_back();
            return ActionResult::NavigateTo(screen);
        }
        use crate::ui::FormInput;
        let input = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Form(input)) => Some(input),
            other => {
                tracing::warn!(?other, "form-dialog completion without Form output");
                None
            }
        };
        match dialog_type {
            FormDialogType::EditName { .. } => {
                let name = match input {
                    Some(FormInput::EditName { name }) => Some(name),
                    _ => None,
                };
                self.form_edit_name(name)
            }
            FormDialogType::EditField { field_id, .. } => {
                let (value, note) = match input {
                    Some(FormInput::EditField { value, note }) => (value, note),
                    _ => (String::new(), String::new()),
                };
                self.form_edit_field(field_id, value, note)
            }
            FormDialogType::AddField { .. } => self.form_add_field(input),
            FormDialogType::CreateGroup => {
                let name = match input {
                    Some(FormInput::CreateGroup { name }) => Some(name),
                    _ => None,
                };
                self.form_create_group(name)
            }
            FormDialogType::RenameGroup { group_id, .. } => {
                let name = match input {
                    Some(FormInput::RenameGroup { name }) => Some(name),
                    _ => None,
                };
                self.form_rename_group(group_id, name)
            }
            FormDialogType::EditRelayUrl { .. } => match input {
                // Persist durably via core so the change survives a restart on
                // every frontend (mobile had no Backend, so this was a no-op).
                Some(FormInput::EditRelayUrl { url }) => {
                    let result = self.vauchi.set_relay_url(&url);
                    self.form_saved(result)
                }
                _ => self.form_saved(Ok::<(), std::convert::Infallible>(())),
            },
        }
    }

    /// Common tail for a saved form dialog: invalidate the parent cache
    /// (except Onboarding, whose state must survive round-trips) and
    /// navigate back, or surface the save error.
    fn form_saved<E: std::fmt::Display>(&mut self, result: Result<(), E>) -> ActionResult {
        match result {
            Ok(()) => {
                // Invalidate parent screen cache so it refreshes with updated data.
                // Exception: don't invalidate Onboarding — its state machine
                // (step, groups, name) must survive FormDialog round-trips.
                // The onboarding ContactInfo screen syncs fields from storage.
                if let Some(parent) = self.nav_history.last()
                    && !matches!(parent, AppScreen::Onboarding)
                {
                    self.engine_cache.remove(parent);
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            Err(e) => ActionResult::ShowAlert {
                title: "Error".into(),
                message: format!("{e}"),
            },
        }
    }

    /// `FormDialogType::EditName` — update the display name.
    fn form_edit_name(&mut self, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "display_name".into(),
                message: "Display name cannot be empty".into(),
            };
        }
        let result = self.vauchi.update_display_name(&name);
        self.form_saved(result)
    }

    /// `FormDialogType::EditField` — update a field's value + note.
    fn form_edit_field(&mut self, field_id: &str, value: String, note: String) -> ActionResult {
        let note = note.trim().to_string();
        let now = self.vauchi.clock().unix_seconds();
        let result = match self.vauchi.own_card() {
            Ok(Some(mut card)) => {
                if let Err(e) = card.update_field_value(field_id, &value, now) {
                    return ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("Failed to update field: {e}"),
                    };
                }
                let note_opt = if note.is_empty() { None } else { Some(note) };
                if let Err(e) = card.update_field_note(field_id, note_opt) {
                    return ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("Failed to update field note: {e}"),
                    };
                }
                self.vauchi.update_own_card(&card).map(|_| ())
            }
            Ok(None) => {
                return ActionResult::ShowAlert {
                    title: "Error".into(),
                    message: "No contact card found".into(),
                };
            }
            Err(e) => Err(e),
        };
        self.form_saved(result)
    }

    /// `FormDialogType::AddField` — parse + add a new own-card field, then
    /// apply group visibility and buffer it into a cached onboarding engine.
    fn form_add_field(&mut self, input: Option<crate::ui::FormInput>) -> ActionResult {
        let (entry_type, label_input, value, note, groups) = match input {
            Some(crate::ui::FormInput::AddField {
                entry_type,
                label,
                value,
                note,
                groups,
            }) => (entry_type, label, value, note, groups),
            _ => (
                "custom".to_string(),
                String::new(),
                String::new(),
                String::new(),
                Vec::new(),
            ),
        };
        let entry_type = entry_type.trim();
        let label_input = label_input.trim();
        let value = value.trim();
        let note = note.trim();
        if value.is_empty() {
            return ActionResult::ValidationError {
                component_id: "field_value".into(),
                message: "Value cannot be empty".into(),
            };
        }
        let field_type = match entry_type {
            "phone" => FieldType::Phone,
            "email" => FieldType::Email,
            "social" => FieldType::Social,
            s if s.starts_with("social:") => FieldType::Social,
            "address" => FieldType::Address,
            "website" => FieldType::Website,
            "birthday" => FieldType::Birthday,
            _ => FieldType::Custom,
        };
        // Use label_input if provided, otherwise derive from catalog
        let label = if !label_input.is_empty() {
            label_input.to_string()
        } else if let Some(entry) = self.field_catalog.get(entry_type) {
            entry.display_name.clone()
        } else {
            entry_type
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string() + &entry_type[1..])
                .unwrap_or_else(|| "Custom".into())
        };
        let mut field = vauchi_core::contact_card::ContactField::new(
            field_type,
            &label,
            value,
            self.vauchi.clock().unix_seconds(),
        );
        if !note.is_empty() {
            field = field.with_note(note.to_string());
        }
        let field_id = field.id().to_string();
        let group_list: Vec<String> = groups
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let result = self.vauchi.add_own_field(field);
        // Apply group visibility from selected groups
        if result.is_ok() && !group_list.is_empty() {
            for group_id in &group_list {
                // best-effort: per-group visibility after
                // field was added successfully; failures
                // here are recoverable from group settings
                #[allow(clippy::let_underscore_must_use)]
                let _ = self
                    .vauchi
                    .set_group_field_visibility(group_id, &field_id, true);
            }
        }
        // During onboarding, also buffer the field in the cached
        // OnboardingEngine so build_contact_info shows it.
        if result.is_ok()
            && let Some(parent) = self.nav_history.last()
            && matches!(parent, AppScreen::Onboarding)
            && let Some(engine) = self.engine_cache.get_mut(parent)
            && let Some(ob) = engine
                .as_any_mut()
                .and_then(|a| a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>())
        {
            ob.push_field(crate::ui::onboarding::FieldSetup {
                field_type: entry_type.to_string(),
                label: label.clone(),
                value: value.to_string(),
                visible_to_groups: group_list,
                shown: true,
            });
        }
        self.form_saved(result)
    }

    /// `FormDialogType::CreateGroup` — create a new group.
    fn form_create_group(&mut self, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "group_name".into(),
                message: "Group name cannot be empty".into(),
            };
        }
        let result = self.vauchi.create_group(name.trim()).map(|_| ());
        self.form_saved(result)
    }

    /// `FormDialogType::RenameGroup` — rename an existing group.
    fn form_rename_group(&mut self, group_id: &str, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "group_name".into(),
                message: "Group name cannot be empty".into(),
            };
        }
        let result = self.vauchi.rename_group(group_id, name.trim());
        self.form_saved(result)
    }
}
