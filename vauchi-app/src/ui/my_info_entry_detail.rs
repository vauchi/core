// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Entry detail screen — edit value, modify group visibility, see which contacts can see it.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Info about a contact that can see this entry (for the read-only contact list).
#[derive(Clone, Debug)]
pub struct EntryContactInfo {
    pub contact_id: String,
    pub name: String,
    pub via_group: String,
}

/// Engine for the MyInfo entry detail screen.
#[derive(Clone, Debug)]
pub struct MyInfoEntryDetailEngine {
    pub field_id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
    /// Private per-field note (never shared).
    pub note: Option<String>,
    /// All groups with their visibility state for this field.
    pub groups: Vec<(String, String, bool)>, // (group_id, group_name, is_visible)
    /// Contacts who can see this field (derived from group membership).
    pub visible_contacts: Vec<EntryContactInfo>,
    /// The unassigned entry's Visible/Hidden toggle state (explicit
    /// `Everyone`). Rendered only while no group grants the entry —
    /// group-assigned entries are governed by the group toggles alone
    /// (field-centric model, 2026-07-05-ungrouped-contacts-default-open).
    pub shown: bool,
    locale: Locale,
}

impl MyInfoEntryDetailEngine {
    pub fn new(
        field_id: String,
        field_type: String,
        label: String,
        value: String,
        note: Option<String>,
        groups: Vec<(String, String, bool)>,
        visible_contacts: Vec<EntryContactInfo>,
    ) -> Self {
        Self {
            field_id,
            field_type,
            label,
            value,
            note,
            groups,
            visible_contacts,
            shown: false,
            locale: Locale::English,
        }
    }

    /// Seed the unassigned entry's Visible/Hidden toggle state.
    pub fn with_shown(mut self, shown: bool) -> Self {
        self.shown = shown;
        self
    }

    /// A field any group grants is group-audience data — the group toggles
    /// govern it and the all-contacts toggle is not rendered.
    fn is_group_assigned(&self) -> bool {
        self.groups.iter().any(|(_, _, visible)| *visible)
    }
}

impl MyInfoEntryDetailEngine {
    /// Rebuild current_screen after external mutation of fields.
    pub fn refresh_screen(&self) -> ScreenModel {
        self.current_screen()
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
}

impl WorkflowEngine for MyInfoEntryDetailEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::MyInfoEntryDetail {
            label: self.label.clone(),
            value: self.value.clone(),
            note: self.note.clone(),
            groups: self.groups.clone(),
        })
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        let crate::ui::EngineUpdate::MyInfoEntryDetail(update) = update else {
            return false;
        };
        match update {
            crate::ui::MyInfoEntryDetailUpdate::GroupVisibility {
                group_id,
                visible,
                visible_contacts,
            } => {
                if let Some(entry) = self.groups.iter_mut().find(|(gid, _, _)| gid == &group_id) {
                    entry.2 = visible;
                }
                self.visible_contacts = visible_contacts;
            }
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Field info
        components.push(Component::Text {
            a11y: None,
            id: "field_info".into(),
            content: format!("{} ({})", self.value, self.label),
            style: TextStyle::Title,
        });

        components.push(Component::Divider);

        // Unassigned entry: one Visible/Hidden toggle governing every
        // contact alike. Disappears once any group grants the entry.
        if !self.is_group_assigned() {
            components.push(Component::ToggleList {
                id: "entry_visibility".into(),
                label: self.t("my_info_entry_detail.entry_visibility_label"),
                items: vec![ToggleItem {
                    id: "shown".into(),
                    label: self.t("my_info_entry_detail.visible_to_all"),
                    selected: self.shown,
                    subtitle: None,
                    a11y: None,
                    info_key: None,
                }],
                a11y: None,
            });
        }

        // Group visibility toggles
        if !self.groups.is_empty() {
            let toggle_items: Vec<ToggleItem> = self
                .groups
                .iter()
                .map(|(gid, gname, visible)| ToggleItem {
                    id: gid.clone(),
                    label: gname.clone(),
                    selected: *visible,
                    subtitle: None,
                    a11y: None,
                    info_key: None,
                })
                .collect();

            components.push(Component::ToggleList {
                id: "group_visibility".into(),
                label: self.t("my_info_entry_detail.visible_to_groups_label"),
                items: toggle_items,
                a11y: None,
            });
        }

        components.push(Component::Divider);

        // Contacts who can see this entry (read-only list)
        if self.visible_contacts.is_empty() {
            components.push(Component::Text {
                a11y: None,
                id: "no_contacts".into(),
                content: self.t("my_info_entry_detail.no_contacts"),
                style: TextStyle::Caption,
            });
        } else {
            components.push(Component::Text {
                a11y: None,
                id: "contacts_header".into(),
                content: get_string_with_args(
                    self.locale,
                    "my_info_entry_detail.visible_to_count",
                    &[("count", &self.visible_contacts.len().to_string())],
                ),
                style: TextStyle::Subtitle,
            });

            let contact_items: Vec<ActionListItem> = self
                .visible_contacts
                .iter()
                .map(|c| ActionListItem {
                    id: c.contact_id.clone(),
                    label: c.name.clone(),
                    icon: None,
                    detail: Some(get_string_with_args(
                        self.locale,
                        "my_info_entry_detail.via_group",
                        &[("group", &c.via_group)],
                    )),
                    a11y: None,
                    info_key: None,
                })
                .collect();

            components.push(Component::ActionList {
                id: "visible_contacts".into(),
                items: contact_items,
            });
        }

        ScreenModel {
            screen_id: "my_info_entry_detail".into(),
            title: self.label.clone(),
            subtitle: Some(self.field_type.clone()),
            components,
            contextual_actions: vec![
                ScreenAction {
                    id: "edit".into(),
                    label: self.t("action.edit"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "delete".into(),
                    label: self.t("action.delete"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    // Make the button's target explicit to screen
                    // readers — "Delete" alone, on a detail screen for
                    // a specific field, is ambiguous. Announce the
                    // field label so VoiceOver says "Delete email,
                    // button" (etc.) rather than just "Delete".
                    a11y: Some(A11y {
                        label: Some(get_string_with_args(
                            self.locale,
                            "my_info_entry_detail.delete_field_a11y",
                            &[("label", &self.label)],
                        )),
                        hint: Some(self.t("my_info_entry_detail.delete_field_hint")),
                        role: None,
                    }),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "group_visibility" => {
                // Toggle group visibility — return a signal so AppEngine can persist
                ActionResult::NavigateTo(self.current_screen())
            }
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "entry_visibility" && item_id == "shown" => {
                // Flip the all-contacts toggle — AppEngine persists via
                // set_field_shown (which arms repropagation).
                self.shown = !self.shown;
                ActionResult::NavigateTo(self.current_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "edit" => ActionResult::NavigateTo(self.current_screen()),
                "delete" => ActionResult::Complete,
                _ => ActionResult::UpdateScreen(self.current_screen()),
            },
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: tests exercise the engine's private render logic
// (is_group_assigned gating) not reachable from tests/it.
#[cfg(test)]
mod tests {
    use super::*;

    fn engine(groups: Vec<(String, String, bool)>) -> MyInfoEntryDetailEngine {
        MyInfoEntryDetailEngine::new(
            "f1".into(),
            "email".into(),
            "Email".into(),
            "a@b.test".into(),
            None,
            groups,
            Vec::new(),
        )
    }

    fn has_entry_visibility_toggle(screen: &ScreenModel) -> bool {
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "entry_visibility"))
    }

    // @internal
    #[test]
    fn unassigned_entry_renders_the_all_contacts_toggle() {
        let e = engine(vec![("g1".into(), "Team".into(), false)]).with_shown(true);
        assert!(
            has_entry_visibility_toggle(&e.current_screen()),
            "no group grants the entry → the Visible/Hidden toggle renders"
        );
    }

    // @internal
    #[test]
    fn group_assigned_entry_hides_the_all_contacts_toggle() {
        let e = engine(vec![("g1".into(), "Team".into(), true)]);
        assert!(
            !has_entry_visibility_toggle(&e.current_screen()),
            "a group grants the entry → group toggles govern, no base toggle"
        );
    }

    // @internal
    #[test]
    fn toggling_entry_visibility_flips_shown() {
        let mut e = engine(Vec::new());
        assert!(!e.shown, "seeded hidden");
        let result = e.handle_action(UserAction::ItemToggled {
            component_id: "entry_visibility".into(),
            item_id: "shown".into(),
        });
        assert!(e.shown, "toggle flips the engine state");
        assert!(matches!(result, ActionResult::NavigateTo(_)));
    }
}
