// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lock screen engine — PIN entry screen with attempt tracking.

use crate::ui::*;

/// Lock screen engine — prompts for a password and tracks failed attempts.
#[derive(Clone, Debug)]
pub struct LockScreenEngine {
    entered_pin: String,
    max_attempts: usize,
    attempts: usize,
}

impl LockScreenEngine {
    pub fn new(max_attempts: usize) -> Self {
        Self {
            entered_pin: String::new(),
            max_attempts,
            attempts: 0,
        }
    }

    /// Record a failed unlock attempt. Returns `true` if max attempts reached (lockout).
    pub fn record_failed_attempt(&mut self) -> bool {
        self.attempts += 1;
        self.attempts >= self.max_attempts
    }

    fn pin_validation_error(&self) -> Option<String> {
        if self.attempts > 0 && self.attempts < self.max_attempts {
            let remaining = self.max_attempts - self.attempts;
            Some(format!(
                "{} attempt{} remaining",
                remaining,
                if remaining == 1 { "" } else { "s" }
            ))
        } else {
            None
        }
    }
}

impl WorkflowEngine for LockScreenEngine {
    fn current_screen(&self) -> ScreenModel {
        let components = vec![Component::PinInput {
            id: "pin".into(),
            label: "Password".into(),
            length: 6,
            masked: true,
            validation_error: self.pin_validation_error(),
        }];

        let actions = vec![ScreenAction {
            id: "unlock".into(),
            label: "Unlock".into(),
            style: ActionStyle::Primary,
            enabled: !self.entered_pin.is_empty(),
        }];

        ScreenModel {
            screen_id: "lock_screen".into(),
            title: "Enter Password".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "pin" => {
                self.entered_pin = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "unlock" => {
                if self.entered_pin.is_empty() {
                    ActionResult::ValidationError {
                        component_id: "pin".into(),
                        message: "Please enter your password".into(),
                    }
                } else {
                    ActionResult::Complete
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
