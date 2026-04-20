// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Enumerate every `UserAction` a `ScreenModel` can emit.
//!
//! Foundation for the Layer 1 reachability harness described in
//! `_private/docs/planning/todo/2026-04-20-frontend-correctness-strategy-plan.md`
//! Phase 0 Task 0.1.

use std::collections::HashSet;

use crate::ui::action::{ActionResult, UserAction};
use crate::ui::component::Component;
use crate::ui::engine::WorkflowEngine;
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
            // Non-empty placeholder so engines with non-empty
            // validators (e.g. `handle_default_name` rejecting a
            // blank display_name) can advance past text-entry
            // screens during BFS.
            out.push(UserAction::TextChanged {
                component_id: id.clone(),
                value: PLACEHOLDER_TEXT.to_string(),
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

/// Placeholder text emitted for every `TextInput` / `EditableText`
/// affordance. Non-empty so engines that reject empty strings
/// (e.g. `OnboardingEngine::handle_default_name` →
/// `ValidationError`) still advance during BFS. Short enough that
/// `max_length` constraints rarely reject it.
pub const PLACEHOLDER_TEXT: &str = "x";

/// Safety cap on BFS node expansion to bound harness runtime.
///
/// An engine that produces a novel `screen_id` on every action
/// would otherwise make the traversal unbounded. Real engines
/// cycle; this is a correctness guard, not a budget target.
const MAX_BFS_SCREENS: usize = 256;

/// Enumerate every `ScreenModel` reachable from the engine
/// produced by `factory()`, by BFS over action outcomes.
///
/// The factory must return a fresh engine on each call — the
/// harness replays the path-to-a-screen from scratch before
/// exploring that screen's affordances, so no `Clone` bound is
/// required on the engine itself.
///
/// # What counts as "reached"
///
/// - An action on the current screen yields a new
///   `ScreenModel` whose `screen_id` has not been seen before.
///   The harness follows `ActionResult::UpdateScreen`,
///   `NavigateTo`, and any variant that leaves the engine on a
///   new rendered screen.
/// - `Complete` / `CompleteWith` / `StartDeviceLink` /
///   `StartBackupImport` / `OpenContact` / `EditContact` /
///   `OpenUrl` / `BackupExportComplete` / `WipeComplete` are
///   terminal from the engine's POV — BFS stops following that
///   path. Other non-screen-changing results
///   (`ValidationError`, `ShowAlert`, `ShowToast`, `Notify`,
///   `ShowInfoOverlay`, `RequestCamera`, `ExchangeCommands`,
///   `PreviewAs`, `ShowContactPicker`, `VerifyFingerprint`,
///   `ShowFormDialog`, `OpenEntryDetail`) are treated as no-ops
///   for the traversal (the engine's `current_screen()` is
///   re-queried; unchanged screens are not re-queued).
///
/// # Ordering and limits
///
/// Screens are returned in BFS discovery order. Traversal caps
/// at `MAX_BFS_SCREENS` screens to guarantee termination on
/// misbehaving engines.
pub fn all_reachable_screens<E, F>(factory: F) -> Vec<ScreenModel>
where
    E: WorkflowEngine,
    F: Fn() -> E,
{
    let mut screens: Vec<ScreenModel> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut frontier_paths: Vec<Vec<UserAction>> = vec![Vec::new()];

    while let Some(path) = frontier_paths.pop() {
        if screens.len() >= MAX_BFS_SCREENS {
            break;
        }

        let mut engine = factory();
        let mut bailed = false;
        for action in &path {
            if is_terminal(&engine.handle_action(action.clone())) {
                bailed = true;
                break;
            }
        }
        if bailed {
            continue;
        }

        let screen = engine.current_screen();
        if !visited.insert(screen.screen_id.clone()) {
            continue;
        }
        screens.push(screen.clone());

        // Split affordances into priming (fills inputs, toggles
        // state — does not typically navigate) vs. triggers
        // (buttons, list selections — may navigate to a new
        // screen). Each child path applies every priming action
        // on the current screen first, then the trigger. Without
        // this, engines that gate navigation on a non-empty
        // `TextChanged` would deadlock on the first entry screen.
        let walked = walk_actions(&screen);
        let (priming, triggers): (Vec<_>, Vec<_>) = walked.into_iter().partition(is_priming_action);

        for trigger in triggers {
            let mut next = path.clone();
            next.extend(priming.iter().cloned());
            next.push(trigger);
            frontier_paths.push(next);
        }
    }

    screens
}

fn is_priming_action(action: &UserAction) -> bool {
    matches!(
        action,
        UserAction::TextChanged { .. }
            | UserAction::ItemToggled { .. }
            | UserAction::SearchChanged { .. }
            | UserAction::SettingsToggled { .. }
            | UserAction::FieldVisibilityChanged { .. }
            | UserAction::SliderChanged { .. }
    )
}

/// Returns `true` when the engine has left its own screen space
/// — further BFS along this path explores a different engine's
/// territory and is out of scope for single-engine reachability.
fn is_terminal(result: &ActionResult) -> bool {
    matches!(
        result,
        ActionResult::Complete
            | ActionResult::CompleteWith { .. }
            | ActionResult::StartDeviceLink
            | ActionResult::StartBackupImport
            | ActionResult::OpenContact { .. }
            | ActionResult::EditContact { .. }
            | ActionResult::OpenUrl { .. }
            | ActionResult::BackupExportComplete { .. }
            | ActionResult::WipeComplete
    )
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
    fn text_input_emits_text_changed_with_placeholder() {
        let screen = screen_with(vec![text_input("display_name")], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![UserAction::TextChanged {
                component_id: "display_name".into(),
                value: PLACEHOLDER_TEXT.into(),
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
                    value: PLACEHOLDER_TEXT.into(),
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
    fn all_reachable_screens_yields_single_screen_from_no_op_engine() {
        use crate::notification_types::PendingNotification;
        use crate::ui::action::ActionResult;

        struct StaticEngine {
            screen: ScreenModel,
        }

        impl WorkflowEngine for StaticEngine {
            fn current_screen(&self) -> ScreenModel {
                self.screen.clone()
            }
            fn handle_action(&mut self, _action: UserAction) -> ActionResult {
                ActionResult::UpdateScreen(self.screen.clone())
            }
            fn poll_notifications(&mut self) -> Vec<PendingNotification> {
                Vec::new()
            }
        }

        let s = screen_with(vec![], vec![screen_action("anything")]);
        let reachable = all_reachable_screens(|| StaticEngine { screen: s.clone() });
        assert_eq!(reachable, vec![s]);
    }

    #[test]
    fn all_reachable_screens_discovers_second_screen_via_action() {
        use crate::notification_types::PendingNotification;
        use crate::ui::action::ActionResult;

        struct TwoScreenEngine {
            step: u8,
        }

        impl WorkflowEngine for TwoScreenEngine {
            fn current_screen(&self) -> ScreenModel {
                match self.step {
                    0 => ScreenModel::new(
                        "start",
                        "start",
                        vec![],
                        vec![ScreenAction {
                            id: "go".into(),
                            label: "go".into(),
                            style: ActionStyle::Primary,
                            enabled: true,
                        }],
                    ),
                    _ => ScreenModel::new("end", "end", vec![], vec![]),
                }
            }
            fn handle_action(&mut self, action: UserAction) -> ActionResult {
                if let UserAction::ActionPressed { action_id } = &action
                    && action_id == "go"
                {
                    self.step = 1;
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            fn poll_notifications(&mut self) -> Vec<PendingNotification> {
                Vec::new()
            }
        }

        let reachable = all_reachable_screens(|| TwoScreenEngine { step: 0 });
        let ids: Vec<_> = reachable.iter().map(|s| s.screen_id.clone()).collect();
        assert_eq!(ids, vec!["start".to_string(), "end".to_string()]);
    }

    #[test]
    fn all_reachable_screens_stops_at_terminal_action_results() {
        use crate::notification_types::PendingNotification;
        use crate::ui::action::ActionResult;

        struct TerminalEngine;

        impl WorkflowEngine for TerminalEngine {
            fn current_screen(&self) -> ScreenModel {
                ScreenModel::new(
                    "lock",
                    "lock",
                    vec![],
                    vec![ScreenAction {
                        id: "done".into(),
                        label: "done".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    }],
                )
            }
            fn handle_action(&mut self, _action: UserAction) -> ActionResult {
                ActionResult::Complete
            }
            fn poll_notifications(&mut self) -> Vec<PendingNotification> {
                Vec::new()
            }
        }

        let reachable = all_reachable_screens(|| TerminalEngine);
        assert_eq!(reachable.len(), 1);
        assert_eq!(reachable[0].screen_id, "lock");
    }
}
