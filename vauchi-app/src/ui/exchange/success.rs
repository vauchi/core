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
    /// Whether this exchange updated a contact we already had, rather than
    /// adding a new one. Re-running the ceremony with the same peer is a
    /// supported flow (it re-syncs and can raise trust), and the terminal
    /// screen must not claim a contact was added when none was.
    pub is_reconnection: bool,
}

/// Locale key for an exchange's terminal title.
///
/// Re-running the ceremony with a peer we already hold is a supported
/// flow — it re-syncs the cards and can raise trust — so the terminal
/// screen has to say which of the two happened. Every engine shares this
/// so the wording cannot drift between modes.
pub(crate) fn completion_title_key(is_reconnection: bool) -> &'static str {
    if is_reconnection {
        "link_exchange.contact_updated_title"
    } else {
        "link_exchange.contact_added_title"
    }
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
            crate::i18n::get_string_with_args(
                locale,
                "exchange.success.exchanged_with",
                &[("name", summary.peer_name.as_str())],
            )
        }),
        status: Status::Success,
        status_label: t(Status::Success.label_key()),
        a11y: None,
    }];

    // The living-connection promise. This is the only line on the surface that
    // says something NameDrop and AirDrop cannot: the copy stays current, and
    // both sides keep control of it.
    //
    // Says nothing about physical presence on purpose. Neither the mutual QR
    // scan nor the audio proximity attestation distance-bounds — audio is a
    // location-limited out-of-band channel, not a distance-bounding primitive,
    // and both are relayable in principle. Claiming "verified in person" here
    // would assert more than the protocol establishes.
    components.push(Component::Text {
        a11y: None,
        id: "stays_updated".into(),
        content: if summary.peer_name.is_empty() {
            t("exchange.success.stays_updated_generic")
        } else {
            crate::i18n::get_string_with_args(
                locale,
                "exchange.success.stays_updated",
                &[("name", they.as_str())],
            )
        },
        style: TextStyle::Body,
    });

    // What they shared with us — the received card fields, read-only.
    if !summary.received_fields.is_empty() {
        components.push(Component::Text {
            a11y: None,
            id: "received_header".into(),
            content: if summary.peer_name.is_empty() {
                t("exchange.success.what_they_shared_generic")
            } else {
                crate::i18n::get_string_with_args(
                    locale,
                    "exchange.success.what_they_shared",
                    &[("name", they.as_str())],
                )
            },
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
                visibility_label: crate::ui::component::visibility_label(
                    &UiFieldVisibility::Shown,
                    locale,
                ),
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
            is_reconnection: false,
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
        assert_eq!(screen.contextual_actions.len(), 1, "one Done action");
        assert_eq!(screen.contextual_actions[0].id, "done");
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
