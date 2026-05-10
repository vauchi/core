// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Change-password engine — rotates the app password from Settings.
//!
//! Single screen with three masked inputs (current / new / confirm).
//! `submit` only enables when all three are non-empty, `new == confirm`,
//! and `new != current`. The actual rotation happens in
//! `handle_completion` for `AppScreen::ChangePassword`, which calls
//! [`vauchi_core::api::Vauchi::change_app_password`].
//!
//! Pure-renderer rule (ADR-021/043): the engine never branches on form
//! factor and exposes only UI-shaped wire types.

use crate::ui::*;
use zeroize::Zeroize;

/// Engine that drives the change-app-password form.
pub struct ChangePasswordEngine {
    current: String,
    new_pw: String,
    confirm: String,
}

impl Default for ChangePasswordEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ChangePasswordEngine {
    fn drop(&mut self) {
        self.current.zeroize();
        self.new_pw.zeroize();
        self.confirm.zeroize();
    }
}

impl ChangePasswordEngine {
    pub fn new() -> Self {
        Self {
            current: String::new(),
            new_pw: String::new(),
            confirm: String::new(),
        }
    }

    /// Returns the current password the user entered. Used by
    /// `handle_completion` after the engine signals `Complete`.
    pub fn current_password(&self) -> &str {
        &self.current
    }

    /// Returns the new password the user entered. Used by
    /// `handle_completion` after the engine signals `Complete`.
    pub fn new_password(&self) -> &str {
        &self.new_pw
    }

    fn submit_enabled(&self) -> bool {
        !self.current.is_empty()
            && !self.new_pw.is_empty()
            && self.new_pw == self.confirm
            && self.new_pw != self.current
    }

    fn confirm_validation_error(&self) -> Option<String> {
        if !self.confirm.is_empty() && self.confirm != self.new_pw {
            Some("New password and confirmation do not match".into())
        } else {
            None
        }
    }

    fn new_validation_error(&self) -> Option<String> {
        if !self.new_pw.is_empty() && !self.current.is_empty() && self.new_pw == self.current {
            Some("New password must differ from current password".into())
        } else {
            None
        }
    }

    fn build_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "change_password".into(),
            title: "Change Password".into(),
            subtitle: None,
            components: vec![
                Component::TextInput {
                    id: "current_password".into(),
                    label: "Current Password".into(),
                    value: String::new(),
                    placeholder: Some("Enter your current password".into()),
                    max_length: Some(128),
                    validation_error: None,
                    input_type: InputType::Password,
                    a11y: Some(A11y {
                        label: Some("Current password".into()),
                        hint: Some("Enter your current app password.".into()),
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                },
                Component::TextInput {
                    id: "new_password".into(),
                    label: "New Password".into(),
                    value: String::new(),
                    placeholder: Some("Choose a new password".into()),
                    max_length: Some(128),
                    validation_error: self.new_validation_error(),
                    input_type: InputType::Password,
                    a11y: Some(A11y {
                        label: Some("New password".into()),
                        hint: Some(
                            "Choose a new password — must differ from your current one.".into(),
                        ),
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                },
                Component::TextInput {
                    id: "confirm_password".into(),
                    label: "Confirm New Password".into(),
                    value: String::new(),
                    placeholder: Some("Re-enter the new password".into()),
                    max_length: Some(128),
                    validation_error: self.confirm_validation_error(),
                    input_type: InputType::Password,
                    a11y: Some(A11y {
                        label: Some("Confirm new password".into()),
                        hint: Some("Re-enter the new password to confirm.".into()),
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
                    enabled: self.submit_enabled(),
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

impl WorkflowEngine for ChangePasswordEngine {
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

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
