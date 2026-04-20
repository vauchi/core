// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Enumerate every `UserAction` a `ScreenModel` can emit.
//!
//! Foundation for the Layer 1 reachability harness described in
//! `_private/docs/planning/todo/2026-04-20-frontend-correctness-strategy-plan.md`
//! Phase 0 Task 0.1.

use crate::ui::action::UserAction;
use crate::ui::component::Component;
use crate::ui::screen::ScreenModel;

/// Returns every `UserAction` the frontends can legitimately emit
/// when rendering `screen`.
///
/// "Legitimately" here means: an affordance exists in the rendered
/// screen that, when exercised by a user, produces the action. It
/// does not cover speculative actions a misbehaving frontend might
/// invent.
///
/// Callers use this together with the engine's `handle_action` to
/// assert reachability invariants (every action the screen offers
/// has a handler arm; every handler arm is reachable from some
/// screen).
pub fn walk_actions(screen: &ScreenModel) -> Vec<UserAction> {
    let mut actions = Vec::new();

    for screen_action in &screen.actions {
        actions.push(UserAction::ActionPressed {
            action_id: screen_action.id.clone(),
        });
    }

    for component in &screen.components {
        walk_component(component, &mut actions);
    }

    actions
}

fn walk_component(component: &Component, out: &mut Vec<UserAction>) {
    match component {
        Component::TextInput { id, .. } | Component::EditableText { id, .. } => {
            out.push(UserAction::TextChanged {
                component_id: id.clone(),
                value: String::new(),
            });
        }
        Component::ToggleList { id, items, .. } => {
            for item in items {
                out.push(UserAction::ItemToggled {
                    component_id: id.clone(),
                    item_id: item.id.clone(),
                });
            }
        }
        Component::ContactList { contacts, .. } => {
            for contact in contacts {
                out.push(UserAction::ListItemSelected {
                    component_id: "contacts".into(),
                    item_id: contact.id.clone(),
                });
            }
        }
        Component::ActionList { id, items } => {
            for item in items {
                out.push(UserAction::ListItemSelected {
                    component_id: id.clone(),
                    item_id: item.id.clone(),
                });
            }
        }
        Component::Dropdown { id, options, .. } => {
            for option in options {
                out.push(UserAction::ListItemSelected {
                    component_id: id.clone(),
                    item_id: option.id.clone(),
                });
            }
        }
        Component::InlineConfirm { id, .. } => {
            out.push(UserAction::ActionPressed {
                action_id: format!("confirm_{id}"),
            });
            out.push(UserAction::ActionPressed {
                action_id: format!("cancel_{id}"),
            });
        }
        // Components that carry no user affordance at the walker's
        // current scope. Phase 1 extends coverage as new reachability
        // invariants are added.
        Component::Text { .. }
        | Component::FieldList { .. }
        | Component::CardPreview { .. }
        | Component::InfoPanel { .. }
        | Component::SettingsGroup { .. }
        | Component::StatusIndicator { .. }
        | Component::PinInput { .. }
        | Component::QrCode { .. }
        | Component::Divider
        | Component::Banner { .. }
        | Component::AvatarPreview { .. }
        | Component::Slider { .. } => {}
    }
}

/// Enumerate every `ScreenModel` reachable from `initial`.
///
/// Phase 0 stub: returns the initial screen only. Phase 1 replaces
/// this with a BFS traversal that feeds `walk_actions` back through
/// the engine's `handle_action` and collects every `ScreenModel`
/// produced.
pub fn all_reachable_screens(initial: ScreenModel) -> Vec<ScreenModel> {
    vec![initial]
}

// INLINE_TEST_REQUIRED: walker arms map 1:1 to private `Component` variants
// and the `confirm_<id>` / `cancel_<id>` convention; co-locating tests keeps
// arm coverage and convention in one diff so future `Component` additions
// fail here instead of silently losing walker coverage.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::component::{
        ActionListItem, Component, ContactItem, DropdownOption, InputType, ToggleItem,
    };
    use crate::ui::screen::{ActionStyle, ScreenAction, ScreenModel};

    fn screen_with(components: Vec<Component>, actions: Vec<ScreenAction>) -> ScreenModel {
        ScreenModel::new("test", "Test", components, actions)
    }

    fn text_input(id: &str) -> Component {
        Component::TextInput {
            id: id.into(),
            label: id.into(),
            value: String::new(),
            placeholder: None,
            max_length: None,
            validation_error: None,
            input_type: InputType::Text,
            a11y: None,
            info_key: None,
        }
    }

    fn toggle_list(id: &str, item_ids: &[&str]) -> Component {
        Component::ToggleList {
            id: id.into(),
            label: id.into(),
            items: item_ids
                .iter()
                .map(|iid| ToggleItem {
                    id: (*iid).into(),
                    label: (*iid).into(),
                    selected: false,
                    subtitle: None,
                    a11y: None,
                    info_key: None,
                })
                .collect(),
            a11y: None,
        }
    }

    fn inline_confirm(id: &str) -> Component {
        Component::InlineConfirm {
            id: id.into(),
            warning: "warn".into(),
            confirm_text: "OK".into(),
            cancel_text: "Cancel".into(),
            destructive: false,
            a11y: None,
        }
    }

    fn screen_action(id: &str) -> ScreenAction {
        ScreenAction {
            id: id.into(),
            label: id.into(),
            style: ActionStyle::Primary,
            enabled: true,
        }
    }

    #[test]
    fn screen_actions_become_action_pressed() {
        let screen = screen_with(
            vec![],
            vec![screen_action("continue"), screen_action("skip")],
        );
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![
                UserAction::ActionPressed {
                    action_id: "continue".into(),
                },
                UserAction::ActionPressed {
                    action_id: "skip".into(),
                },
            ]
        );
    }

    #[test]
    fn text_input_emits_text_changed() {
        let screen = screen_with(vec![text_input("display_name")], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![UserAction::TextChanged {
                component_id: "display_name".into(),
                value: String::new(),
            }]
        );
    }

    #[test]
    fn toggle_list_emits_item_toggled_per_item() {
        let screen = screen_with(vec![toggle_list("groups", &["family", "work"])], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![
                UserAction::ItemToggled {
                    component_id: "groups".into(),
                    item_id: "family".into(),
                },
                UserAction::ItemToggled {
                    component_id: "groups".into(),
                    item_id: "work".into(),
                },
            ]
        );
    }

    #[test]
    fn inline_confirm_emits_confirm_and_cancel_action_pressed() {
        let screen = screen_with(vec![inline_confirm("emergency_wipe")], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![
                UserAction::ActionPressed {
                    action_id: "confirm_emergency_wipe".into(),
                },
                UserAction::ActionPressed {
                    action_id: "cancel_emergency_wipe".into(),
                },
            ]
        );
    }

    #[test]
    fn action_list_items_emit_list_item_selected() {
        let component = Component::ActionList {
            id: "actions".into(),
            items: vec![
                ActionListItem {
                    id: "edit".into(),
                    label: "Edit".into(),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                },
                ActionListItem {
                    id: "archive".into(),
                    label: "Archive".into(),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                },
            ],
        };
        let screen = screen_with(vec![component], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![
                UserAction::ListItemSelected {
                    component_id: "actions".into(),
                    item_id: "edit".into(),
                },
                UserAction::ListItemSelected {
                    component_id: "actions".into(),
                    item_id: "archive".into(),
                },
            ]
        );
    }

    #[test]
    fn contact_list_items_emit_list_item_selected() {
        let component = Component::ContactList {
            id: "contacts_list".into(),
            contacts: vec![ContactItem {
                id: "c-1".into(),
                name: "Alice".into(),
                subtitle: None,
                avatar_initials: "A".into(),
                status: None,
                searchable_fields: vec![],
                a11y: None,
            }],
            searchable: false,
        };
        let screen = screen_with(vec![component], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![UserAction::ListItemSelected {
                component_id: "contacts".into(),
                item_id: "c-1".into(),
            }]
        );
    }

    #[test]
    fn dropdown_options_emit_list_item_selected() {
        let component = Component::Dropdown {
            id: "theme".into(),
            label: "Theme".into(),
            selected: None,
            options: vec![
                DropdownOption {
                    id: "light".into(),
                    label: "Light".into(),
                },
                DropdownOption {
                    id: "dark".into(),
                    label: "Dark".into(),
                },
            ],
            a11y: None,
        };
        let screen = screen_with(vec![component], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![
                UserAction::ListItemSelected {
                    component_id: "theme".into(),
                    item_id: "light".into(),
                },
                UserAction::ListItemSelected {
                    component_id: "theme".into(),
                    item_id: "dark".into(),
                },
            ]
        );
    }

    #[test]
    fn mixed_screen_returns_actions_in_order() {
        let screen = screen_with(
            vec![
                text_input("name"),
                toggle_list("groups", &["a"]),
                inline_confirm("delete"),
            ],
            vec![screen_action("save")],
        );
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![
                UserAction::ActionPressed {
                    action_id: "save".into(),
                },
                UserAction::TextChanged {
                    component_id: "name".into(),
                    value: String::new(),
                },
                UserAction::ItemToggled {
                    component_id: "groups".into(),
                    item_id: "a".into(),
                },
                UserAction::ActionPressed {
                    action_id: "confirm_delete".into(),
                },
                UserAction::ActionPressed {
                    action_id: "cancel_delete".into(),
                },
            ]
        );
    }

    #[test]
    fn all_reachable_screens_returns_initial_only_in_phase_0() {
        let screen = screen_with(vec![], vec![]);
        let reachable = all_reachable_screens(screen.clone());
        assert_eq!(reachable, vec![screen]);
    }
}
