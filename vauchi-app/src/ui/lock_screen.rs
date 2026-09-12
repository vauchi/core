// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lock screen engine — credential entry with attempt tracking.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use vauchi_core::Command;
use vauchi_core::exchange::capability::types::{BiometricType, DeviceCapabilities};
use zeroize::Zeroize;

/// Default maximum failed unlock attempts before lockout.
pub const DEFAULT_LOCK_MAX_ATTEMPTS: usize = 5;

/// Lock screen engine — prompts for a password and tracks failed attempts.
#[derive(Debug)]
pub struct LockScreenEngine {
    entered_pin: String,
    max_attempts: usize,
    attempts: usize,
    locale: Locale,
    has_biometrics: bool,
    biometric_type: Option<BiometricType>,
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
            locale: Locale::English,
            has_biometrics: false,
            biometric_type: None,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// Offer biometric unlock when the shell reported the hardware.
    pub fn with_device_capabilities(mut self, capabilities: &DeviceCapabilities) -> Self {
        self.set_device_capabilities(capabilities);
        self
    }

    fn set_device_capabilities(&mut self, capabilities: &DeviceCapabilities) {
        self.has_biometrics = capabilities.has_biometrics;
        self.biometric_type = capabilities.biometric_type.clone();
    }

    /// Record a failed unlock attempt. Returns `true` if max attempts reached (lockout).
    pub fn record_failed_attempt(&mut self) -> bool {
        self.attempts += 1;
        self.attempts >= self.max_attempts
    }

    fn remaining_attempts_message(&self) -> Option<String> {
        if self.attempts > 0 && self.attempts < self.max_attempts {
            let remaining = self.max_attempts - self.attempts;
            Some(if remaining == 1 {
                get_string(self.locale, "lock_screen.attempt_remaining_singular")
            } else {
                get_string_with_args(
                    self.locale,
                    "lock_screen.attempts_remaining_plural",
                    &[("remaining", &remaining.to_string())],
                )
            })
        } else {
            None
        }
    }

    fn lock_glyph(&self) -> Component {
        let spoken = get_string(self.locale, "lock_screen.lock_glyph_a11y");
        Component::InfoPanel {
            id: "lock_glyph".into(),
            icon: Some("lock".into()),
            title: String::new(),
            items: vec![InfoItem {
                icon: Some("lock".into()),
                title: spoken.clone(),
                detail: String::new(),
            }],
            a11y: Some(A11y {
                label: Some(spoken),
                hint: None,
                role: Some(AccessibilityRole::Image),
            }),
        }
    }

    fn password_input(&self) -> Component {
        // A masked free-text field, not a fixed-length PinInput: the app
        // password can be up to 128 chars and alphanumeric, and the duress
        // PIN is typed into this same field — a numeric 6-slot widget locks
        // both out (2026-07-03-lock-screen-pin-cap-locks-out-passwords).
        Component::TextInput {
            id: "pin".into(),
            label: get_string(self.locale, "auth.unlock.field_label"),
            // Echo the entered value: the TUI reconstructs the field from
            // this on every keystroke (no local buffer); masking is the
            // renderer's job via `input_type` (matches `display_name`).
            value: self.entered_pin.clone(),
            placeholder: None,
            max_length: Some(128),
            validation_error: None,
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(get_string(self.locale, "lock_screen.password_entry_a11y")),
                hint: Some(get_string(self.locale, "lock_screen.password_hint")),
                role: None,
            }),
            info_key: None,
        }
    }

    fn attempts_status(&self) -> Option<Component> {
        let remaining = self.remaining_attempts_message()?;
        Some(Component::StatusIndicator {
            id: "attempts".into(),
            icon: Some("warning".into()),
            title: remaining,
            detail: None,
            status: Status::Warning,
            status_label: get_string(self.locale, Status::Warning.label_key()),
            a11y: None,
        })
    }

    fn biometric_label_key(&self) -> &'static str {
        match self.biometric_type {
            Some(BiometricType::FaceId) => "lock_screen.unlock_face_button",
            Some(BiometricType::Fingerprint) => "lock_screen.unlock_fingerprint_button",
            _ => "lock_screen.unlock_biometric_button",
        }
    }

    fn actions(&self) -> Vec<ScreenAction> {
        let mut actions = vec![ScreenAction {
            id: "unlock".into(),
            label: get_string(self.locale, "lock_screen.unlock_button"),
            style: ActionStyle::Primary,
            enabled: !self.entered_pin.is_empty(),
            a11y: None,
        }];
        if self.has_biometrics {
            actions.push(ScreenAction {
                id: "unlock_biometric".into(),
                label: get_string(self.locale, self.biometric_label_key()),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            });
        }
        actions
    }
}

impl WorkflowEngine for LockScreenEngine {
    fn current_screen(&self) -> ScreenModel {
        let mut components = vec![self.lock_glyph(), self.password_input()];
        components.extend(self.attempts_status());

        ScreenModel {
            screen_id: "lock_screen".into(),
            title: get_string(self.locale, "lock.title"),
            subtitle: Some(get_string(self.locale, "lock_screen.password_hint")),
            components,
            contextual_actions: self.actions(),
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

    fn apply_update(&mut self, update: EngineUpdate) -> bool {
        match update {
            EngineUpdate::DeviceCapabilities(capabilities) => {
                self.set_device_capabilities(&capabilities);
                true
            }
            _ => false,
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
                        message: get_string(self.locale, "lock_screen.enter_password_error"),
                    }
                } else {
                    ActionResult::Complete
                }
            }
            // Only an action the batch offered may reach the shell: a press
            // forged without the capability stays inert.
            UserAction::ActionPressed { action_id }
                if action_id == "unlock_biometric" && self.has_biometrics =>
            {
                ActionResult::Commands {
                    commands: vec![Command::RequestBiometricUnlock],
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
