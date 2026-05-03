// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group / Label detail engine — shows details of a single contact group
//! (a.k.a. visibility label in some frontends) including per-field
//! visibility toggles. Drives both the iOS `LabelDetailView` /
//! Android `LabelDetailScreen` retirement (Pair 2 of the Pure Humble
//! UI retirement work — see
//! `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`).

use crate::ui::*;

/// One row in the field-visibility toggle list. The engine holds these
/// so that the rendered `Component::ToggleList` stays in sync with the
/// underlying `Group::is_field_visible(field_id)` values fetched at
/// engine construction time.
#[derive(Clone, Debug)]
pub struct GroupFieldVisibility {
    pub field_id: String,
    pub label: String,
    pub value: String,
    pub is_visible: bool,
}

/// Action id prefix for the per-field visibility toggle component.
pub const FIELD_VISIBILITY_COMPONENT_ID: &str = "field_visibility";

/// Engine that displays details of a single contact group / visibility label.
#[derive(Clone, Debug)]
pub struct GroupDetailEngine {
    group_id: String,
    group_name: String,
    members: Vec<Item>,
    fields: Vec<GroupFieldVisibility>,
    pending_delete: bool,
}

impl GroupDetailEngine {
    pub fn new(group_id: String, group_name: String, members: Vec<Item>) -> Self {
        Self {
            group_id,
            group_name,
            members,
            fields: Vec::new(),
            pending_delete: false,
        }
    }

    /// Builder: attach the own-card field set with their per-group
    /// visibility, so the LabelDetail screen can offer field-visibility
    /// toggles. Without this, the engine renders only the contacts
    /// list (matches the legacy `GroupDetail` behavior).
    pub fn with_field_visibility(mut self, fields: Vec<GroupFieldVisibility>) -> Self {
        self.fields = fields;
        self
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components: Vec<Component> = Vec::new();

        components.push(Component::InfoPanel {
            id: "group_info".into(),
            icon: Some("group".into()),
            title: "Group Info".into(),
            items: vec![
                InfoItem {
                    icon: Some("members".into()),
                    title: "Members".into(),
                    detail: format!("{}", self.members.len()),
                },
                InfoItem {
                    icon: Some("eye".into()),
                    title: "Visible Fields".into(),
                    detail: format!("{}", self.visible_field_count()),
                },
            ],
            a11y: Some(A11y {
                label: Some("Group Info".into()),
                hint: None,
                role: Some(AccessibilityRole::Heading),
            }),
        });

        if !self.fields.is_empty() {
            components.push(Component::ToggleList {
                id: FIELD_VISIBILITY_COMPONENT_ID.into(),
                label: "Field Visibility".into(),
                items: self
                    .fields
                    .iter()
                    .map(|f| ToggleItem {
                        id: f.field_id.clone(),
                        label: f.label.clone(),
                        selected: f.is_visible,
                        subtitle: Some(f.value.clone()),
                        a11y: Some(A11y {
                            label: Some(format!("Visibility for {}", f.label)),
                            hint: Some(if f.is_visible {
                                "Visible to this group".into()
                            } else {
                                "Hidden from this group".into()
                            }),
                            role: None,
                        }),
                        info_key: None,
                    })
                    .collect(),
                a11y: Some(A11y {
                    label: Some("Field Visibility toggles".into()),
                    hint: Some(
                        "Toggle which of your fields contacts in this group can see.".into(),
                    ),
                    role: None,
                }),
            });
        }

        components.push(Component::List {
            id: "members".into(),
            items: self.members.clone(),
            searchable: false,
        });

        if self.pending_delete {
            components.push(Component::InlineConfirm {
                id: "delete_group".into(),
                warning: format!(
                    "This will permanently delete \"{}\". Contacts will not be deleted.",
                    self.group_name
                ),
                confirm_text: "Delete Group".into(),
                cancel_text: "Cancel".into(),
                destructive: true,
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "group_detail".into(),
            title: self.group_name.clone(),
            subtitle: None,
            components,
            actions: {
                let mut actions: Vec<ScreenAction> = self
                    .members
                    .iter()
                    .map(|m| ScreenAction {
                        id: format!("preview-as-member:{}", m.id),
                        label: format!("Preview as {}", m.name),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    })
                    .collect();
                actions.push(ScreenAction {
                    id: "rename".into(),
                    label: "Rename".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "delete_group".into(),
                    label: "Delete Group".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                });
                actions
            },
            progress: None,
            ..Default::default()
        }
    }

    fn visible_field_count(&self) -> usize {
        self.fields.iter().filter(|f| f.is_visible).count()
    }
}

impl WorkflowEngine for GroupDetailEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == FIELD_VISIBILITY_COMPONENT_ID => {
                // Find the toggle and flip its in-memory state so the next
                // build_screen reflects the optimistic value. AppEngine
                // routing persists the change via
                // `vauchi.set_group_field_visibility_and_repropagate` and
                // re-fetches the engine afterwards, so the optimistic
                // update is merely a UI smoothness aid.
                let mut new_visible = false;
                if let Some(field) = self.fields.iter_mut().find(|f| f.field_id == item_id) {
                    field.is_visible = !field.is_visible;
                    new_visible = field.is_visible;
                }
                ActionResult::SetGroupFieldVisibility {
                    group_id: self.group_id.clone(),
                    field_id: item_id,
                    visible: new_visible,
                }
            }
            UserAction::ActionPressed { action_id } => {
                if let Some(contact_id) = action_id.strip_prefix("preview-as-member:") {
                    return ActionResult::PreviewAs {
                        contact_id: contact_id.to_string(),
                    };
                }
                match action_id.as_str() {
                    "rename" => ActionResult::ShowFormDialog {
                        dialog_type: "rename_group".into(),
                        context_id: Some(self.group_id.clone()),
                    },
                    "delete_group" => {
                        self.pending_delete = true;
                        ActionResult::UpdateScreen(self.build_screen())
                    }
                    "confirm_delete_group" => {
                        self.pending_delete = false;
                        ActionResult::Complete
                    }
                    "cancel_delete_group" => {
                        self.pending_delete = false;
                        ActionResult::UpdateScreen(self.build_screen())
                    }
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                }
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: covers private build_screen helpers and the
// internal field-visibility partitioning logic that does not need
// pub-API leakage. Cross-crate integration tests live in
// vauchi-core/tests/it/group_detail_engine_tests.rs.
#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, name: &str) -> Item {
        Item {
            id: id.into(),
            name: name.into(),
            subtitle: None,
            avatar_initials: name.chars().next().unwrap_or('?').to_string(),
            status: None,
            searchable_fields: vec![],
            actions: vec![],
            a11y: None,
        }
    }

    fn fld(id: &str, label: &str, value: &str, visible: bool) -> GroupFieldVisibility {
        GroupFieldVisibility {
            field_id: id.into(),
            label: label.into(),
            value: value.into(),
            is_visible: visible,
        }
    }

    // @internal
    #[test]
    fn empty_group_emits_info_panel_and_contact_list() {
        let e = GroupDetailEngine::new("g1".into(), "Work".into(), vec![]);
        let screen = e.current_screen();
        assert_eq!(screen.screen_id, "group_detail");
        assert_eq!(screen.title, "Work");
        // InfoPanel + ContactList = 2 components (no field visibility toggles)
        assert_eq!(screen.components.len(), 2);
        assert!(matches!(&screen.components[0], Component::InfoPanel { .. }));
        assert!(matches!(&screen.components[1], Component::List { .. }));
    }

    // @internal
    #[test]
    fn with_field_visibility_emits_toggle_list() {
        let e = GroupDetailEngine::new("g1".into(), "Work".into(), vec![member("c1", "Alice")])
            .with_field_visibility(vec![
                fld("f1", "Email", "alice@example.com", true),
                fld("f2", "Phone", "+1 555-0100", false),
            ]);
        let screen = e.current_screen();
        // InfoPanel + ToggleList + ContactList
        assert_eq!(screen.components.len(), 3);
        match &screen.components[1] {
            Component::ToggleList { id, items, .. } => {
                assert_eq!(id, FIELD_VISIBILITY_COMPONENT_ID);
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].id, "f1");
                assert!(items[0].selected);
                assert!(!items[1].selected);
                assert_eq!(items[0].subtitle.as_deref(), Some("alice@example.com"));
            }
            other => panic!("expected ToggleList, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn info_panel_includes_visible_field_count() {
        let e =
            GroupDetailEngine::new("g1".into(), "Work".into(), vec![]).with_field_visibility(vec![
                fld("f1", "Email", "x", true),
                fld("f2", "Phone", "y", false),
                fld("f3", "Address", "z", true),
            ]);
        let screen = e.current_screen();
        match &screen.components[0] {
            Component::InfoPanel { items, .. } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].title, "Members");
                assert_eq!(items[0].detail, "0");
                assert_eq!(items[1].title, "Visible Fields");
                assert_eq!(items[1].detail, "2");
            }
            other => panic!("expected InfoPanel, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn toggle_returns_set_group_field_visibility_and_flips_state() {
        let mut e = GroupDetailEngine::new("g1".into(), "Work".into(), vec![])
            .with_field_visibility(vec![fld("f1", "Email", "x", true)]);
        let result = e.handle_action(UserAction::ItemToggled {
            component_id: FIELD_VISIBILITY_COMPONENT_ID.into(),
            item_id: "f1".into(),
        });
        match result {
            ActionResult::SetGroupFieldVisibility {
                group_id,
                field_id,
                visible,
            } => {
                assert_eq!(group_id, "g1");
                assert_eq!(field_id, "f1");
                assert!(!visible);
            }
            other => panic!("expected SetGroupFieldVisibility, got {other:?}"),
        }
        // Optimistic flip
        assert!(!e.fields[0].is_visible);
    }

    // @internal
    #[test]
    fn toggle_unknown_field_id_is_noop() {
        let mut e = GroupDetailEngine::new("g1".into(), "Work".into(), vec![])
            .with_field_visibility(vec![fld("f1", "Email", "x", true)]);
        let result = e.handle_action(UserAction::ItemToggled {
            component_id: FIELD_VISIBILITY_COMPONENT_ID.into(),
            item_id: "nonexistent".into(),
        });
        // Engine still returns the set-visibility ActionResult (default false)
        // — the AppEngine routing layer will silently noop on the underlying
        // vauchi.set_group_field_visibility call when the field id is bogus.
        match result {
            ActionResult::SetGroupFieldVisibility { field_id, .. } => {
                assert_eq!(field_id, "nonexistent");
            }
            other => panic!("expected SetGroupFieldVisibility, got {other:?}"),
        }
        // No internal flip happened
        assert!(e.fields[0].is_visible);
    }

    // @internal
    #[test]
    fn toggle_other_component_id_falls_through() {
        let mut e = GroupDetailEngine::new("g1".into(), "Work".into(), vec![]);
        let result = e.handle_action(UserAction::ItemToggled {
            component_id: "some_other_toggle".into(),
            item_id: "x".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn delete_group_emits_inline_confirm() {
        let mut e = GroupDetailEngine::new("g1".into(), "Work".into(), vec![]);
        let _ = e.handle_action(UserAction::ActionPressed {
            action_id: "delete_group".into(),
        });
        let screen = e.current_screen();
        assert!(
            screen
                .components
                .iter()
                .any(|c| matches!(c, Component::InlineConfirm { .. }))
        );
    }

    // @internal
    #[test]
    fn rename_emits_form_dialog() {
        let mut e = GroupDetailEngine::new("g1".into(), "Work".into(), vec![]);
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "rename".into(),
        });
        match result {
            ActionResult::ShowFormDialog {
                dialog_type,
                context_id,
            } => {
                assert_eq!(dialog_type, "rename_group");
                assert_eq!(context_id, Some("g1".to_string()));
            }
            other => panic!("expected ShowFormDialog, got {other:?}"),
        }
    }
}
