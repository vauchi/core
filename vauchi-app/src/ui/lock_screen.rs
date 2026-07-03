// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lock screen engine — credential entry with attempt tracking.

use crate::ui::*;
use zeroize::Zeroize;

/// Default maximum failed unlock attempts before lockout.
pub const DEFAULT_LOCK_MAX_ATTEMPTS: usize = 5;

/// Lock screen engine — prompts for a password and tracks failed attempts.
#[derive(Debug)]
pub struct LockScreenEngine {
    entered_pin: String,
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
        // A masked free-text field, not a fixed-length PinInput: the app
        // password can be up to 128 chars and alphanumeric, and the duress
        // PIN is typed into this same field — a numeric 6-slot widget locks
        // both out (2026-07-03-lock-screen-pin-cap-locks-out-passwords).
        let components = vec![Component::TextInput {
            id: "pin".into(),
            label: "Password".into(),
            // Echo the entered value: the TUI reconstructs the field from
            // this on every keystroke (no local buffer); masking is the
            // renderer's job via `input_type` (matches `display_name`).
            value: self.entered_pin.clone(),
            placeholder: None,
            max_length: Some(128),
            validation_error: self.pin_validation_error(),
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some("Password entry".into()),
                hint: Some("Enter your password to unlock".into()),
                role: None,
            }),
            info_key: None,
        }];

        let actions = vec![ScreenAction {
            id: "unlock".into(),
            label: "Unlock".into(),
            style: ActionStyle::Primary,
            enabled: !self.entered_pin.is_empty(),
            a11y: None,
        }];

        ScreenModel {
            screen_id: "lock_screen".into(),
            title: "Enter Password".into(),
            subtitle: None,
            components,
            actions,
            progress: None,
            ..Default::default()
        }
    }

    fn engine_output(&self) -> Option<EngineOutput> {
        if self.entered_pin.is_empty() {
            None
        } else {
            Some(EngineOutput::Lock {
                pin: self.entered_pin.clone(),
            })
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "pin" => {
                // Masked TextInput sends the full current value on each change.
                self.entered_pin.zeroize();
                self.entered_pin = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            // "unlock" is the rendered button; "submit_pin" is what frontends
            // emit when the user presses Enter/return in the password
            // TextInput (the `submit_{id}` convention shared by onboarding,
            // backup, etc.). Both must unlock — else Enter-to-unlock is dead
            // (a TUI regression from the lock input becoming a TextInput,
            // 2026-07-03-lock-screen-pin-cap-locks-out-passwords).
            UserAction::ActionPressed { action_id }
                if action_id == "unlock" || action_id == "submit_pin" =>
            {
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
