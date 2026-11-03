// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The owner's recovery passphrase step on the social recovery screen
//! (vauchi/private#796, D16): set or change it, typed twice, or remove it.
//! The AppEngine checks the two entries, has Core judge the strength and
//! keep it, and reports back here.

use crate::i18n::{Locale, get_string};
use crate::ui::*;

pub(crate) const PASSPHRASE_ROW: &str = "recovery_passphrase";
pub(crate) const NEW_INPUT: &str = "recovery_passphrase_new";
pub(crate) const CONFIRM_INPUT: &str = "recovery_passphrase_confirm";
pub(crate) const SAVE_ACTION: &str = "save_recovery_passphrase";
pub(crate) const REMOVE_ACTION: &str = "remove_recovery_passphrase";
pub(crate) const CANCEL_ACTION: &str = "cancel_recovery_passphrase";

#[derive(Clone, Debug, Default)]
pub(crate) struct PassphraseState {
    pub is_set: bool,
    pub new: String,
    pub confirm: String,
    pub refused: Option<String>,
}

impl PassphraseState {
    /// Forgets what was typed and why it was refused.
    pub fn clear(&mut self) {
        self.new.clear();
        self.confirm.clear();
        self.refused = None;
    }

    /// Whether `component_id` is one of the step's two inputs.
    pub fn is_input(component_id: &str) -> bool {
        component_id == NEW_INPUT || component_id == CONFIRM_INPUT
    }

    /// Takes in what the owner typed into one of the two inputs.
    pub fn typed(&mut self, component_id: &str, value: String) {
        match component_id {
            NEW_INPUT => self.new = value,
            CONFIRM_INPUT => self.confirm = value,
            _ => {}
        }
    }

    /// Applies what the AppEngine reports.
    pub fn apply(&mut self, update: RecoveryUpdate) {
        match update {
            RecoveryUpdate::PassphraseSet(is_set) => {
                self.clear();
                self.is_set = is_set;
            }
            RecoveryUpdate::PassphraseRefused(message) => self.refused = Some(message),
            _ => {}
        }
    }
}

/// The row the first step lists, saying whether a passphrase is set.
pub(crate) fn passphrase_row(locale: Locale, state: &PassphraseState) -> ActionListItem {
    let detail = if state.is_set {
        "recovery.passphrase_on"
    } else {
        "recovery.passphrase_off"
    };
    ActionListItem {
        id: PASSPHRASE_ROW.into(),
        label: get_string(locale, "recovery.passphrase_button"),
        icon: None,
        detail: Some(get_string(locale, detail)),
        a11y: None,
        info_key: None,
    }
}

pub(crate) fn passphrase_screen(locale: Locale, state: &PassphraseState) -> ScreenModel {
    let t = |key: &str| get_string(locale, key);
    let input = |id: &str, label_key: &str, value: &str| Component::TextInput {
        id: id.into(),
        label: t(label_key),
        value: value.into(),
        placeholder: None,
        max_length: None,
        validation_error: None,
        input_type: InputType::Password,
        a11y: Some(A11y {
            label: Some(t(label_key)),
            hint: None,
            role: Some(AccessibilityRole::TextField),
        }),
        info_key: None,
    };
    let mut components = vec![
        Component::Text {
            id: "recovery_passphrase_explanation".into(),
            content: t("recovery.passphrase_explanation"),
            style: TextStyle::Body,
            a11y: None,
        },
        input(NEW_INPUT, "recovery.passphrase_new_label", &state.new),
        input(
            CONFIRM_INPUT,
            "recovery.passphrase_confirm_label",
            &state.confirm,
        ),
    ];
    if let Some(refused) = &state.refused {
        components.push(Component::Text {
            id: "recovery_passphrase_error".into(),
            content: refused.clone(),
            style: TextStyle::Body,
            a11y: None,
        });
    }
    let action = |id: &str, label: String, style: ActionStyle| ScreenAction {
        id: id.into(),
        a11y: Some(A11y::labeled(label.clone())),
        label,
        style,
        enabled: true,
    };
    let mut actions = vec![action(
        SAVE_ACTION,
        t("recovery.passphrase_save"),
        ActionStyle::Primary,
    )];
    if state.is_set {
        actions.push(action(
            REMOVE_ACTION,
            t("recovery.passphrase_remove"),
            ActionStyle::Destructive,
        ));
    }
    actions.push(action(
        CANCEL_ACTION,
        t("action.cancel"),
        ActionStyle::Secondary,
    ));
    ScreenModel {
        screen_id: "recovery_status".into(),
        title: t("recovery.passphrase_title"),
        subtitle: None,
        components,
        contextual_actions: actions,
        progress: None,
        ..Default::default()
    }
}
