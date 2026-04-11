// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups engine — displays and manages contact groups with Members/Visibility modes.

use crate::ui::*;

/// Which aspect of groups is being managed.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum GroupsMode {
    /// Show group membership (which contacts are in each group).
    Members,
    /// Show field visibility (which of your fields each group can see).
    Visibility,
}

/// Summary info for a group.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub member_count: usize,
    pub visible_field_count: usize,
}

/// Engine that displays and manages contact groups.
pub struct GroupsEngine {
    groups: Vec<GroupInfo>,
    mode: GroupsMode,
    pending_delete_group_id: Option<String>,
}

impl GroupsEngine {
    pub fn new(groups: Vec<GroupInfo>, mode: GroupsMode) -> Self {
        Self {
            groups,
            mode,
            pending_delete_group_id: None,
        }
    }

    /// Returns the current mode.
    pub fn mode(&self) -> &GroupsMode {
        &self.mode
    }

    /// Returns the group ID pending deletion, if any.
    pub fn pending_delete_group_id(&self) -> Option<&str> {
        self.pending_delete_group_id.as_deref()
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Mode toggle (radio-style: only one selected at a time)
        components.push(Component::ToggleList {
            id: "mode_toggle".into(),
            label: "View Mode".into(),
            items: vec![
                ToggleItem {
                    id: "members".into(),
                    label: "Members".into(),
                    selected: self.mode == GroupsMode::Members,
                    subtitle: Some("Which contacts are in each group".into()),
                },
                ToggleItem {
                    id: "visibility".into(),
                    label: "Visibility".into(),
                    selected: self.mode == GroupsMode::Visibility,
                    subtitle: Some("Which of your fields each group sees".into()),
                },
            ],
        });

        // Group list with mode-dependent detail text
        let items: Vec<ActionListItem> = self
            .groups
            .iter()
            .map(|g| {
                let detail = match self.mode {
                    GroupsMode::Members => {
                        let n = g.member_count;
                        if n == 1 {
                            "1 member".into()
                        } else {
                            format!("{n} members")
                        }
                    }
                    GroupsMode::Visibility => {
                        let n = g.visible_field_count;
                        if n == 1 {
                            "1 visible field".into()
                        } else {
                            format!("{n} visible fields")
                        }
                    }
                };
                ActionListItem {
                    id: g.id.clone(),
                    label: g.name.clone(),
                    icon: Some("group".into()),
                    detail: Some(detail),
                }
            })
            .collect();

        components.push(Component::ActionList {
            id: "groups".into(),
            items,
        });

        if self.pending_delete_group_id.is_some() {
            components.push(Component::InlineConfirm {
                id: "delete_group".into(),
                warning:
                    "This will permanently delete the selected group. Members will be unassigned."
                        .into(),
                confirm_text: "Delete Group".into(),
                cancel_text: "Cancel".into(),
                destructive: true,
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "groups_list".into(),
            title: "Groups".into(),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "new_group".into(),
                    label: "New Group".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "rename_group".into(),
                    label: "Rename Group".into(),
                    style: ActionStyle::Secondary,
                    enabled: !self.groups.is_empty(),
                },
                ScreenAction {
                    id: "delete_group".into(),
                    label: "Delete Group".into(),
                    style: ActionStyle::Secondary,
                    enabled: !self.groups.is_empty(),
                },
                ScreenAction {
                    id: "merge_groups".into(),
                    label: "Merge Groups".into(),
                    style: ActionStyle::Secondary,
                    enabled: self.groups.len() >= 2,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for GroupsEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            // Mode toggle: switch between Members and Visibility
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "mode_toggle" => {
                self.mode = match item_id.as_str() {
                    "members" => GroupsMode::Members,
                    "visibility" => GroupsMode::Visibility,
                    _ => return ActionResult::UpdateScreen(self.build_screen()),
                };
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Group selected from list — reuses OpenContact to signal "open detail".
            // AppEngine routes this to GroupDetail when the current screen is Groups.
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == "groups" => ActionResult::OpenContact {
                contact_id: item_id,
            },
            // Screen-level actions
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "delete_group" => {
                    if let Some(group) = self.groups.first() {
                        self.pending_delete_group_id = Some(group.id.clone());
                    }
                    ActionResult::UpdateScreen(self.build_screen())
                }
                "confirm_delete_group" => {
                    // Keep pending_delete_group_id for handle_completion to read
                    ActionResult::Complete
                }
                "cancel_delete_group" => {
                    self.pending_delete_group_id = None;
                    ActionResult::UpdateScreen(self.build_screen())
                }
                "new_group" => ActionResult::ShowFormDialog {
                    dialog_type: "create_group".into(),
                    context_id: None,
                },
                "rename_group" => {
                    // Use first group if none specifically selected
                    let group_id = self.groups.first().map(|g| g.id.clone());
                    ActionResult::ShowFormDialog {
                        dialog_type: "rename_group".into(),
                        context_id: group_id,
                    }
                }
                "merge_groups" => ActionResult::ShowAlert {
                    title: "Coming Soon".into(),
                    message: "Group merging will be available in a future update.".into(),
                },
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: Tests access private GroupsEngine internals
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_groups() -> Vec<GroupInfo> {
        vec![
            GroupInfo {
                id: "g1".into(),
                name: "Family".into(),
                member_count: 5,
                visible_field_count: 3,
            },
            GroupInfo {
                id: "g2".into(),
                name: "Work".into(),
                member_count: 12,
                visible_field_count: 1,
            },
        ]
    }

    #[test]
    fn test_default_members_mode_shows_member_counts() {
        let engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let screen = engine.current_screen();

        assert_eq!(screen.screen_id, "groups_list");
        assert_eq!(screen.title, "Groups");

        // ActionList should have 2 groups
        let action_list = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::ActionList { id, .. } if id == "groups"))
            .expect("should have groups ActionList");
        if let Component::ActionList { items, .. } = action_list {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].detail.as_deref(), Some("5 members"));
            assert_eq!(items[1].detail.as_deref(), Some("12 members"));
        }
    }

    #[test]
    fn test_visibility_mode_shows_field_counts() {
        let engine = GroupsEngine::new(sample_groups(), GroupsMode::Visibility);
        let screen = engine.current_screen();

        let action_list = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::ActionList { id, .. } if id == "groups"))
            .expect("should have groups ActionList");
        if let Component::ActionList { items, .. } = action_list {
            assert_eq!(items[0].detail.as_deref(), Some("3 visible fields"));
            assert_eq!(items[1].detail.as_deref(), Some("1 visible field"));
        }
    }

    #[test]
    fn test_mode_toggle_switches_to_visibility() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        assert_eq!(engine.mode(), &GroupsMode::Members);

        let result = engine.handle_action(UserAction::ItemToggled {
            component_id: "mode_toggle".into(),
            item_id: "visibility".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
        assert_eq!(engine.mode(), &GroupsMode::Visibility);
    }

    #[test]
    fn test_mode_toggle_switches_to_members() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Visibility);

        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "mode_toggle".into(),
            item_id: "members".into(),
        });
        assert_eq!(engine.mode(), &GroupsMode::Members);
    }

    #[test]
    fn test_mode_toggle_component_reflects_current_mode() {
        let engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let screen = engine.current_screen();

        let toggle = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::ToggleList { id, .. } if id == "mode_toggle"))
            .expect("should have mode toggle");
        if let Component::ToggleList { items, .. } = toggle {
            assert_eq!(items.len(), 2);
            assert!(items[0].selected); // Members selected
            assert!(!items[1].selected); // Visibility not selected
        }
    }

    #[test]
    fn test_group_list_item_selected_opens_detail() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);

        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "groups".into(),
            item_id: "g1".into(),
        });
        assert_eq!(
            result,
            ActionResult::OpenContact {
                contact_id: "g1".into()
            }
        );
    }

    #[test]
    fn test_actions_disabled_when_no_groups() {
        let engine = GroupsEngine::new(vec![], GroupsMode::Members);
        let screen = engine.current_screen();

        let new_action = screen.actions.iter().find(|a| a.id == "new_group").unwrap();
        assert!(new_action.enabled);

        let rename = screen
            .actions
            .iter()
            .find(|a| a.id == "rename_group")
            .unwrap();
        assert!(!rename.enabled);

        let delete = screen
            .actions
            .iter()
            .find(|a| a.id == "delete_group")
            .unwrap();
        assert!(!delete.enabled);

        let merge = screen
            .actions
            .iter()
            .find(|a| a.id == "merge_groups")
            .unwrap();
        assert!(!merge.enabled);
    }

    #[test]
    fn test_merge_requires_at_least_two_groups() {
        let one_group = vec![GroupInfo {
            id: "g1".into(),
            name: "Family".into(),
            member_count: 3,
            visible_field_count: 2,
        }];
        let engine = GroupsEngine::new(one_group, GroupsMode::Members);
        let screen = engine.current_screen();

        let merge = screen
            .actions
            .iter()
            .find(|a| a.id == "merge_groups")
            .unwrap();
        assert!(!merge.enabled);
    }

    #[test]
    fn test_merge_enabled_with_two_groups() {
        let engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let screen = engine.current_screen();

        let merge = screen
            .actions
            .iter()
            .find(|a| a.id == "merge_groups")
            .unwrap();
        assert!(merge.enabled);
    }

    #[test]
    fn test_singular_member_count() {
        let groups = vec![GroupInfo {
            id: "g1".into(),
            name: "Solo".into(),
            member_count: 1,
            visible_field_count: 0,
        }];
        let engine = GroupsEngine::new(groups, GroupsMode::Members);
        let screen = engine.current_screen();

        if let Component::ActionList { items, .. } = &screen.components[1] {
            assert_eq!(items[0].detail.as_deref(), Some("1 member"));
        }
    }

    // @internal
    #[test]
    fn test_new_group_action_returns_form_dialog() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "new_group".into(),
        });
        assert!(
            matches!(result, ActionResult::ShowFormDialog { dialog_type, .. } if dialog_type == "create_group")
        );
    }

    // @internal
    #[test]
    fn test_rename_group_action_returns_form_dialog() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "rename_group".into(),
        });
        assert!(
            matches!(result, ActionResult::ShowFormDialog { dialog_type, .. } if dialog_type == "rename_group")
        );
    }

    #[test]
    fn delete_group_shows_inline_confirm() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "delete_group".into(),
        });
        let screen = match result {
            ActionResult::UpdateScreen(s) => s,
            other => panic!("Expected UpdateScreen, got {:?}", other),
        };
        let has_inline_confirm = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InlineConfirm { destructive, .. } if *destructive));
        assert!(
            has_inline_confirm,
            "delete_group should show a destructive InlineConfirm"
        );
    }

    #[test]
    fn confirm_delete_group_returns_complete() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "delete_group".into(),
        });
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm_delete_group".into(),
        });
        assert!(
            matches!(result, ActionResult::Complete),
            "confirm should return Complete"
        );
    }

    #[test]
    fn cancel_delete_group_removes_inline_confirm() {
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "delete_group".into(),
        });
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel_delete_group".into(),
        });
        let screen = match result {
            ActionResult::UpdateScreen(s) => s,
            other => panic!("Expected UpdateScreen, got {:?}", other),
        };
        let has_inline_confirm = screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InlineConfirm { .. }));
        assert!(!has_inline_confirm, "cancel should remove InlineConfirm");
    }
}
