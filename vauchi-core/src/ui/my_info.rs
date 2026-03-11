// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! MyInfo screen engine — shows user's own card, entries, and visibility controls.

use crate::ui::*;

/// Progress summary for the MyInfo screen.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MyInfoProgress {
    pub completed_steps: usize,
    pub total_steps: usize,
}

/// A single own field for display on the MyInfo screen.
#[derive(Clone, Debug)]
pub struct OwnFieldInfo {
    pub field_id: String,
    pub field_type: String,
    pub label: String,
    pub value: String,
    /// Group names that can see this field.
    pub visible_groups: Vec<String>,
    /// Number of contacts who can see this field (derived from group membership).
    pub contact_count: usize,
}

/// View mode for the MyInfo screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MyInfoViewMode {
    /// List of entries with group info and contact count.
    EntryView,
    /// Tabs per group showing entries visible to that group.
    GroupView { selected_tab: usize },
}

/// Group info for the group view tabs.
#[derive(Clone, Debug)]
pub struct MyInfoGroupTab {
    pub group_id: String,
    pub group_name: String,
    /// Field indices (into own_fields) visible to this group.
    pub field_indices: Vec<usize>,
}

/// MyInfo screen engine — shows user's own card entries.
#[derive(Clone, Debug)]
pub struct MyInfoEngine {
    display_name: String,
    own_fields: Vec<OwnFieldInfo>,
    groups: Vec<MyInfoGroupTab>,
    view_mode: MyInfoViewMode,
}

impl MyInfoEngine {
    pub fn new(_progress: MyInfoProgress) -> Self {
        Self {
            display_name: String::new(),
            own_fields: Vec::new(),
            groups: Vec::new(),
            view_mode: MyInfoViewMode::EntryView,
        }
    }

    /// Set the user's display name and own card fields.
    pub fn with_own_card(mut self, display_name: String, fields: Vec<OwnFieldInfo>) -> Self {
        self.display_name = display_name;
        self.own_fields = fields;
        self
    }

    /// Set the group tabs for group view.
    pub fn with_groups(mut self, groups: Vec<MyInfoGroupTab>) -> Self {
        self.groups = groups;
        self
    }

    fn build_entry_view(&self) -> Vec<Component> {
        let mut components = Vec::new();

        if self.own_fields.is_empty() {
            components.push(Component::Text {
                id: "empty_hint".into(),
                content: "No entries yet. Add your first entry to share with contacts.".into(),
                style: TextStyle::Caption,
            });
            return components;
        }

        // Build selectable entry list using ActionList
        let items: Vec<ActionListItem> = self
            .own_fields
            .iter()
            .map(|f| {
                let groups_str = if f.visible_groups.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", f.visible_groups.join(", "))
                };
                let contacts_str = if f.contact_count > 0 {
                    format!("{} contacts", f.contact_count)
                } else {
                    String::new()
                };
                let detail = match (groups_str.is_empty(), contacts_str.is_empty()) {
                    (true, true) => None,
                    (false, true) => Some(groups_str),
                    (true, false) => Some(contacts_str),
                    (false, false) => Some(format!("{groups_str} {contacts_str}")),
                };
                ActionListItem {
                    id: f.field_id.clone(),
                    label: format!("{} ({})", f.value, f.label),
                    icon: Some(f.field_type.clone()),
                    detail,
                }
            })
            .collect();

        components.push(Component::ActionList {
            id: "own_entries".into(),
            items,
        });

        components
    }

    fn build_group_view(&self, selected_tab: usize) -> Vec<Component> {
        let mut components = Vec::new();

        if self.groups.is_empty() {
            components.push(Component::Text {
                id: "no_groups".into(),
                content: "No groups created. Create groups to control field visibility.".into(),
                style: TextStyle::Caption,
            });
            return components;
        }

        // Tab labels
        let tab_items: Vec<ActionListItem> = self
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| ActionListItem {
                id: g.group_id.clone(),
                label: g.group_name.clone(),
                icon: None,
                detail: if i == selected_tab {
                    Some("selected".into())
                } else {
                    None
                },
            })
            .collect();

        components.push(Component::ActionList {
            id: "group_tabs".into(),
            items: tab_items,
        });

        // Entries visible to selected group
        if let Some(group) = self.groups.get(selected_tab) {
            if group.field_indices.is_empty() {
                components.push(Component::Text {
                    id: "group_empty".into(),
                    content: format!(
                        "No entries visible to {}. Assign entries via entry detail.",
                        group.group_name
                    ),
                    style: TextStyle::Caption,
                });
            } else {
                let items: Vec<ActionListItem> = group
                    .field_indices
                    .iter()
                    .filter_map(|&idx| self.own_fields.get(idx))
                    .map(|f| ActionListItem {
                        id: f.field_id.clone(),
                        label: format!("{} ({})", f.value, f.label),
                        icon: Some(f.field_type.clone()),
                        detail: None,
                    })
                    .collect();

                components.push(Component::ActionList {
                    id: "group_entries".into(),
                    items,
                });
            }
        }

        components
    }
}

impl WorkflowEngine for MyInfoEngine {
    fn current_screen(&self) -> ScreenModel {
        let components = match &self.view_mode {
            MyInfoViewMode::EntryView => self.build_entry_view(),
            MyInfoViewMode::GroupView { selected_tab } => self.build_group_view(*selected_tab),
        };

        let view_label = match &self.view_mode {
            MyInfoViewMode::EntryView => "Group View",
            MyInfoViewMode::GroupView { .. } => "Entry View",
        };

        let actions = vec![
            ScreenAction {
                id: "add_field".into(),
                label: "Add Entry".into(),
                style: ActionStyle::Primary,
                enabled: true,
            },
            ScreenAction {
                id: "toggle_view".into(),
                label: view_label.into(),
                style: ActionStyle::Secondary,
                enabled: true,
            },
        ];

        ScreenModel {
            screen_id: "my_info".into(),
            title: self.display_name.clone(),
            subtitle: None,
            components,
            actions,
            progress: None,
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "add_field" => {
                // Signal to AppEngine to navigate to AddField form
                ActionResult::NavigateTo(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "toggle_view" => {
                self.view_mode = match &self.view_mode {
                    MyInfoViewMode::EntryView => MyInfoViewMode::GroupView { selected_tab: 0 },
                    MyInfoViewMode::GroupView { .. } => MyInfoViewMode::EntryView,
                };
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } => {
                match component_id.as_str() {
                    "own_entries" | "group_entries" => {
                        // Entry selected → navigate to entry detail
                        ActionResult::OpenEntryDetail { field_id: item_id }
                    }
                    "group_tabs" => {
                        // Group tab selected → switch tab
                        if let Some(idx) = self.groups.iter().position(|g| g.group_id == item_id) {
                            self.view_mode = MyInfoViewMode::GroupView { selected_tab: idx };
                        }
                        ActionResult::UpdateScreen(self.current_screen())
                    }
                    _ => ActionResult::UpdateScreen(self.current_screen()),
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
