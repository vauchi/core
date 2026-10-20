// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The PIN step that opens device linking (#469): a completed link hands
//! the new device the identity, so the PIN is asked again even in an
//! unlocked app, the way the lock screen asks for it.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use zeroize::{Zeroize, Zeroizing};

pub const CONFIRM_PIN_ACTION_ID: &str = "confirm_pin";
pub const PIN_INPUT_ID: &str = "pin";

/// The typed PIN: never printed, wiped when replaced or dropped. Every
/// clone is a `Zeroizing` too, so no copy outlives its owner unwiped.
#[derive(Clone, Default)]
pub(crate) struct EnteredPin(Zeroizing<String>);

impl EnteredPin {
    pub(crate) fn set(&mut self, value: String) {
        self.0 = Zeroizing::new(value);
    }

    pub(crate) fn clear(&mut self) {
        self.0.zeroize();
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for EnteredPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

pub(crate) fn confirm_pin_screen(
    locale: Locale,
    entered: &str,
    error: Option<&str>,
) -> ScreenModel {
    confirm_pin_screen_for(
        locale,
        PinStepCopy {
            screen_id: "link_confirm_pin",
            title_key: "device_link.title",
            hint_key: "devices.link.confirm_password_hint",
        },
        entered,
        error,
    )
}

/// What a PIN re-confirmation step is called and what it says, so every
/// step asking for the session PIN again shares one screen.
pub(crate) struct PinStepCopy {
    pub(crate) screen_id: &'static str,
    pub(crate) title_key: &'static str,
    pub(crate) hint_key: &'static str,
}

pub(crate) fn confirm_pin_screen_for(
    locale: Locale,
    copy: PinStepCopy,
    entered: &str,
    error: Option<&str>,
) -> ScreenModel {
    let t = |key: &str| get_string(locale, key);
    ScreenModel {
        screen_id: copy.screen_id.into(),
        title: t(copy.title_key),
        subtitle: Some(t(copy.hint_key)),
        components: vec![Component::TextInput {
            id: PIN_INPUT_ID.into(),
            label: t("auth.unlock.field_label"),
            value: entered.to_string(),
            placeholder: None,
            max_length: Some(128),
            validation_error: error.map(str::to_string),
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(t("lock_screen.password_entry_a11y")),
                hint: Some(t(copy.hint_key)),
                role: None,
            }),
            info_key: None,
        }],
        contextual_actions: vec![
            ScreenAction {
                id: CONFIRM_PIN_ACTION_ID.into(),
                label: t("action.continue"),
                style: ActionStyle::Primary,
                enabled: !entered.is_empty(),
                a11y: None,
            },
            ScreenAction {
                id: "cancel".into(),
                label: t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
        ],
        ..Default::default()
    }
}
