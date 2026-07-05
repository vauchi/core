// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! App-password engine — sets or rotates the app password from Settings.
//!
//! One engine, two modes (chosen at construction from
//! [`vauchi_core::api::Vauchi::is_password_enabled`]):
//!
//! - **setup** (no password yet): two masked inputs (new / confirm); `submit`
//!   enables when both are non-empty and equal. Completion calls
//!   [`vauchi_core::api::Vauchi::setup_app_password`].
//! - **change** (password exists): three masked inputs (current / new /
//!   confirm); `submit` additionally requires a non-empty current that differs
//!   from new. Completion calls
//!   [`vauchi_core::api::Vauchi::change_app_password`].
//!
//! The mode lives in core so every Humble frontend inherits the setup path —
//! without it, frontends that only render the change form can never configure
//! a first password (see problem `2026-06-13-ios-app-password-setup-missing`).
//!
//! Pure-renderer rule (ADR-021/043): the engine never branches on form
//! factor and exposes only UI-shaped wire types.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use zeroize::Zeroize;

/// Engine that drives the set/change-app-password form.
pub struct ChangePasswordEngine {
    /// True when a password already exists → change mode; false → setup mode.
    password_enabled: bool,
    current: String,
    new_pw: String,
    confirm: String,
    locale: Locale,
}

impl Drop for ChangePasswordEngine {
    fn drop(&mut self) {
        self.current.zeroize();
        self.new_pw.zeroize();
        self.confirm.zeroize();
    }
}

impl ChangePasswordEngine {
    /// `password_enabled` is whether an app password already exists, i.e.
    /// `Vauchi::is_password_enabled()` at the time the screen opens.
    pub fn new(password_enabled: bool) -> Self {
        Self {
            password_enabled,
            current: String::new(),
            new_pw: String::new(),
            confirm: String::new(),
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Returns the current password the user entered. Empty in setup mode.
    /// Used by `handle_completion` after the engine signals `Complete`.
    pub fn current_password(&self) -> &str {
        &self.current
    }

    /// Returns the new password the user entered. Used by
    /// `handle_completion` after the engine signals `Complete`.
    pub fn new_password(&self) -> &str {
        &self.new_pw
    }

    fn submit_enabled(&self) -> bool {
        if self.new_pw.is_empty() || self.new_pw != self.confirm {
            return false;
        }
        if self.password_enabled {
            !self.current.is_empty() && self.new_pw != self.current
        } else {
            true
        }
    }

    fn confirm_validation_error(&self) -> Option<String> {
        if !self.confirm.is_empty() && self.confirm != self.new_pw {
            Some(self.t("change_password.confirm_mismatch_error"))
        } else {
            None
        }
    }

    fn new_validation_error(&self) -> Option<String> {
        if self.password_enabled
            && !self.new_pw.is_empty()
            && !self.current.is_empty()
            && self.new_pw == self.current
        {
            Some(self.t("change_password.new_must_differ_error"))
        } else {
            None
        }
    }

    fn current_password_input(&self) -> Component {
        Component::TextInput {
            id: "current_password".into(),
            label: self.t("change_password.current_password_label"),
            value: String::new(),
            placeholder: Some(self.t("change_password.current_password_placeholder")),
            max_length: Some(128),
            validation_error: None,
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(self.t("change_password.current_password_a11y")),
                hint: Some(self.t("change_password.current_password_hint")),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        }
    }

    fn new_password_input(&self) -> Component {
        let (label, placeholder, a11y_label, hint) = if self.password_enabled {
            (
                self.t("change_password.new_password_label_change"),
                self.t("change_password.new_password_placeholder_change"),
                self.t("change_password.new_password_a11y_change"),
                self.t("change_password.new_password_hint_change"),
            )
        } else {
            (
                self.t("auth.unlock.field_label"),
                self.t("change_password.new_password_placeholder_setup"),
                self.t("auth.unlock.field_label"),
                self.t("change_password.new_password_hint_setup"),
            )
        };
        Component::TextInput {
            id: "new_password".into(),
            label,
            value: String::new(),
            placeholder: Some(placeholder),
            max_length: Some(128),
            validation_error: self.new_validation_error(),
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(a11y_label),
                hint: Some(hint),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        }
    }

    fn confirm_password_input(&self) -> Component {
        let (label, placeholder, a11y_label, hint) = if self.password_enabled {
            (
                self.t("change_password.confirm_password_label_change"),
                self.t("change_password.confirm_password_placeholder_change"),
                self.t("change_password.confirm_password_a11y_change"),
                self.t("change_password.confirm_password_hint_change"),
            )
        } else {
            (
                self.t("change_password.confirm_password_label_setup"),
                self.t("change_password.confirm_password_placeholder_setup"),
                self.t("change_password.confirm_password_a11y_setup"),
                self.t("change_password.confirm_password_hint_setup"),
            )
        };
        Component::TextInput {
            id: "confirm_password".into(),
            label,
            value: String::new(),
            placeholder: Some(placeholder),
            max_length: Some(128),
            validation_error: self.confirm_validation_error(),
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(a11y_label),
                hint: Some(hint),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::with_capacity(3);
        if self.password_enabled {
            components.push(self.current_password_input());
        }
        components.push(self.new_password_input());
        components.push(self.confirm_password_input());

        let (title, subtitle) = if self.password_enabled {
            (self.t("change_password.title_change"), None)
        } else {
            (
                self.t("change_password.title_setup"),
                Some(self.t("change_password.subtitle_setup")),
            )
        };

        ScreenModel {
            screen_id: "change_password".into(),
            title,
            subtitle,
            components,
            actions: vec![
                ScreenAction {
                    id: "submit".into(),
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: self.submit_enabled(),
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
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for ChangePasswordEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::ChangePassword {
            current: self.current_password().to_string(),
            new: self.new_password().to_string(),
        })
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } => {
                match component_id.as_str() {
                    "current_password" => self.current = value,
                    "new_password" => self.new_pw = value,
                    "confirm_password" => self.confirm = value,
                    _ => {}
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "submit" if self.submit_enabled() => ActionResult::Complete,
                "cancel" => {
                    self.current.zeroize();
                    self.new_pw.zeroize();
                    self.confirm.zeroize();
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.current_screen()),
            },
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
