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

/// Component id of the display-name picker (M2 S7 Record E). Rendered
/// in place of the static name `Text` only when `name_options` has
/// more than one candidate.
pub(super) const NAME_PICKER_COMPONENT_ID: &str = "preview_name_picker";
/// Option id for the base (card-default) display name — always the
/// first `name_options` entry and the initial `selected_name_id`.
pub(super) const DEFAULT_NAME_OPTION_ID: &str = "default";

/// Configuration for the field preview screen.
pub(super) struct FieldPreviewConfig {
    /// The full card to preview.
    pub card: ContactCard,
    /// Display name to show (group override or card default). Mirrors
    /// whichever `name_options` entry `selected_name_id` currently
    /// points at.
    pub display_name: String,
    /// Field IDs to share — exactly this set, always resolved by
    /// `group_filter::resolve_exchange_allow` (an empty set shares nothing;
    /// fields default hidden under the field-centric model).
    pub visible_field_ids: HashSet<String>,
    /// Candidate display names for this exchange: the base name
    /// (id `"default"`) plus one deduplicated entry per selected
    /// group carrying a `display_name_override` (M2 S7 Record E). A
    /// picker only renders when this has more than one entry — no
    /// silent precedence rules; the base name is always the initial
    /// pick.
    pub name_options: Vec<DropdownOption>,
    /// The `name_options` entry currently selected. Always `"default"`
    /// until the user picks a different option.
    pub selected_name_id: String,
}

/// Result of handling an action in the field preview engine.
pub(super) enum FieldPreviewResult {
    /// User pressed "Start exchange" — proceed with frozen card.
    StartExchange,
    /// User pressed "Change groups" — go back to group selection.
    ChangeGroups,
    /// User picked a `name_options` entry on the display-name picker
    /// (M2 S7 Record E). Carries the chosen option id — the caller
    /// looks up the matching label to apply.
    NameSelected(String),
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
            let visible = config.visible_field_ids.contains(f.id());
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

    let name_component = if config.name_options.len() > 1 {
        Component::Dropdown {
            id: NAME_PICKER_COMPONENT_ID.into(),
            label: t("exchange.preview.name_picker_label"),
            selected: Some(config.selected_name_id.clone()),
            options: config.name_options.clone(),
            a11y: None,
        }
    } else {
        Component::Text {
            id: "preview_name".into(),
            content: config.display_name.clone(),
            style: TextStyle::Title,
        }
    };

    ScreenModel {
        screen_id: "exchange_field_preview".into(),
        title: t("exchange.preview.title"),
        subtitle: None,
        components: vec![
            name_component,
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
    match action {
        UserAction::ActionPressed { action_id } => match action_id.as_str() {
            "start_exchange" => Some(FieldPreviewResult::StartExchange),
            "change_groups" => Some(FieldPreviewResult::ChangeGroups),
            _ => None,
        },
        UserAction::ListItemSelected {
            component_id,
            item_id,
        } if component_id == NAME_PICKER_COMPONENT_ID => {
            Some(FieldPreviewResult::NameSelected(item_id.clone()))
        }
        _ => None,
    }
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

    fn no_name_choice() -> (Vec<DropdownOption>, String) {
        (vec![], DEFAULT_NAME_OPTION_ID.into())
    }

    /// Allow-list covering every field on the card — the "share the whole
    /// card" arrange now that the preview always receives a resolved set.
    fn all_field_ids(card: &ContactCard) -> HashSet<String> {
        card.fields().iter().map(|f| f.id().to_string()).collect()
    }

    #[test]
    fn preview_shows_all_fields_when_no_visibility_filter() {
        let (name_options, selected_name_id) = no_name_choice();
        let card = sample_card();
        let config = FieldPreviewConfig {
            visible_field_ids: all_field_ids(&card),
            card,
            display_name: "Alice".into(),
            name_options,
            selected_name_id,
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
        let (name_options, selected_name_id) = no_name_choice();
        let config = FieldPreviewConfig {
            card,
            display_name: "Alice".into(),
            visible_field_ids: HashSet::from([email_id.clone()]),
            name_options,
            selected_name_id,
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
        let (name_options, selected_name_id) = no_name_choice();
        let card = sample_card();
        let config = FieldPreviewConfig {
            visible_field_ids: all_field_ids(&card),
            card,
            display_name: "Dr. Egloff".into(),
            name_options,
            selected_name_id,
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
        let (name_options, selected_name_id) = no_name_choice();
        let card = sample_card();
        let config = FieldPreviewConfig {
            visible_field_ids: all_field_ids(&card),
            card,
            display_name: "Alice".into(),
            name_options,
            selected_name_id,
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
        let (name_options, selected_name_id) = no_name_choice();
        let config = FieldPreviewConfig {
            visible_field_ids: all_field_ids(&card),
            card,
            display_name: "Alice".into(),
            name_options,
            selected_name_id,
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);
        let fields = extract_fields(&screen);
        assert_eq!(fields.len(), 2, "Card with 2 fields should show 2 entries");
        assert_eq!(
            fields[0].value, email_value,
            "Field value should be preserved"
        );
    }

    // ── Name picker (M2 S7 Record E) ─────────────────────────────

    // @internal
    #[test]
    fn single_name_option_renders_static_text_not_dropdown() {
        let card = sample_card();
        let config = FieldPreviewConfig {
            visible_field_ids: all_field_ids(&card),
            card,
            display_name: "Alice".into(),
            name_options: vec![DropdownOption {
                id: DEFAULT_NAME_OPTION_ID.into(),
                label: "Alice".into(),
            }],
            selected_name_id: DEFAULT_NAME_OPTION_ID.into(),
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);
        assert!(
            !screen
                .components
                .iter()
                .any(|c| matches!(c, Component::Dropdown { .. })),
            "a single candidate name must not show a picker"
        );
        let name = screen.components.iter().find_map(|c| match c {
            Component::Text { content, .. } => Some(content.as_str()),
            _ => None,
        });
        assert_eq!(name, Some("Alice"));
    }

    // @internal
    #[test]
    fn multiple_name_options_render_dropdown_defaulting_to_base_name() {
        let name_options = vec![
            DropdownOption {
                id: DEFAULT_NAME_OPTION_ID.into(),
                label: "Alice".into(),
            },
            DropdownOption {
                id: "g1".into(),
                label: "Dr. Alice".into(),
            },
        ];
        let card = sample_card();
        let config = FieldPreviewConfig {
            visible_field_ids: all_field_ids(&card),
            card,
            display_name: "Alice".into(),
            name_options: name_options.clone(),
            selected_name_id: DEFAULT_NAME_OPTION_ID.into(),
        };
        let screen = build_field_preview_screen(&config, crate::i18n::Locale::English);
        let dropdown = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::Dropdown {
                    id,
                    selected,
                    options,
                    ..
                } if id == NAME_PICKER_COMPONENT_ID => Some((selected.clone(), options.clone())),
                _ => None,
            })
            .expect("multiple name candidates must render the picker");
        assert_eq!(dropdown.0, Some(DEFAULT_NAME_OPTION_ID.to_string()));
        assert_eq!(dropdown.1, name_options);
        assert!(
            !screen
                .components
                .iter()
                .any(|c| matches!(c, Component::Text { id, .. } if id == "preview_name")),
            "the picker replaces the static name Text, not adds alongside it"
        );
    }

    // @internal
    #[test]
    fn name_picker_selection_returns_name_selected() {
        let result = handle_field_preview_action(&UserAction::ListItemSelected {
            component_id: NAME_PICKER_COMPONENT_ID.into(),
            item_id: "g1".into(),
        });
        assert!(matches!(result, Some(FieldPreviewResult::NameSelected(id)) if id == "g1"));
    }

    // @internal
    #[test]
    fn list_item_selected_on_other_component_returns_none() {
        let result = handle_field_preview_action(&UserAction::ListItemSelected {
            component_id: "some_other_dropdown".into(),
            item_id: "x".into(),
        });
        assert!(result.is_none());
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
