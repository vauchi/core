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

    /// Localized inline message for a field-value validation failure,
    /// anchored on the value input.
    ///
    /// Validating before the save is what keeps the error out of the UI as
    /// a `Display` chain: letting `update_own_card` fail surfaced
    /// `invalid state: Validation error: Invalid phone number format` to
    /// users (verification 2026-08-07, TUI-12/QT-5). Core owns the
    /// error-to-copy translation (ADR-045 Amendment 1) — a shell must
    /// never receive the chain and never classify it.
    fn field_value_validation_error(
        &self,
        field_type: &FieldType,
        value: &str,
    ) -> Option<ActionResult> {
        use vauchi_core::contact_card::ValidationError as Ve;
        let message = match vauchi_core::contact_card::validate_value(field_type, value) {
            Ok(()) => return None,
            Err(Ve::InvalidPhone) => self.t("validation.invalid_phone"),
            Err(Ve::InvalidEmail) => self.t("validation.invalid_email"),
            Err(Ve::InvalidUrl) => self.t("validation.invalid_url"),
            Err(Ve::EmptyValue) => self.t("validation.field_empty"),
            Err(Ve::ValueTooLong { max }) => crate::i18n::get_string_with_args(
                self.render_context.resolved_locale(),
                "validation.too_long",
                &[("max", &max.to_string())],
            ),
            Err(Ve::InvalidSocialUsername) => self.t("validation.invalid_format"),
            // ValidationError is #[non_exhaustive]: a variant added later
            // must degrade to generic copy here rather than fall through to
            // the save and reach the user as a Display chain.
            Err(_) => self.t("validation.invalid_format"),
        };
        Some(ActionResult::ValidationError {
            component_id: "field_value".into(),
            message,
        })
    }

    /// Common tail for a saved form dialog: invalidate the parent cache
    /// (except Onboarding, whose state must survive round-trips) and
    /// navigate back, or surface the save error.
    fn form_saved<E: std::fmt::Display>(&mut self, result: Result<(), E>) -> ActionResult {
        match result {
            Ok(()) => {
                self.invalidate_parent_screen_cache();
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            Err(e) => ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: format!("{e}"),
            },
        }
    }

    /// Drop the parent screen's cached engine so it refreshes with the
    /// data the form just wrote.
    ///
    /// Exception: don't invalidate Onboarding — its state machine (step,
    /// groups, name) must survive FormDialog round-trips. The onboarding
    /// ContactInfo screen syncs fields from storage.
    fn invalidate_parent_screen_cache(&mut self) {
        if let Some(parent) = self.nav_history.last()
            && !matches!(parent, AppScreen::Onboarding)
        {
            self.engine_cache.remove(parent);
        }
    }

    /// `FormDialogType::EditName` — update the display name.
    fn form_edit_name(&mut self, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "display_name".into(),
                message: self.t("settings.error_empty_name"),
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
                if let Some(field_type) = card
                    .fields()
                    .iter()
                    .find(|f| f.id() == field_id)
                    .map(|f| f.field_type())
                    && let Some(invalid) = self.field_value_validation_error(&field_type, &value)
                {
                    return invalid;
                }
                if let Err(e) = card.update_field_value(field_id, &value, now) {
                    return ActionResult::ShowAlert {
                        title: self.t("error.title"),
                        message: format!("Failed to update field: {e}"),
                    };
                }
                let note_opt = if note.is_empty() { None } else { Some(note) };
                if let Err(e) = card.update_field_note(field_id, note_opt) {
                    return ActionResult::ShowAlert {
                        title: self.t("error.title"),
                        message: format!("Failed to update field note: {e}"),
                    };
                }
                self.vauchi.update_own_card(&card).map(|_| ())
            }
            Ok(None) => {
                return ActionResult::ShowAlert {
                    title: self.t("error.title"),
                    message: self.t("contact_detail.card_not_found"),
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
                message: self.t("validation.field_empty"),
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
        if let Some(invalid) = self.field_value_validation_error(&field_type, value) {
            return invalid;
        }
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
        // A grant can fail on its own (e.g. the group was deleted while this
        // form was open). The field is still saved, so count the shortfall
        // instead of aborting, and report it below — a silently ungranted
        // field reads to the user as shared when nobody can see it.
        let mut ungranted_groups = 0_usize;
        if result.is_ok() {
            for group_id in &group_list {
                if self
                    .vauchi
                    .set_group_field_visibility(group_id, &field_id, true)
                    .is_err()
                {
                    ungranted_groups += 1;
                }
            }
        }
        // During onboarding, also buffer the field in the cached
        // OnboardingEngine so build_contact_info shows it.
        if result.is_ok()
            && let Some(parent) = self.nav_history.last()
            && matches!(parent, AppScreen::Onboarding)
            && let Some(engine) = self.engine_cache.get_mut(parent)
            && !engine.apply_update(crate::ui::EngineUpdate::Onboarding(
                crate::ui::OnboardingUpdate::PushField(crate::ui::onboarding::FieldSetup {
                    field_type: entry_type.to_string(),
                    label: label.clone(),
                    value: value.to_string(),
                    visible_to_groups: group_list,
                    shown: true,
                }),
            ))
        {
            tracing::warn!("onboarding PushField not consumed by cached engine");
        }
        if result.is_ok() && ungranted_groups > 0 {
            self.invalidate_parent_screen_cache();
            return ActionResult::ShowAlert {
                title: self.t("my_info.add_field.groups_not_shared_title"),
                message: crate::i18n::get_string_with_args(
                    self.render_context.resolved_locale(),
                    "my_info.add_field.groups_not_shared_message",
                    &[("count", &ungranted_groups.to_string())],
                ),
            };
        }
        self.form_saved(result)
    }

    /// `FormDialogType::CreateGroup` — create a new group.
    fn form_create_group(&mut self, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "group_name".into(),
                message: self.t("groups.error_empty_name"),
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
                message: self.t("groups.error_empty_name"),
            };
        }
        let result = self.vauchi.rename_group(group_id, name.trim());
        self.form_saved(result)
    }
}
