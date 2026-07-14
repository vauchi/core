// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared, core-driven exchange **success** screen.
//!
//! Every exchange engine (multi-stage Glance/Hover/TapHoverShake and —
//! as they adopt it — BLE, NFC, Link) renders the same terminal success
//! chrome from an [`ExchangeSuccessSummary`] the AppEngine assembles when
//! the exchange finalizes: who you exchanged with, what they shared, what
//! they can now see of *your* card, and which groups the new contact
//! joined. Per ADR-021/043 the frontends are pure renderers of this
//! `ScreenModel`.
//!
//! Design / scope: `_private/docs/problems/2026-06-04-exchange-terminal-screens`.

use crate::i18n::Locale;
use crate::ui::component::icon_for_field_type;
use crate::ui::*;

/// Everything the success screen shows. Mode-agnostic so all exchange
/// engines share one terminal screen; the AppEngine builds it at finalize
/// from the just-persisted contact + the user's own card visibility.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExchangeSuccessSummary {
    /// The peer's display name (empty if their card carried none).
    pub peer_name: String,
    /// Fields the peer shared with us, as `(field_type, label, value)`.
    pub received_fields: Vec<(String, String, String)>,
    /// Labels of *our* card fields this new contact can now see.
    pub my_visible_fields: Vec<String>,
    /// Names of the groups the contact was added to (may be empty).
    pub group_names: Vec<String>,
}

/// Build the shared success `ScreenModel`.
///
/// `screen_id` is the calling engine's screen id (so per-engine frontend
/// routing is unchanged); `done_action_id` is its "done" affordance id.
pub fn build_exchange_success_screen(
    screen_id: &str,
    title: impl Into<String>,
    done_action_id: &str,
    summary: &ExchangeSuccessSummary,
    locale: Locale,
) -> ScreenModel {
    let t = |key: &str| crate::i18n::get_string(locale, key);
    // Second-person possessive-friendly handle for the narration.
    let they = if summary.peer_name.is_empty() {
        "they".to_string()
    } else {
        summary.peer_name.clone()
    };

    let mut components: Vec<Component> = vec![Component::StatusIndicator {
        id: "status".into(),
        icon: Some("checkmark.circle".into()),
        title: t("exchange.terminal.complete"),
        detail: Some(if summary.peer_name.is_empty() {
            t("exchange.success_title")
        } else {
            format!("Exchanged with {}", summary.peer_name)
        }),
        status: Status::Success,
        status_label: t(Status::Success.label_key()),
        a11y: None,
    }];

    // What they shared with us — the received card fields, read-only.
    if !summary.received_fields.is_empty() {
        components.push(Component::Text {
            id: "received_header".into(),
            content: format!("What {they} shared"),
            style: TextStyle::Caption,
        });
        let fields = summary
            .received_fields
            .iter()
            .map(|(field_type, label, value)| Field {
                id: label.clone(),
                field_type: field_type.clone(),
                label: label.clone(),
                value: value.clone(),
                icon: icon_for_field_type(field_type).to_string(),
                visibility: UiFieldVisibility::Shown,
                a11y: None,
            })
            .collect();
        components.push(Component::FieldList {
            id: "received_fields".into(),
            title: t("fields.a11y_contact_fields"),
            fields,
            visibility_mode: VisibilityMode::ReadOnly,
            available_scopes: Vec::new(),
            a11y: None,
        });
    }

    // Which group(s) the new contact joined.
    if !summary.group_names.is_empty() {
        components.push(Component::InfoPanel {
            id: "added_to_groups".into(),
            icon: Some("folder".into()),
            title: t("exchange.success.added_to"),
            items: summary
                .group_names
                .iter()
                .map(|g| InfoItem {
                    icon: None,
                    title: g.clone(),
                    detail: String::new(),
                })
                .collect(),
            a11y: None,
        });
    }

    // What they can now see of MY card (the applied visibility).
    let visibility_detail = if summary.my_visible_fields.is_empty() {
        "Only your name for now — add fields to a shared group to share more.".to_string()
    } else {
        summary.my_visible_fields.join(", ")
    };
    components.push(Component::InfoPanel {
        id: "my_visibility".into(),
        icon: Some("eye".into()),
        title: format!("What {they} can see of your card"),
        items: vec![InfoItem {
            icon: None,
            title: visibility_detail,
            detail: String::new(),
        }],
        a11y: None,
    });

    ScreenModel::new(
        screen_id,
        title,
        components,
        vec![ScreenAction {
            id: done_action_id.into(),
            label: t("action.done"),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: None,
        }],
    )
}

// INLINE_TEST_REQUIRED: exercises the pure ScreenModel shape the shared
// builder emits (component ids + per-section presence), co-located with
// the builder so the contract stays in one place.
#[cfg(test)]
mod tests {
    use super::*;

    // The wire `Component` enum has no `id()` accessor; match the
    // variants the success builder emits.
    fn component_ids(screen: &ScreenModel) -> Vec<String> {
        screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::StatusIndicator { id, .. }
                | Component::Text { id, .. }
                | Component::FieldList { id, .. }
                | Component::InfoPanel { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect()
    }

    // @internal
    #[test]
    fn full_summary_renders_all_sections() {
        let summary = ExchangeSuccessSummary {
            peer_name: "Bob".into(),
            received_fields: vec![("email".into(), "Email".into(), "bob@example.com".into())],
            my_visible_fields: vec!["Phone".into(), "Website".into()],
            group_names: vec!["Friends".into()],
        };
        let screen = build_exchange_success_screen(
            "exchange_success",
            "Done",
            "done",
            &summary,
            Locale::English,
        );
        let ids = component_ids(&screen);
        assert!(ids.contains(&"status".to_string()), "status present");
        assert!(
            ids.contains(&"received_fields".to_string()),
            "received fields present"
        );
        assert!(
            ids.contains(&"added_to_groups".to_string()),
            "group section present"
        );
        assert!(
            ids.contains(&"my_visibility".to_string()),
            "visibility section present"
        );
        assert_eq!(screen.actions.len(), 1, "one Done action");
        assert_eq!(screen.actions[0].id, "done");
    }

    // @internal
    #[test]
    fn empty_summary_omits_optional_sections_but_keeps_status_and_visibility() {
        let screen = build_exchange_success_screen(
            "exchange_success",
            "Done",
            "done",
            &ExchangeSuccessSummary::default(),
            Locale::English,
        );
        let ids = component_ids(&screen);
        assert!(ids.contains(&"status".to_string()), "status always present");
        assert!(
            !ids.contains(&"received_fields".to_string()),
            "no received-fields section when the peer shared nothing",
        );
        assert!(
            !ids.contains(&"added_to_groups".to_string()),
            "no group section when no group was assigned",
        );
        // Visibility section always renders (empty -> explanatory hint).
        assert!(
            ids.contains(&"my_visibility".to_string()),
            "visibility always present"
        );
    }
}
