// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group / Label detail engine — shows details of a single contact group
//! (a.k.a. visibility label in some frontends) including per-field
//! visibility toggles. Drives both the iOS `LabelDetailView` /
//! Android `LabelDetailScreen` retirement (Pair 2 of the Pure Humble
//! UI retirement work — see
//! `_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`).

use crate::i18n::{Locale, get_string, get_string_with_args};
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
    /// Seen by this group's members whatever the toggle says, because the
    /// entry is set to Visible and no group governs it.
    pub shown_to_everyone: bool,
    /// Contacts outside this group who see the entry now and would lose it
    /// if this group took it over: a grant makes it group-governed, closed
    /// to everyone without one (field-centric partition, 2026-07-10).
    pub stop_seeing_if_granted: usize,
}

const GRANT_CONFIRM_ID: &str = "grant_entry";
const CONFIRM_GRANT_ACTION: &str = "confirm_grant_entry";
const CANCEL_GRANT_ACTION: &str = "cancel_grant_entry";

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
    pending_grant: Option<String>,
    locale: Locale,
}

impl GroupDetailEngine {
    pub fn new(group_id: String, group_name: String, members: Vec<Item>) -> Self {
        Self {
            group_id,
            group_name,
            members,
            fields: Vec::new(),
            pending_delete: false,
            pending_grant: None,
            locale: Locale::English,
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

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-13).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components: Vec<Component> = Vec::new();

        components.push(Component::InfoPanel {
            id: "group_info".into(),
            icon: Some("people".into()),
            title: self.t("group_detail.group_info_title"),
            items: vec![
                InfoItem {
                    icon: Some("people".into()),
                    title: self.t("group_detail.contacts_label"),
                    detail: format!("{}", self.members.len()),
                },
                InfoItem {
                    icon: Some("eye".into()),
                    title: self.t("group_detail.sees_label"),
                    detail: self.entries_seen(),
                },
            ],
            a11y: Some(A11y {
                label: Some(self.t("group_detail.group_info_title")),
                hint: None,
                role: Some(AccessibilityRole::Heading),
            }),
        });

        if !self.fields.is_empty() {
            components.push(Component::ToggleList {
                id: FIELD_VISIBILITY_COMPONENT_ID.into(),
                label: self.t("group_detail.entries_label"),
                items: self
                    .fields
                    .iter()
                    .map(|f| ToggleItem {
                        id: f.field_id.clone(),
                        label: f.label.clone(),
                        selected: f.is_visible,
                        subtitle: Some(self.entry_subtitle(f)),
                        a11y: Some(A11y {
                            label: Some(get_string_with_args(
                                self.locale,
                                "group_detail.visibility_for_a11y",
                                &[("label", &f.label)],
                            )),
                            hint: Some(self.entry_hint(f)),
                            role: None,
                        }),
                        info_key: None,
                    })
                    .collect(),
                a11y: Some(A11y {
                    label: Some(self.t("group_detail.entries_label")),
                    hint: Some(self.t("group_detail.entries_hint")),
                    role: None,
                }),
            });
        }

        if let Some(confirm) = self.grant_confirmation() {
            components.push(confirm);
        }

        components.push(Component::List {
            id: "members".into(),
            items: self.members.clone(),
            searchable: false,
            total_count: 0,
            offset: 0,
            window: 0,
        });

        if self.pending_delete {
            components.push(Component::InlineConfirm {
                id: "delete_group".into(),
                warning: get_string_with_args(
                    self.locale,
                    "group_detail.delete_warning",
                    &[("name", &self.group_name)],
                ),
                confirm_text: self.t("group_detail.delete_group_button"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: "confirm_delete_group".into(),
                cancel_action_id: "cancel_delete_group".into(),
                destructive: true,
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "group_detail".into(),
            title: self.group_name.clone(),
            subtitle: None,
            components,
            contextual_actions: self.build_actions(),
            progress: None,
            ..Default::default()
        }
    }

    fn build_actions(&self) -> Vec<ScreenAction> {
        let mut actions: Vec<ScreenAction> = self
            .members
            .iter()
            .map(|m| ScreenAction {
                id: format!("preview-as-member:{}", m.id),
                label: get_string_with_args(
                    self.locale,
                    "group_detail.preview_as",
                    &[("name", &m.name)],
                ),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            })
            .collect();
        actions.push(ScreenAction {
            id: "rename".into(),
            label: self.t("group_detail.rename_button"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });
        actions.push(ScreenAction {
            id: "delete_group".into(),
            label: self.t("group_detail.delete_group_button"),
            style: ActionStyle::Destructive,
            enabled: true,
            a11y: None,
        });
        actions
    }

    fn grant_confirmation(&self) -> Option<Component> {
        let field_id = self.pending_grant.as_ref()?;
        let field = self.fields.iter().find(|f| &f.field_id == field_id)?;
        let others = field.stop_seeing_if_granted;
        let warning = if others == 1 {
            get_string_with_args(
                self.locale,
                "group_detail.grant_scope_warning_singular",
                &[("group", &self.group_name)],
            )
        } else {
            get_string_with_args(
                self.locale,
                "group_detail.grant_scope_warning_plural",
                &[("group", &self.group_name), ("count", &others.to_string())],
            )
        };
        Some(Component::InlineConfirm {
            id: GRANT_CONFIRM_ID.into(),
            warning,
            confirm_text: get_string_with_args(
                self.locale,
                "group_detail.grant_scope_confirm",
                &[("group", &self.group_name)],
            ),
            cancel_text: self.t("action.cancel"),
            confirm_action_id: CONFIRM_GRANT_ACTION.into(),
            cancel_action_id: CANCEL_GRANT_ACTION.into(),
            destructive: false,
            a11y: None,
        })
    }

    /// Flips the toggle optimistically; AppEngine routing persists the
    /// change via `set_group_field_visibility_and_repropagate` and
    /// re-fetches the engine afterwards.
    fn set_visible(&mut self, field_id: String) -> ActionResult {
        let mut new_visible = false;
        if let Some(field) = self.fields.iter_mut().find(|f| f.field_id == field_id) {
            field.is_visible = !field.is_visible;
            new_visible = field.is_visible;
        }
        ActionResult::SetGroupFieldVisibility {
            group_id: self.group_id.clone(),
            field_id,
            visible: new_visible,
        }
    }

    fn entries_seen(&self) -> String {
        let n = self
            .fields
            .iter()
            .filter(|f| f.is_visible || f.shown_to_everyone)
            .count();
        if n == 1 {
            self.t("group_detail.entry_count_singular")
        } else {
            get_string_with_args(
                self.locale,
                "group_detail.entry_count_plural",
                &[("count", &n.to_string())],
            )
        }
    }

    fn entry_subtitle(&self, field: &GroupFieldVisibility) -> String {
        if field.shown_to_everyone && !field.is_visible {
            get_string_with_args(
                self.locale,
                "group_detail.shown_to_everyone_subtitle",
                &[("value", &field.value)],
            )
        } else {
            field.value.clone()
        }
    }

    fn entry_hint(&self, field: &GroupFieldVisibility) -> String {
        if field.is_visible {
            self.t("group_detail.visible_to_group_hint")
        } else if field.shown_to_everyone {
            self.t("group_detail.shown_to_everyone_hint")
        } else {
            self.t("group_detail.hidden_from_group_hint")
        }
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
                let takes_from_others = self.fields.iter().any(|f| {
                    f.field_id == item_id
                        && !f.is_visible
                        && f.shown_to_everyone
                        && f.stop_seeing_if_granted > 0
                });
                if takes_from_others {
                    self.pending_grant = Some(item_id);
                    return ActionResult::UpdateScreen(self.build_screen());
                }
                self.set_visible(item_id)
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
                    CONFIRM_GRANT_ACTION => match self.pending_grant.take() {
                        Some(field_id) => self.set_visible(field_id),
                        None => ActionResult::UpdateScreen(self.build_screen()),
                    },
                    CANCEL_GRANT_ACTION => {
                        self.pending_grant = None;
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
            initials: name.chars().next().unwrap_or('?').to_string(),
            status: None,
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
            shown_to_everyone: false,
            stop_seeing_if_granted: 0,
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
    fn info_panel_counts_contacts_and_entries_seen() {
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
                assert_eq!(items[0].title, "Contacts");
                assert_eq!(items[0].detail, "0");
                assert_eq!(items[1].title, "Sees");
                assert_eq!(items[1].detail, "2 entries");
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
