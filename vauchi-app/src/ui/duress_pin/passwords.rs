// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress setup's password branch (ADR-032, amended 2026-11-10): on a
//! device without biometric unlock, the current app password, then a
//! duress password and its confirmation, both under the backup password
//! rule.

use vauchi_core::identity::password::validate_password;
use zeroize::Zeroize;

use super::{DuressPinEngine, DuressPinStep};
use crate::ui::*;

const CHANGE_APP_PASSWORD: &str = "change_app_password";

fn is_real_password(password: &str) -> bool {
    validate_password(password).is_ok()
}

impl DuressPinEngine {
    pub(super) fn start_password_entry(&mut self) -> ActionResult {
        self.step = DuressPinStep::EnterAppPassword;
        self.app_password.zeroize();
        self.new_pin.zeroize();
        self.confirm_pin.zeroize();
        self.app_password_weak = false;
        ActionResult::NavigateTo(self.current_screen())
    }

    /// Back to the app password step after Core refused the app password.
    pub(super) fn refuse_app_password(&mut self) {
        self.step = DuressPinStep::EnterAppPassword;
        self.app_password.zeroize();
    }

    fn password_input(&self, id: &str, label_key: &str, hint_key: &str, value: &str) -> Component {
        Component::TextInput {
            id: id.into(),
            label: self.t(label_key),
            value: value.to_string(),
            placeholder: None,
            max_length: None,
            validation_error: None,
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(self.t(label_key)),
                hint: Some(self.t(hint_key)),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        }
    }

    fn step_actions(&self) -> Vec<ScreenAction> {
        let action = |id: &str, key: &str, style: ActionStyle| ScreenAction {
            id: id.into(),
            label: self.t(key),
            style,
            enabled: true,
            a11y: Some(A11y::labeled(self.t(key))),
        };
        let mut actions = vec![
            action("back", "action.back", ActionStyle::Secondary),
            action("continue", "action.continue", ActionStyle::Primary),
        ];
        if self.step == DuressPinStep::EnterAppPassword && self.app_password_weak {
            actions.push(action(
                CHANGE_APP_PASSWORD,
                "resistance.duress.change_app_password",
                ActionStyle::Secondary,
            ));
        }
        actions
    }

    fn password_step_screen(&self, screen_id: &str, components: Vec<Component>) -> ScreenModel {
        ScreenModel {
            screen_id: screen_id.into(),
            title: self.t("resistance.duress.setup"),
            subtitle: None,
            components,
            contextual_actions: self.step_actions(),
            progress: self.progress(),
            ..Default::default()
        }
    }

    pub(super) fn enter_app_password_screen(&self) -> ScreenModel {
        self.password_step_screen(
            "duress_enter_app_password",
            vec![
                Component::Text {
                    id: "passwords_explanation".into(),
                    content: self.t("resistance.duress.passwords_explanation"),
                    style: TextStyle::Body,
                    a11y: None,
                },
                self.password_input(
                    "app_password",
                    "resistance.duress.enter_app_password",
                    "resistance.duress.enter_app_password_hint",
                    &self.app_password,
                ),
            ],
        )
    }

    pub(super) fn enter_password_screen(&self) -> ScreenModel {
        self.password_step_screen(
            "duress_enter_password",
            vec![
                self.password_input(
                    "password",
                    "resistance.duress.enter_password",
                    "resistance.duress.enter_password_hint",
                    &self.new_pin,
                ),
                Component::Text {
                    id: "shred_note".into(),
                    content: self.t("resistance.duress.shred_note"),
                    style: TextStyle::Caption,
                    a11y: None,
                },
                Component::Text {
                    id: "forensic_note".into(),
                    content: self.t("resistance.duress.forensic_note"),
                    style: TextStyle::Caption,
                    a11y: None,
                },
            ],
        )
    }

    pub(super) fn confirm_password_screen(&self) -> ScreenModel {
        self.password_step_screen(
            "duress_confirm_password",
            vec![self.password_input(
                "confirm_password",
                "resistance.duress.confirm_password",
                "resistance.duress.confirm_password_entry_hint",
                &self.confirm_pin,
            )],
        )
    }

    fn field_error(&self, component_id: &str, key: &str) -> ActionResult {
        ActionResult::ValidationError {
            component_id: component_id.into(),
            message: self.t(key),
        }
    }

    fn replace(field: &mut String, value: String) {
        field.zeroize();
        *field = value;
    }

    fn go_to(&mut self, step: DuressPinStep) -> ActionResult {
        self.step = step;
        ActionResult::NavigateTo(self.current_screen())
    }

    fn continue_from_app_password(&mut self) -> ActionResult {
        if self.app_password.is_empty() {
            return self.field_error("app_password", "resistance.duress.error_empty_password");
        }
        self.app_password_weak = !is_real_password(&self.app_password);
        if self.app_password_weak {
            return self.field_error("app_password", "resistance.duress.error_weak_app_password");
        }
        self.go_to(DuressPinStep::EnterPassword)
    }

    fn continue_from_password(&mut self) -> ActionResult {
        if self.new_pin.is_empty() {
            self.field_error("password", "resistance.duress.error_empty_password")
        } else if !is_real_password(&self.new_pin) {
            self.field_error("password", "resistance.duress.error_weak_password")
        } else {
            self.go_to(DuressPinStep::ConfirmPassword)
        }
    }

    fn continue_from_confirmation(&mut self) -> ActionResult {
        if self.confirm_pin != self.new_pin {
            return self.field_error(
                "confirm_password",
                "resistance.duress.error_password_mismatch",
            );
        }
        self.config.enabled = true;
        ActionResult::Complete
    }

    pub(super) fn handle_password_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            (
                DuressPinStep::EnterAppPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "app_password" => {
                Self::replace(&mut self.app_password, value);
                ActionResult::UpdateScreen(self.current_screen())
            }
            (
                DuressPinStep::EnterPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "password" => {
                Self::replace(&mut self.new_pin, value);
                ActionResult::UpdateScreen(self.current_screen())
            }
            (
                DuressPinStep::ConfirmPassword,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "confirm_password" => {
                Self::replace(&mut self.confirm_pin, value);
                ActionResult::UpdateScreen(self.current_screen())
            }
            (step, UserAction::ActionPressed { action_id }) if action_id == "continue" => {
                match step {
                    DuressPinStep::EnterAppPassword => self.continue_from_app_password(),
                    DuressPinStep::EnterPassword => self.continue_from_password(),
                    _ => self.continue_from_confirmation(),
                }
            }
            (step, UserAction::ActionPressed { action_id }) if action_id == "back" => {
                let previous = match step {
                    DuressPinStep::ConfirmPassword => DuressPinStep::EnterPassword,
                    DuressPinStep::EnterPassword => DuressPinStep::EnterAppPassword,
                    _ => DuressPinStep::Overview,
                };
                self.go_to(previous)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
