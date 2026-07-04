// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Field preview engine — read-only preview of the card about to
//! be shared, filtered by group visibility.
//!
//! Shows which fields will be shared and which are excluded,
//! with display name override applied if set.

use std::collections::HashSet;

use crate::ui::*;
use vauchi_core::contact_card::ContactCard;

/// Configuration for the field preview screen.
pub(super) struct FieldPreviewConfig {
    /// The full card to preview.
    pub card: ContactCard,
    /// Display name to show (group override or card default).
    pub display_name: String,
    /// Field IDs to share. `None` = share all (no group filter);
    /// `Some(set)` = share exactly `set` (an empty set shares nothing —
    /// default-closed, so a group exposing no fields shares none).
    pub visible_field_ids: Option<HashSet<String>>,
}

/// Result of handling an action in the field preview engine.
pub(super) enum FieldPreviewResult {
    /// User pressed "Start exchange" — proceed with frozen card.
    StartExchange,
    /// User pressed "Change groups" — go back to group selection.
    ChangeGroups,
}

/// Build a field preview screen from the config.
pub(super) fn build_field_preview_screen(
    config: &FieldPreviewConfig,
    locale: crate::i18n::Locale,
) -> ScreenModel {
    let t = |key: &str| crate::i18n::get_string(locale, key);
    let fields: Vec<Field> = config
        .card
        .fields()
        .iter()
        .map(|f| {
            let visible = match &config.visible_field_ids {
                None => true,
                Some(allow) => allow.contains(f.id()),
            };
            let visibility = if visible {
                UiFieldVisibility::Shown
            } else {
                UiFieldVisibility::Hidden
            };
            let field_type_str = format!("{:?}", f.field_type());
            Field {
                id: f.id().to_string(),
                label: f.label().to_string(),
                value: f.value().to_string(),
                icon: crate::ui::component::icon_for_field_type(&field_type_str).into(),
                field_type: field_type_str,
                a11y: Some(A11y {
                    label: Some(format!("{}: {}", f.label(), f.value())),
                    hint: match visibility {
                        UiFieldVisibility::Shown => None,
                        UiFieldVisibility::Hidden => {
                            Some("This field is hidden from contacts".into())
                        }
                        UiFieldVisibility::Groups(_) => Some("Visible to specific groups".into()),
                    },
                    role: None,
                }),
                visibility,
            }
        })
        .collect();

    ScreenModel {
        screen_id: "exchange_field_preview".into(),
        title: t("exchange.preview.title"),
        subtitle: None,
        components: vec![
            Component::Text {
                id: "preview_name".into(),
                content: config.display_name.clone(),
                style: TextStyle::Title,
            },
            Component::FieldList {
                id: "preview_fields".into(),
                fields,
                visibility_mode: VisibilityMode::ReadOnly,
                available_groups: vec![],
                a11y: Some(A11y {
                    label: Some(t("exchange.preview.fields_a11y")),
                    hint: None,
                    role: None,
                }),
            },
        ],
        actions: vec![
            ScreenAction {
                id: "start_exchange".into(),
                label: t("exchange.preview.start"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            },
            ScreenAction {
                id: "change_groups".into(),
                label: t("exchange.preview.change_groups"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
        ],
        ..Default::default()
    }
}

/// Handle a user action on the field preview screen.
pub(super) fn handle_field_preview_action(action: &UserAction) -> Option<FieldPreviewResult> {
    if let UserAction::ActionPressed { action_id } = action {
        match action_id.as_str() {
            "start_exchange" => return Some(FieldPreviewResult::StartExchange),
            "change_groups" => return Some(FieldPreviewResult::ChangeGroups),
            _ => {}
        }
    }
    None
}

// INLINE_TEST_REQUIRED: Tests access private FieldPreviewConfig, FieldPreviewResult, and builder functions
#[cfg(test)]
mod tests {
    use super::*;
    use vauchi_core::contact_card::{ContactField, FieldType};

    fn sample_card() -> ContactCard {
        let mut card = ContactCard::new("Alice");
        card.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "alice@example.com",
            0,
        ))
        .unwrap();
        card.add_field(ContactField::new(
            FieldType::Phone,
            "phone",
            "+1234567890",
            0,
        ))
        .unwrap();
        card
    }

    #[test]
    fn preview_shows_all_fields_when_no_visibility_filter() {
        let config = FieldPreviewConfig {
            card: sample_card(),
            display_name: "Alice".into(),
            visible_field_ids: None,
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);
        assert_eq!(screen.screen_id, "exchange_field_preview");

        let fields = extract_fields(&screen);
        assert_eq!(fields.len(), 2);
        assert!(
            fields
                .iter()
                .all(|f| f.visibility == UiFieldVisibility::Shown),
            "All fields should be visible"
        );
    }

    #[test]
    fn preview_dims_excluded_fields() {
        let card = sample_card();
        let email_id = card.fields()[0].id().to_string();
        let config = FieldPreviewConfig {
            card,
            display_name: "Alice".into(),
            visible_field_ids: Some(HashSet::from([email_id.clone()])),
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);

        let fields = extract_fields(&screen);
        let email = fields.iter().find(|f| f.id == email_id).unwrap();
        assert_eq!(
            email.visibility,
            UiFieldVisibility::Shown,
            "Visible field should be Shown"
        );

        let phone = fields.iter().find(|f| f.id != email_id).unwrap();
        assert_eq!(
            phone.visibility,
            UiFieldVisibility::Hidden,
            "Excluded field should be Hidden"
        );
    }

    #[test]
    fn preview_shows_display_name_override() {
        let config = FieldPreviewConfig {
            card: sample_card(),
            display_name: "Dr. Egloff".into(),
            visible_field_ids: None,
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);

        let name = screen.components.iter().find_map(|c| match c {
            Component::Text { content, .. } => Some(content.as_str()),
            _ => None,
        });
        assert_eq!(name, Some("Dr. Egloff"));
    }

    #[test]
    fn start_exchange_action_returns_start() {
        let result = handle_field_preview_action(&UserAction::ActionPressed {
            action_id: "start_exchange".into(),
        });
        assert!(matches!(result, Some(FieldPreviewResult::StartExchange)));
    }

    #[test]
    fn change_groups_action_returns_change() {
        let result = handle_field_preview_action(&UserAction::ActionPressed {
            action_id: "change_groups".into(),
        });
        assert!(matches!(result, Some(FieldPreviewResult::ChangeGroups)));
    }

    #[test]
    fn unknown_action_returns_none() {
        let result = handle_field_preview_action(&UserAction::ActionPressed {
            action_id: "something_else".into(),
        });
        assert!(result.is_none());
    }

    #[test]
    fn preview_has_both_actions() {
        let config = FieldPreviewConfig {
            card: sample_card(),
            display_name: "Alice".into(),
            visible_field_ids: None,
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);
        assert_eq!(screen.actions.len(), 2);
        assert_eq!(screen.actions[0].id, "start_exchange");
        assert_eq!(screen.actions[1].id, "change_groups");
    }

    #[test]
    fn preview_with_populated_card_shows_field_values() {
        let card = sample_card();
        let email_value = card.fields()[0].value().to_string();
        let config = FieldPreviewConfig {
            card,
            display_name: "Alice".into(),
            visible_field_ids: None,
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);
        let fields = extract_fields(&screen);
        assert_eq!(fields.len(), 2, "Card with 2 fields should show 2 entries");
        assert_eq!(
            fields[0].value, email_value,
            "Field value should be preserved"
        );
    }

    fn extract_fields(screen: &ScreenModel) -> &[Field] {
        screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::FieldList { fields, .. } => Some(fields.as_slice()),
                _ => None,
            })
            .unwrap()
    }
}
