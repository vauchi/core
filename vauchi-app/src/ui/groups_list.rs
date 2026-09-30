// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups engine — lists contact groups, each with how many contacts it has
//! and how many of your entries they see (#446).

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

const GROUPS_ID: &str = "groups";
const ACTIONS_ID: &str = "group_actions";
const ADD_GROUP_ID: &str = "add_group";

/// Summary info for a group.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub member_count: usize,
    /// Entries a contact in only this group is shown: the group's grants
    /// plus the entries shown to everyone that no group governs.
    pub entries_seen: usize,
}

/// Counts the own-card entries a contact whose only group is `group` sees,
/// by the same partition as `Vauchi::get_effective_field_visibility`, so the
/// row cannot promise more or less than propagation delivers.
#[cfg(feature = "network-rustls")]
pub(crate) fn entries_seen_by_member(
    card: &vauchi_core::contact_card::ContactCard,
    groups: &[vauchi_core::Group],
    group: &vauchi_core::Group,
) -> usize {
    card.fields()
        .iter()
        .filter(|field| {
            let id = field.id();
            group.is_field_visible(id)
                || (!groups.iter().any(|g| g.is_field_visible(id))
                    && card.field_visibility().is_explicitly_everyone(id))
        })
        .count()
}

/// Engine that displays contact groups.
///
/// Rename / delete are per-group actions reached by tapping a group row →
/// `GroupDetail` (which owns them unambiguously). The list itself offers
/// only "Add group": the former list-level Rename/Delete operated on
/// `groups.first()` regardless of which group the user meant — a
/// wrong-group bug — and "Merge Groups" was an unimplemented stub. See
/// `2026-06-05-screen-ux-declutter`.
pub struct GroupsEngine {
    groups: Vec<GroupInfo>,
    /// One-shot model-shift education, shown the first time the owner
    /// creates a group while already having exchanged contacts: entries
    /// assigned to a group become visible only to its members; unassigned
    /// entries keep their Visible/Hidden toggle (Decision 3,
    /// 2026-07-05-ungrouped-contacts-default-open).
    education_banner: bool,
    locale: Locale,
}

impl GroupsEngine {
    pub fn new(groups: Vec<GroupInfo>) -> Self {
        Self {
            groups,
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

    fn count(&self, n: usize, singular: &str, plural: &str) -> String {
        if n == 1 {
            self.t(singular)
        } else {
            get_string_with_args(self.locale, plural, &[("count", &n.to_string())])
        }
    }

    fn row_detail(&self, group: &GroupInfo) -> String {
        let contacts = self.count(
            group.member_count,
            "groups_list.contact_count_singular",
            "groups_list.contact_count_plural",
        );
        let entries = self.count(
            group.entries_seen,
            "groups_list.sees_entry_count_singular",
            "groups_list.sees_entry_count_plural",
        );
        get_string_with_args(
            self.locale,
            "groups_list.row_detail",
            &[("contacts", &contacts), ("entries", &entries)],
        )
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        if self.education_banner {
            components.push(Component::Text {
                a11y: None,
                id: "first_group_education".into(),
                content: self.t("groups_list.first_group_education"),
                style: TextStyle::Caption,
            });
        }

        if self.groups.is_empty() {
            components.push(Component::Text {
                a11y: None,
                id: "empty".into(),
                content: self.t("groups_list.empty_explanation"),
                style: TextStyle::Body,
            });
        } else {
            components.push(Component::ActionList {
                id: GROUPS_ID.into(),
                items: self
                    .groups
                    .iter()
                    .map(|g| ActionListItem {
                        id: g.id.clone(),
                        label: g.name.clone(),
                        icon: Some("people".into()),
                        detail: Some(self.row_detail(g)),
                        a11y: None,
                        info_key: None,
                    })
                    .collect(),
            });
        }

        components.push(Component::ButtonList {
            id: ACTIONS_ID.into(),
            items: vec![ActionListItem {
                id: ADD_GROUP_ID.into(),
                label: self.t("groups_list.add_group_button"),
                icon: None,
                detail: None,
                a11y: None,
                info_key: None,
            }],
        });

        ScreenModel {
            screen_id: "groups_list".into(),
            title: self.t("nav.groups"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for GroupsEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            // Group selected from list — reuses OpenContact to signal "open detail".
            // AppEngine routes this to GroupDetail when the current screen is Groups.
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == GROUPS_ID => ActionResult::OpenContact {
                contact_id: item_id,
            },
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == ACTIONS_ID && item_id == ADD_GROUP_ID => {
                ActionResult::ShowFormDialog {
                    dialog_type: "create_group".into(),
                    context_id: None,
                }
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
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
        let on = GroupsEngine::new(Vec::new()).with_education_banner(true);
        let off = GroupsEngine::new(Vec::new());
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
