// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The question a contact detail asks as the owner turns on recovery
//! trust once a guardian group exists: which group the contact joins
//! (vauchi/private#796, step 1g, D17). Recovery takes one guardian of
//! every group, so the owner groups people who can stand in for each
//! other as each one is added, never afterwards.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::{A11y, ActionListItem, Component, TextStyle};

/// The picker's list; a row is `group:<n>` or [`NEW_GROUP_ROW`].
pub const GUARDIAN_GROUP_LIST: &str = "guardian_group";
pub const NEW_GROUP_ROW: &str = "new_group";
pub const GUARDIAN_GROUP_CANCEL_LIST: &str = "guardian_group_cancel";
pub const CANCEL_ROW: &str = "cancel";
pub const GROUP_ROW_PREFIX: &str = "group:";

/// An existing guardian group as the picker lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardianGroupRow {
    pub group_id: u8,
    pub member_names: Vec<String>,
}

/// The question, the rows to pick from and a way out, for `contact_name`.
pub fn guardian_group_components(
    locale: Locale,
    contact_name: &str,
    groups: &[GuardianGroupRow],
) -> Vec<Component> {
    let name = [("name", contact_name)];
    let mut rows: Vec<ActionListItem> = groups
        .iter()
        .map(|group| {
            let number = group.group_id.to_string();
            let label = get_string_with_args(
                locale,
                "contact_detail.guardian_group_label",
                &[("number", &number)],
            );
            let members = group.member_names.join(", ");
            ActionListItem {
                id: format!("{GROUP_ROW_PREFIX}{number}"),
                a11y: Some(A11y::labeled(format!("{label}: {members}"))),
                label,
                icon: None,
                detail: Some(members),
                info_key: None,
            }
        })
        .collect();
    let new_label = get_string(locale, "contact_detail.guardian_group_new");
    let new_detail =
        get_string_with_args(locale, "contact_detail.guardian_group_new_detail", &name);
    rows.push(ActionListItem {
        id: NEW_GROUP_ROW.into(),
        a11y: Some(A11y::labeled(format!("{new_label}: {new_detail}"))),
        label: new_label,
        icon: None,
        detail: Some(new_detail),
        info_key: None,
    });
    let cancel = get_string(locale, "action.cancel");
    vec![
        Component::Text {
            id: "guardian_group_question".into(),
            content: get_string_with_args(locale, "contact_detail.guardian_group_question", &name),
            style: TextStyle::Subtitle,
            a11y: None,
        },
        Component::Text {
            id: "guardian_group_explanation".into(),
            content: get_string(locale, "contact_detail.guardian_group_explanation"),
            style: TextStyle::Body,
            a11y: None,
        },
        Component::ActionList {
            id: GUARDIAN_GROUP_LIST.into(),
            items: rows,
        },
        Component::Text {
            id: "guardian_group_minimum".into(),
            content: get_string(locale, "contact_detail.guardian_group_minimum"),
            style: TextStyle::Caption,
            a11y: None,
        },
        Component::ButtonList {
            id: GUARDIAN_GROUP_CANCEL_LIST.into(),
            items: vec![ActionListItem {
                id: CANCEL_ROW.into(),
                a11y: Some(A11y::labeled(cancel.clone())),
                label: cancel,
                icon: None,
                detail: None,
                info_key: None,
            }],
        },
    ]
}
