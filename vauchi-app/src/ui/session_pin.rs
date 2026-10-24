// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The session PIN step in front of a credential change (#777): it stands
//! in for the screen that asked for the change until the PIN that opened
//! this session is entered again.

use crate::i18n::{Locale, get_string};
use crate::ui::device_link_pin::{EnteredPin, PIN_INPUT_ID, PinStepCopy, confirm_pin_screen_for};
use crate::ui::*;

pub const SESSION_PIN_SCREEN_ID: &str = "confirm_session_pin";

pub struct SessionPinEngine {
    locale: Locale,
    title_key: &'static str,
    hint_key: &'static str,
    entered: EnteredPin,
    error: Option<String>,
}

impl SessionPinEngine {
    pub fn new(locale: Locale, title_key: &'static str, hint_key: &'static str) -> Self {
        Self {
            locale,
            title_key,
            hint_key,
            entered: EnteredPin::default(),
            error: None,
        }
    }
}

impl WorkflowEngine for SessionPinEngine {
    fn current_screen(&self) -> ScreenModel {
        confirm_pin_screen_for(
            self.locale,
            PinStepCopy {
                screen_id: SESSION_PIN_SCREEN_ID,
                title_key: self.title_key,
                hint_key: self.hint_key,
            },
            self.entered.as_str(),
            self.error.as_deref(),
        )
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        if let UserAction::TextChanged {
            component_id,
            value,
        } = action
            && component_id == PIN_INPUT_ID
        {
            self.entered.set(value);
            self.error = None;
        }
        ActionResult::UpdateScreen(self.current_screen())
    }

    fn apply_update(&mut self, update: EngineUpdate) -> bool {
        if !matches!(update, EngineUpdate::SessionPinRejected) {
            return false;
        }
        self.entered.clear();
        self.error = Some(get_string(self.locale, "lock_screen.wrong_password"));
        true
    }

    fn engine_output(&self) -> Option<EngineOutput> {
        (!self.entered.as_str().is_empty()).then(|| EngineOutput::SessionPin {
            pin: self.entered.as_str().to_owned(),
        })
    }
}
