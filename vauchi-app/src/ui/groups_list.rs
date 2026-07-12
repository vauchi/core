// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups engine — displays and manages contact groups with Members/Visibility modes.

use crate::i18n::{Locale, get_string, get_string_with_args};
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

/// Engine that displays contact groups.
///
/// Rename / delete are per-group actions reached by tapping a group row →
/// `GroupDetail` (which owns them unambiguously). The list itself offers
/// only "New Group": the former list-level Rename/Delete operated on
/// `groups.first()` regardless of which group the user meant — a
/// wrong-group bug — and "Merge Groups" was an unimplemented stub. See
/// `2026-06-05-screen-ux-declutter`.
pub struct GroupsEngine {
    groups: Vec<GroupInfo>,
    mode: GroupsMode,
    /// One-shot model-shift education, shown the first time the owner
    /// creates a group while already having exchanged contacts: entries
    /// assigned to a group become visible only to its members; unassigned
    /// entries keep their Visible/Hidden toggle (Decision 3,
    /// 2026-07-05-ungrouped-contacts-default-open).
    education_banner: bool,
    locale: Locale,
}

impl GroupsEngine {
    pub fn new(groups: Vec<GroupInfo>, mode: GroupsMode) -> Self {
        Self {
            groups,
            mode,
            education_banner: false,
            locale: Locale::English,
        }
    }

    /// Enable the one-shot first-group education banner.
    pub fn with_education_banner(mut self, show: bool) -> Self {
        self.education_banner = show;
        self
    }

    /// Decides the one-shot banner and burns the flag: true exactly once —
    /// when the first group exists alongside ≥1 contact and the education
    /// has never been shown. Storage failures skip the banner rather than
    /// re-showing it forever.
    #[cfg(feature = "network-rustls")]
    pub fn first_group_education(vauchi: &vauchi_core::api::Vauchi, group_count: usize) -> bool {
        if group_count != 1 || vauchi.contact_count().unwrap_or(0) == 0 {
            return false;
        }
        let Ok(mut flags) = vauchi.load_settings_flags() else {
            return false;
        };
        if flags.first_group_education_shown {
            return false;
        }
        flags.first_group_education_shown = true;
        vauchi.save_settings_flags(&flags).is_ok()
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Returns the current mode.
    pub fn mode(&self) -> &GroupsMode {
        &self.mode
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        if self.education_banner {
            components.push(Component::Text {
                id: "first_group_education".into(),
                content: self.t("groups_list.first_group_education"),
                style: TextStyle::Caption,
            });
        }

        // Mode toggle (radio-style: only one selected at a time)
        components.push(self.mode_toggle());

        // Group list with mode-dependent detail text
        let items: Vec<ActionListItem> = self
            .groups
            .iter()
            .map(|g| {
                let detail = match self.mode {
                    GroupsMode::Members => {
                        let n = g.member_count;
                        if n == 1 {
                            self.t("groups_list.member_count_singular")
                        } else {
                            get_string_with_args(
                                self.locale,
                                "groups_list.member_count_plural",
                                &[("count", &n.to_string())],
                            )
                        }
                    }
                    GroupsMode::Visibility => {
                        let n = g.visible_field_count;
                        if n == 1 {
                            self.t("groups_list.visible_field_count_singular")
                        } else {
                            get_string_with_args(
                                self.locale,
                                "groups_list.visible_field_count_plural",
                                &[("count", &n.to_string())],
                            )
                        }
                    }
                };
                ActionListItem {
                    id: g.id.clone(),
                    label: g.name.clone(),
                    icon: Some("people".into()),
                    detail: Some(detail),
                    a11y: None,
                    info_key: None,
                }
            })
            .collect();

        components.push(Component::ActionList {
            id: "groups".into(),
            items,
        });

        ScreenModel {
            screen_id: "groups_list".into(),
            title: self.t("nav.groups"),
            subtitle: None,
            components,
            // Only "New Group" is a list-level action. Rename/delete a group
            // by opening it (tap a row → GroupDetail), where the target is
            // unambiguous.
            actions: vec![ScreenAction {
                id: "new_group".into(),
                label: self.t("form.new_group_title"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn mode_toggle(&self) -> Component {
        let members_selected = self.mode == GroupsMode::Members;
        let visibility_selected = self.mode == GroupsMode::Visibility;
        let selected_suffix = self.t("onboarding.a11y_selected");
        let not_selected_suffix = self.t("onboarding.a11y_not_selected");
        let toggle_hint = self.t("onboarding.a11y_toggle_hint");
        Component::ToggleList {
            id: "mode_toggle".into(),
            label: self.t("groups_list.view_mode_label"),
            items: vec![
                ToggleItem {
                    id: "members".into(),
                    label: self.t("group_detail.members_label"),
                    selected: members_selected,
                    subtitle: Some(self.t("groups_list.members_subtitle")),
                    a11y: Some(A11y {
                        label: Some(format!(
                            "{}, {}",
                            self.t("group_detail.members_label"),
                            if members_selected {
                                &selected_suffix
                            } else {
                                &not_selected_suffix
                            }
                        )),
                        hint: Some(toggle_hint.clone()),
                        role: Some(AccessibilityRole::Toggle),
                    }),
                    info_key: None,
                },
                ToggleItem {
                    id: "visibility".into(),
                    label: self.t("groups_list.visibility_mode_label"),
                    selected: visibility_selected,
                    subtitle: Some(self.t("groups_list.visibility_subtitle")),
                    a11y: Some(A11y {
                        label: Some(format!(
                            "{}, {}",
                            self.t("groups_list.visibility_mode_label"),
                            if visibility_selected {
                                &selected_suffix
                            } else {
                                &not_selected_suffix
                            }
                        )),
                        hint: Some(toggle_hint),
                        role: Some(AccessibilityRole::Toggle),
                    }),
                    info_key: None,
                },
            ],
            a11y: Some(A11y {
                label: Some(self.t("groups_list.view_mode_options_a11y")),
                hint: Some(self.t("contact_detail.select_items_hint")),
                role: None,
            }),
        }
    }
}

impl WorkflowEngine for GroupsEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
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
            // Screen-level actions. Only "New Group" lives here now; rename
            // and delete are per-group affordances on `GroupDetail`.
            UserAction::ActionPressed { action_id } if action_id == "new_group" => {
                ActionResult::ShowFormDialog {
                    dialog_type: "create_group".into(),
                    context_id: None,
                }
            }
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
    fn groups_list_offers_only_new_group_action() {
        // The list is intentionally a single primary action — rename/delete
        // a group from its detail screen, not here (no ambiguous selection).
        for groups in [vec![], sample_groups()] {
            let engine = GroupsEngine::new(groups, GroupsMode::Members);
            let screen = engine.current_screen();
            let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
            assert_eq!(
                ids,
                vec!["new_group"],
                "Groups list must offer only New Group, got {ids:?}"
            );
            assert!(screen.actions[0].enabled, "New Group is always enabled");
        }
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
    fn unknown_screen_actions_are_inert() {
        // Rename/delete/merge are no longer list-level actions; if a stale
        // frontend still emits one, the engine must not crash or mutate —
        // it just re-renders the current screen.
        let mut engine = GroupsEngine::new(sample_groups(), GroupsMode::Members);
        for stale in ["rename_group", "delete_group", "merge_groups"] {
            let result = engine.handle_action(UserAction::ActionPressed {
                action_id: stale.into(),
            });
            assert!(
                matches!(result, ActionResult::UpdateScreen(_)),
                "stale action `{stale}` must be inert, got {result:?}"
            );
        }
    }
}

// INLINE_TEST_REQUIRED: exercises the private education_banner render gate.
#[cfg(all(test, feature = "network-rustls"))]
mod education_tests {
    use super::*;

    // @internal
    #[test]
    fn education_banner_renders_only_when_enabled() {
        let on = GroupsEngine::new(Vec::new(), GroupsMode::Members).with_education_banner(true);
        let off = GroupsEngine::new(Vec::new(), GroupsMode::Members);
        let has = |e: &GroupsEngine| {
            e.current_screen()
                .components
                .iter()
                .any(|c| matches!(c, Component::Text { id, .. } if id == "first_group_education"))
        };
        assert!(has(&on), "enabled banner renders the education text");
        assert!(!has(&off), "banner absent unless explicitly enabled");
    }

    // @internal
    #[test]
    fn first_group_education_fires_exactly_once() {
        use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};
        let mut wb = Vauchi::in_memory().unwrap();
        wb.create_identity("Owner").unwrap();
        assert!(
            !GroupsEngine::first_group_education(&wb, 1),
            "no contacts yet -> no education"
        );
        let contact = Contact::from_exchange(
            [3u8; 32],
            ContactCard::new("Bob"),
            SymmetricKey::generate(),
            0,
        );
        wb.add_contact(contact).unwrap();
        assert!(
            !GroupsEngine::first_group_education(&wb, 0),
            "no group yet -> no education"
        );
        assert!(
            GroupsEngine::first_group_education(&wb, 1),
            "first group + contacts -> educate once"
        );
        assert!(
            !GroupsEngine::first_group_education(&wb, 1),
            "flag burned -> never again"
        );
    }
}
