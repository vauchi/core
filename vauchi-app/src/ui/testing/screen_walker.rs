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
        Component::PinInput { id, length, .. } => {
            // Prime the field one digit at a time: PIN engines
            // accumulate a single character per `TextChanged`
            // (`value.len() == 1` — e.g. `duress_pin.rs`,
            // `lock_screen.rs`), so emitting `length` single-char
            // events fills the PIN and clears length / non-empty gates
            // during BFS. A fixed digit means two PIN fields
            // (enter + confirm) fill identically, so confirm-match
            // gates (`confirm_pin == new_pin`) also clear.
            for _ in 0..*length {
                out.push(UserAction::TextChanged {
                    component_id: id.clone(),
                    value: PIN_PRIMING_DIGIT.to_string(),
                });
            }
        }
        Component::ToggleList { id, items, .. } => {
            for item in items {
                out.push(UserAction::ItemToggled {
                    component_id: id.clone(),
                    item_id: item.id.clone(),
                });
            }
        }
        Component::List {
            id,
            items,
            total_count,
            offset,
            window,
            ..
        } => {
            for item in items {
                out.push(UserAction::ListItemSelected {
                    component_id: id.clone(),
                    item_id: item.id.clone(),
                });
                // Per-row actions (archive, hide, delete, undo, …)
                // are surfaced via swipe / long-press / context-menu
                // affordances. Without enumerating them the
                // reachability harness misses every row-action arm
                // an engine declares — silently passing green even
                // when affordances and handlers diverge. See
                // `2026-05-08` audit finding P-11.
                for action in &item.actions {
                    out.push(UserAction::ListItemAction {
                        component_id: id.clone(),
                        item_id: item.id.clone(),
                        action_id: action.id.clone(),
                    });
                }
            }
            // Windowed emissions add the renderer's prefetch dispatch —
            // same P-11 rationale as per-row actions.
            if *total_count > 0 {
                out.push(UserAction::ListWindowRequested {
                    component_id: id.clone(),
                    offset: offset + window,
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
        Component::Row { items, .. } => {
            // A layout container — recurse so nested affordances (e.g.
            // the exchange preview row's `ActionList` switch/cancel)
            // are still walked for reachability.
            for child in items {
                walk_component(child, out);
            }
        }
        Component::SectionedActionList { id, sections } => {
            // Walk every item in every section — affordance shape is the
            // same as ActionList (taps → ListItemSelected). Section
            // grouping is presentation, not a different affordance.
            for section in sections {
                for item in &section.items {
                    out.push(UserAction::ListItemSelected {
                        component_id: id.clone(),
                        item_id: item.id.clone(),
                    });
                }
            }
        }
        Component::Indicator { action_id, .. } => {
            // Indicators are tappable only when action_id is Some — None
            // means display-only and produces no affordance to walk.
            if let Some(action_id) = action_id {
                out.push(UserAction::ActionPressed {
                    action_id: action_id.clone(),
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
        Component::Banner {
            action_id,
            action_label,
            ..
        } => {
            // A Banner renders an action button only when both
            // action_label and action_id are non-empty. Empty
            // action_label is the convention for a passive
            // informational banner (e.g. ACTION_OFFLINE_BANNER) —
            // emitting an ActionPressed for those would create a
            // phantom affordance the renderer never shows.
            if !action_label.is_empty() && !action_id.is_empty() {
                out.push(UserAction::ActionPressed {
                    action_id: action_id.clone(),
                });
            }
        }
        // Components that carry no user affordance at the walker's
        // current scope. Remaining Phase 2 extensions (Slider value
        // change, EditableText edit-toggle) require teaching the
        // walker to diff further non-ActionPressed shapes — see
        // contact_list.rs reachability test docstring.
        Component::Text { .. }
        | Component::FieldList { .. }
        | Component::Preview { .. }
        | Component::InfoPanel { .. }
        | Component::SettingsGroup { .. }
        | Component::StatusIndicator { .. }
        | Component::QrCode { .. }
        | Component::Divider
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

/// Single digit emitted per `PinInput` slot during BFS priming.
/// A fixed digit (not the alphabetic [`PLACEHOLDER_TEXT`]) so that
/// `length` events fill a numeric PIN field, and two PIN fields
/// (enter + confirm) fill to the *same* value — clearing
/// `confirm_pin == new_pin` gates (e.g. `DuressPinEngine`).
pub const PIN_PRIMING_DIGIT: &str = "1";

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
///   `StartLinkExchange` / `OpenContact` / `EditContact` /
///   `OpenUrl` / `BackupExportComplete` / `WipeComplete` are
///   terminal from the engine's POV — BFS stops following that
///   path. (`StartLinkExchange` is the `LinkExchangeEngine`'s
///   Retry handoff: it restarts a fresh link flow via the
///   AppEngine, leaving the current engine's screen space.) Other non-screen-changing results
///   (`ValidationError`, `ShowAlert`, `ShowToast`, `Notify`,
///   `ShowInfoOverlay`, `RequestCamera`, `Commands`,
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
            | ActionResult::StartLinkExchange
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
        ActionListItem, Component, DropdownOption, InputType, Item, ListItemAction,
        ListItemActionKind, ToggleItem,
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

    fn pin_input(id: &str, length: usize) -> Component {
        Component::PinInput {
            id: id.into(),
            label: id.into(),
            length,
            filled: 0,
            masked: true,
            validation_error: None,
            a11y: None,
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
            a11y: None,
        }
    }

    // @internal
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

    // @internal
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

    // @internal
    #[test]
    fn pin_input_emits_one_single_char_text_changed_per_digit() {
        // The walker primes a PinInput one digit at a time: engines
        // accumulate a single character per `TextChanged`
        // (`value.len() == 1`), so `length` events fill the PIN and
        // clear length / non-empty gates during BFS.
        let screen = screen_with(vec![pin_input("pin", 4)], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(actions.len(), 4, "one priming event per digit");
        let values: Vec<&str> = actions
            .iter()
            .map(|a| match a {
                UserAction::TextChanged {
                    component_id,
                    value,
                } => {
                    assert_eq!(component_id, "pin", "primes the PinInput's own id");
                    assert_eq!(
                        value.chars().count(),
                        1,
                        "single char per event so the engine's len==1 accumulation branch fires"
                    );
                    value.as_str()
                }
                other => panic!("expected TextChanged, got {other:?}"),
            })
            .collect();
        // Identical digits so two PIN fields (enter + confirm) fill to
        // the same value and confirm-match gates clear.
        assert!(
            values.windows(2).all(|w| w[0] == w[1]),
            "priming digits must be identical, got {values:?}"
        );
    }

    // @internal
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

    // @internal
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

    fn banner(action_id: &str, action_label: &str) -> Component {
        Component::Banner {
            text: "banner text".into(),
            action_label: action_label.into(),
            action_id: action_id.into(),
            a11y: None,
        }
    }

    // @internal
    #[test]
    fn banner_with_action_emits_action_pressed() {
        let screen = screen_with(vec![banner("open_update_link", "Update")], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![UserAction::ActionPressed {
                action_id: "open_update_link".into(),
            }]
        );
    }

    // @internal
    #[test]
    fn banner_with_empty_action_label_emits_nothing() {
        // ACTION_OFFLINE_BANNER ships with empty action_label so the
        // renderer shows no button — the walker must not emit a
        // phantom affordance.
        let screen = screen_with(vec![banner("offline_banner", "")], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(actions, Vec::<UserAction>::new());
    }

    // @internal
    #[test]
    fn banner_with_empty_action_id_emits_nothing() {
        let screen = screen_with(vec![banner("", "Dismiss")], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(actions, Vec::<UserAction>::new());
    }

    // @internal
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

    // @internal
    #[test]
    fn contact_list_items_emit_list_item_selected() {
        let component = Component::List {
            id: "contacts_list".into(),
            items: vec![Item {
                id: "c-1".into(),
                name: "Alice".into(),
                subtitle: None,
                avatar_initials: "A".into(),
                status: None,
                actions: vec![],
                a11y: None,
            }],
            searchable: false,
            total_count: 0,
            offset: 0,
            window: 0,
        };
        let screen = screen_with(vec![component], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![UserAction::ListItemSelected {
                component_id: "contacts_list".into(),
                item_id: "c-1".into(),
            }]
        );
    }

    // @internal
    #[test]
    fn contact_list_items_emit_list_item_action_per_row_action() {
        let component = Component::List {
            id: "contacts_list".into(),
            items: vec![Item {
                id: "c-1".into(),
                name: "Alice".into(),
                subtitle: None,
                avatar_initials: "A".into(),
                status: None,
                actions: vec![
                    ListItemAction {
                        id: "archive".into(),
                        label: "Archive".into(),
                        kind: ListItemActionKind::Archive,
                        destructive: false,
                    },
                    ListItemAction {
                        id: "delete".into(),
                        label: "Delete".into(),
                        kind: ListItemActionKind::Delete,
                        destructive: true,
                    },
                ],
                a11y: None,
            }],
            searchable: false,
            total_count: 0,
            offset: 0,
            window: 0,
        };
        let screen = screen_with(vec![component], vec![]);
        let actions = walk_actions(&screen);
        assert_eq!(
            actions,
            vec![
                UserAction::ListItemSelected {
                    component_id: "contacts_list".into(),
                    item_id: "c-1".into(),
                },
                UserAction::ListItemAction {
                    component_id: "contacts_list".into(),
                    item_id: "c-1".into(),
                    action_id: "archive".into(),
                },
                UserAction::ListItemAction {
                    component_id: "contacts_list".into(),
                    item_id: "c-1".into(),
                    action_id: "delete".into(),
                },
            ]
        );
    }

    // @internal
    #[test]
    fn windowed_list_emits_window_request_affordance() {
        // Mirrors the P-11 per-row-action lesson: without enumerating
        // the window-request affordance the reachability harness would
        // pass green even if an engine's `ListWindowRequested` arm and
        // the renderer's prefetch dispatch diverge.
        let component = Component::List {
            id: "contacts".into(),
            items: vec![Item {
                id: "c-200".into(),
                name: "Alice".into(),
                subtitle: None,
                avatar_initials: "A".into(),
                status: None,
                actions: vec![],
                a11y: None,
            }],
            searchable: false,
            total_count: 500,
            offset: 200,
            window: 1,
        };
        let screen = screen_with(vec![component], vec![]);
        let actions = walk_actions(&screen);
        assert!(
            actions.contains(&UserAction::ListWindowRequested {
                component_id: "contacts".into(),
                offset: 201,
            }),
            "windowed List must surface the window-request affordance, got {actions:?}"
        );
    }

    // @internal
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

    // @internal
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

    // @internal
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

    // @internal
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
                            a11y: None,
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

    // @internal
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
                        a11y: None,
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
