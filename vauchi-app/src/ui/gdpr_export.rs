// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The steps in front of every data export (#712): the session password,
//! confirmed by the AppEngine, then for the data export a format — an
//! encrypted file under its own strong password, or vCard or JSON behind a
//! warning that the file is unprotected. One confirmation covers one export.

use super::*;
use crate::ui::device_link_pin::{PinStepCopy, confirm_pin_screen_for};
use vauchi_core::identity::password::validate_password;

pub(crate) const SET_APP_PASSWORD_ACTION_ID: &str = "set_app_password";
const EXPORT_PASSWORD_ID: &str = "export_password";
const EXPORT_PASSWORD_CONFIRM_ID: &str = "export_password_confirm";

impl GdprEngine {
    pub(super) fn begin_export(&mut self, kind: &'static str) -> ActionResult {
        self.abandon_export();
        self.last_action = None;
        self.pending_export = Some(kind);
        self.step = if self.app_password_set {
            GdprStep::ConfirmExportPin
        } else if self.user_presence_available {
            GdprStep::ExportAwaitPresence
        } else {
            GdprStep::ExportNeedsAppPassword
        };
        ActionResult::NavigateTo(self.build_screen())
    }

    pub(super) fn abandon_export(&mut self) {
        self.pending_export = None;
        self.entered_pin.clear();
        self.pin_error = None;
        self.export_password.clear();
        self.export_password_confirm.clear();
        self.export_password_error = None;
        self.readable_export = None;
    }

    pub(super) fn apply_export_update(&mut self, update: GdprExportUpdate) -> bool {
        match update {
            GdprExportUpdate::Verified
                if matches!(
                    self.step,
                    GdprStep::ConfirmExportPin | GdprStep::ExportAwaitPresence
                ) =>
            {
                self.entered_pin.clear();
                self.pin_error = None;
                match self.pending_export.take() {
                    Some("heirloom") => {
                        self.last_action = Some("heirloom".into());
                        self.step = GdprStep::Overview;
                    }
                    _ => self.step = GdprStep::ChooseExportFormat,
                }
            }
            GdprExportUpdate::PinRejected if matches!(self.step, GdprStep::ConfirmExportPin) => {
                self.entered_pin.clear();
                self.pin_error = Some(self.t("lock_screen.wrong_password"));
            }
            GdprExportUpdate::PinRejected if matches!(self.step, GdprStep::ExportAwaitPresence) => {
                self.pin_error = Some(self.t("privacy.export_presence_rejected"));
            }
            GdprExportUpdate::UserPresenceUnavailable
                if matches!(self.step, GdprStep::ExportAwaitPresence) =>
            {
                self.step = GdprStep::ExportNeedsAppPassword;
            }
            GdprExportUpdate::Finished => {
                self.abandon_export();
                self.last_action = None;
                self.step = GdprStep::Overview;
            }
            _ => return false,
        }
        true
    }

    /// The format choice, the export password and the readable-file
    /// warning; `None` leaves the action to the engine's other arms.
    pub(super) fn handle_export_step(&mut self, action: &UserAction) -> Option<ActionResult> {
        match (&self.step, action) {
            (GdprStep::ChooseExportFormat, UserAction::ActionPressed { action_id }) => {
                match action_id.as_str() {
                    "export_encrypted" => self.step = GdprStep::ExportPassword,
                    "export_vcard" => self.warn_readable(GdprChoice::ExportVcard),
                    "export_json" => self.warn_readable(GdprChoice::Export),
                    _ => return None,
                }
                Some(ActionResult::NavigateTo(self.build_screen()))
            }
            (
                GdprStep::ExportPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) => {
                match component_id.as_str() {
                    EXPORT_PASSWORD_ID => self.export_password.set(value.clone()),
                    EXPORT_PASSWORD_CONFIRM_ID => self.export_password_confirm.set(value.clone()),
                    _ => return None,
                }
                self.export_password_error = None;
                Some(ActionResult::UpdateScreen(self.build_screen()))
            }
            (GdprStep::ExportPassword, UserAction::ActionPressed { action_id })
                if action_id == "confirm_export_password" =>
            {
                Some(self.seal_with_export_password())
            }
            (GdprStep::ReadableExportWarning, UserAction::ActionPressed { action_id })
                if action_id == "confirm_readable_export" =>
            {
                self.last_action = match self.readable_export {
                    Some(GdprChoice::ExportVcard) => Some("export_vcard".into()),
                    Some(GdprChoice::Export) => Some("export".into()),
                    _ => return None,
                };
                Some(ActionResult::Complete)
            }
            _ => None,
        }
    }

    fn warn_readable(&mut self, format: GdprChoice) {
        self.readable_export = Some(format);
        self.step = GdprStep::ReadableExportWarning;
    }

    fn seal_with_export_password(&mut self) -> ActionResult {
        let password = self.export_password.as_str();
        let error = if validate_password(password).is_err() {
            Some("privacy.export_password_weak")
        } else if password != self.export_password_confirm.as_str() {
            Some("backup.error_passwords_mismatch")
        } else {
            None
        };
        if let Some(key) = error {
            self.export_password_error = Some(self.t(key));
            return ActionResult::UpdateScreen(self.build_screen());
        }
        self.last_action = Some("export_encrypted".into());
        ActionResult::Complete
    }

    pub(super) fn build_confirm_export_pin(&self) -> ScreenModel {
        confirm_pin_screen_for(
            self.locale,
            PinStepCopy {
                screen_id: "export_confirm_pin",
                title_key: "privacy.export_data",
                hint_key: "privacy.export_confirm_pin_hint",
            },
            self.entered_pin.as_str(),
            self.pin_error.as_deref(),
        )
    }

    pub(super) fn build_export_needs_app_password(&self) -> ScreenModel {
        self.notice_screen(
            "export_needs_app_password",
            "privacy.export_needs_password_title",
            "privacy.export_needs_password_body",
            SET_APP_PASSWORD_ACTION_ID,
            "privacy.set_app_password",
            ActionStyle::Primary,
        )
    }

    pub(super) fn build_export_await_presence(&self) -> ScreenModel {
        let body = self
            .pin_error
            .clone()
            .unwrap_or_else(|| self.t("privacy.export_presence_waiting"));
        ScreenModel {
            screen_id: "export_await_presence".into(),
            title: self.t("privacy.export_data"),
            components: vec![Component::Text {
                id: "export_await_presence_body".into(),
                content: body.clone(),
                style: TextStyle::Body,
                a11y: Some(A11y::labeled(body)),
            }],
            contextual_actions: vec![ScreenAction {
                id: "cancel".into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            ..Default::default()
        }
    }

    pub(super) fn build_readable_export_warning(&self) -> ScreenModel {
        self.notice_screen(
            "export_readable_warning",
            "privacy.export_readable_title",
            "privacy.export_readable_warning",
            "confirm_readable_export",
            "privacy.export_readable_confirm",
            ActionStyle::Serious,
        )
    }

    pub(super) fn build_choose_export_format(&self) -> ScreenModel {
        let hint = self.t("privacy.export_format_hint");
        let action = |id: &str, key: &str, style: ActionStyle| ScreenAction {
            id: id.into(),
            label: self.t(key),
            style,
            enabled: true,
            a11y: None,
        };
        ScreenModel {
            screen_id: "export_choose_format".into(),
            title: self.t("privacy.export_choose_format_title"),
            components: vec![Component::Text {
                id: "export_format_hint".into(),
                content: hint.clone(),
                style: TextStyle::Body,
                a11y: Some(A11y::labeled(hint)),
            }],
            contextual_actions: vec![
                action(
                    "export_encrypted",
                    "privacy.export_encrypted",
                    ActionStyle::Primary,
                ),
                action(
                    "export_vcard",
                    "privacy.export_vcard",
                    ActionStyle::Secondary,
                ),
                action("export_json", "privacy.export_json", ActionStyle::Secondary),
                action("cancel", "action.cancel", ActionStyle::Secondary),
            ],
            ..Default::default()
        }
    }

    pub(super) fn build_export_password(&self) -> ScreenModel {
        let input =
            |id: &str, label_key: &str, value: &str, error: Option<String>| Component::TextInput {
                id: id.into(),
                label: self.t(label_key),
                value: value.to_string(),
                placeholder: None,
                max_length: Some(256),
                validation_error: error,
                input_type: InputType::Password,
                a11y: Some(A11y::labeled(self.t(label_key))),
                info_key: None,
            };
        let both_entered = !self.export_password.as_str().is_empty()
            && !self.export_password_confirm.as_str().is_empty();
        ScreenModel {
            screen_id: "export_password".into(),
            title: self.t("privacy.export_password_title"),
            subtitle: Some(self.t("privacy.export_password_hint")),
            components: vec![
                input(
                    EXPORT_PASSWORD_ID,
                    "privacy.export_password_label",
                    self.export_password.as_str(),
                    self.export_password_error.clone(),
                ),
                input(
                    EXPORT_PASSWORD_CONFIRM_ID,
                    "privacy.export_password_confirm_label",
                    self.export_password_confirm.as_str(),
                    None,
                ),
            ],
            contextual_actions: vec![
                ScreenAction {
                    id: "confirm_export_password".into(),
                    label: self.t("action.continue"),
                    style: ActionStyle::Primary,
                    enabled: both_entered,
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            ..Default::default()
        }
    }

    fn notice_screen(
        &self,
        screen_id: &str,
        title_key: &str,
        body_key: &str,
        action_id: &str,
        action_key: &str,
        style: ActionStyle,
    ) -> ScreenModel {
        let body = self.t(body_key);
        ScreenModel {
            screen_id: screen_id.into(),
            title: self.t(title_key),
            components: vec![Component::Text {
                id: format!("{screen_id}_body"),
                content: body.clone(),
                style: TextStyle::Body,
                a11y: Some(A11y::labeled(body)),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: action_id.into(),
                    label: self.t(action_key),
                    style,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            ..Default::default()
        }
    }
}
