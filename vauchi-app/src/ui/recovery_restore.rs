// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The new phone's restore step on the recovery screen (vauchi/private#796,
//! step 3b): it says how many guardians handed the backup back, asks for
//! the owner's passphrase when the backup is sealed under one, and then
//! says how many contacts came back.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

pub(crate) const RESTORE_ROW: &str = "restore_contacts";
pub(crate) const RESTORE_NOW_ACTION: &str = "restore_now";
pub(crate) const CANCEL_RESTORE_ACTION: &str = "cancel_restore";
pub(crate) const PASSPHRASE_INPUT: &str = "restore_passphrase";

/// What the restore step knows; filled by the AppEngine, which reads the
/// returned backups and runs the restore.
#[derive(Clone, Debug, Default)]
pub(crate) struct RestoreState {
    pub same_identity: bool,
    pub handed_back: usize,
    pub needs_passphrase: bool,
    pub passphrase: String,
    pub refused: Option<String>,
    pub restored: Option<usize>,
}

/// Applies a restore result to the step's state; `RestoreOffered` starts
/// it, the others land only once it exists.
pub(crate) fn apply_restore_update(slot: &mut Option<RestoreState>, update: RecoveryUpdate) {
    match update {
        RecoveryUpdate::RestoreOffered {
            handed_back,
            needs_passphrase,
        } => {
            *slot = Some(RestoreState {
                handed_back,
                needs_passphrase,
                ..RestoreState::default()
            });
        }
        RecoveryUpdate::Restored(count) => {
            if let Some(state) = slot {
                state.restored = Some(count);
                state.refused = None;
                state.passphrase.clear();
            }
        }
        RecoveryUpdate::RestoreRefused(message) => {
            if let Some(state) = slot {
                state.refused = Some(message);
            }
        }
        _ => {}
    }
}

/// The contacts not yet moved to this phone, by name, with whether each
/// did not answer in 14 days; `None` when every contact moved.
pub(crate) fn rekey_waiting_panel(locale: Locale, waiting: &[(String, bool)]) -> Option<Component> {
    if waiting.is_empty() {
        return None;
    }
    let items = waiting
        .iter()
        .map(|(name, not_answered)| InfoItem {
            icon: None,
            title: name.clone(),
            detail: get_string(
                locale,
                if *not_answered {
                    "recovery.rekey_not_answered"
                } else {
                    "recovery.rekey_asking"
                },
            ),
        })
        .collect();
    let title = get_string(locale, "recovery.rekey_waiting_title");
    Some(Component::InfoPanel {
        id: "rekey_waiting".into(),
        icon: None,
        a11y: Some(A11y::labeled(title.clone())),
        title,
        items,
    })
}

/// `key_one` for one, `key` with `{count}` otherwise.
fn counted(locale: Locale, key: &str, count: usize) -> String {
    if count == 1 {
        get_string(locale, &format!("{key}_one"))
    } else {
        get_string_with_args(locale, key, &[("count", &count.to_string())])
    }
}

/// The row the recovery screen's first step offers once a guardian has
/// handed a backup back.
pub(crate) fn restore_row(locale: Locale, state: &RestoreState) -> ActionListItem {
    ActionListItem {
        id: RESTORE_ROW.into(),
        label: get_string(locale, "recovery.restore_button"),
        icon: None,
        detail: Some(counted(
            locale,
            "recovery.restore_button_detail",
            state.handed_back,
        )),
        a11y: None,
        info_key: None,
    }
}

pub(crate) fn restore_screen(locale: Locale, state: &RestoreState) -> ScreenModel {
    let t = |key: &str| get_string(locale, key);
    let (components, actions) = match state.restored {
        Some(count) => (
            vec![Component::StatusIndicator {
                id: "restore_done".into(),
                icon: Some("checkmark.circle.fill".into()),
                title: counted(locale, "recovery.restore_done", count),
                detail: (!state.same_identity).then(|| t("recovery.restore_done_detail")),
                status: Status::Success,
                status_label: t(Status::Success.label_key()),
                a11y: None,
            }],
            vec![action("done", t("action.done"), ActionStyle::Primary)],
        ),
        None => (
            ask_components(locale, state),
            vec![
                action(
                    RESTORE_NOW_ACTION,
                    t("recovery.restore_now"),
                    ActionStyle::Primary,
                ),
                action(
                    CANCEL_RESTORE_ACTION,
                    t("action.cancel"),
                    ActionStyle::Secondary,
                ),
            ],
        ),
    };
    ScreenModel {
        screen_id: "recovery_status".into(),
        title: t("recovery.restore_title"),
        subtitle: None,
        components,
        contextual_actions: actions,
        progress: None,
        ..Default::default()
    }
}

fn ask_components(locale: Locale, state: &RestoreState) -> Vec<Component> {
    let t = |key: &str| get_string(locale, key);
    let mut components = vec![Component::Text {
        id: "restore_handed_back".into(),
        content: counted(locale, "recovery.restore_handed_back", state.handed_back),
        style: TextStyle::Body,
        a11y: None,
    }];
    if state.needs_passphrase {
        components.push(Component::TextInput {
            id: PASSPHRASE_INPUT.into(),
            label: t("recovery.restore_passphrase_label"),
            value: state.passphrase.clone(),
            placeholder: None,
            max_length: None,
            validation_error: None,
            input_type: InputType::Password,
            a11y: Some(A11y {
                label: Some(t("recovery.restore_passphrase_label")),
                hint: Some(t("recovery.restore_passphrase_hint")),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        });
    }
    if let Some(refused) = &state.refused {
        components.push(Component::Text {
            id: "restore_error".into(),
            content: refused.clone(),
            style: TextStyle::Body,
            a11y: None,
        });
    }
    components
}

fn action(id: &str, label: String, style: ActionStyle) -> ScreenAction {
    ScreenAction {
        id: id.into(),
        a11y: Some(A11y::labeled(label.clone())),
        label,
        style,
        enabled: true,
    }
}
