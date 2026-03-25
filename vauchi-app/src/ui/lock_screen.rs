// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lock screen engine — PIN entry screen with attempt tracking.

use crate::ui::*;
use zeroize::Zeroize;

/// Lock screen engine — prompts for a password and tracks failed attempts.
#[derive(Debug)]
pub struct LockScreenEngine {
    entered_pin: String,
    pin_length: usize,
    max_attempts: usize,
    attempts: usize,
}

impl Drop for LockScreenEngine {
    fn drop(&mut self) {
        self.entered_pin.zeroize();
    }
}

impl LockScreenEngine {
    pub fn new(max_attempts: usize) -> Self {
        Self {
            entered_pin: String::new(),
            pin_length: 6,
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
            length: self.pin_length,
            filled: self.entered_pin.len(),
            masked: true,
            validation_error: self.pin_validation_error(),
            accessible_label: None,
            accessible_hint: None,
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

    fn collected_input(&self) -> Option<String> {
        if self.entered_pin.is_empty() {
            None
        } else {
            Some(self.entered_pin.clone())
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "pin" => {
                if value.is_empty() {
                    // Backspace: remove last character
                    self.entered_pin.pop();
                } else if value.len() == 1 {
                    // Single character: accumulate (ignore if at max length)
                    if self.entered_pin.len() < self.pin_length {
                        self.entered_pin.push_str(&value);
                    }
                } else {
                    // Full value (e.g. from programmatic input): replace
                    self.entered_pin = value;
                }
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
            UserAction::ActionPressed { action_id } if action_id == "auth_failed" => {
                self.record_failed_attempt();
                self.entered_pin.zeroize();
                self.entered_pin.clear();
                ActionResult::UpdateScreen(self.current_screen())
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
