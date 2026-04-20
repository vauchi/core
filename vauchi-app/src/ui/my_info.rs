// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! MyInfo screen engine — shows user's own card, entries, and visibility controls.

use crate::ui::contact_detail::SharedInfoView;
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
#[non_exhaustive]
pub enum MyInfoViewMode {
    /// List of entries with group info and contact count.
    EntryView,
    /// Tabs per group showing entries visible to that group.
    GroupView { selected_tab: usize },
    /// Read-only preview showing how the card looks to a specific contact.
    PreviewAs { contact_name: String },
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
    /// Data for the PreviewAs view mode — my card as seen by a specific contact.
    preview_data: Option<SharedInfoView>,
    /// Show a first-exchange prompt (user has no contacts yet).
    show_exchange_prompt: bool,
    /// Avatar image bytes (WebP) for the AvatarPreview component.
    avatar_data: Option<Vec<u8>>,
}

impl MyInfoEngine {
    pub fn new(_progress: MyInfoProgress) -> Self {
        Self {
            display_name: String::new(),
            own_fields: Vec::new(),
            groups: Vec::new(),
            view_mode: MyInfoViewMode::EntryView,
            preview_data: None,
            show_exchange_prompt: false,
            avatar_data: None,
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

    /// Set the preview data for PreviewAs view mode.
    pub fn with_preview(mut self, data: SharedInfoView) -> Self {
        self.preview_data = Some(data);
        self
    }

    /// Show a first-exchange prompt when the user has no contacts.
    pub fn with_exchange_prompt(mut self, show: bool) -> Self {
        self.show_exchange_prompt = show;
        self
    }

    /// Set the avatar image data for the AvatarPreview component.
    pub fn with_avatar_data(mut self, data: Option<Vec<u8>>) -> Self {
        self.avatar_data = data;
        self
    }

    /// Set the view mode directly (used for testing and navigation).
    pub fn with_view_mode(mut self, mode: MyInfoViewMode) -> Self {
        self.view_mode = mode;
        self
    }

    fn build_entry_view(&self) -> Vec<Component> {
        let mut components = Vec::new();

        if self.own_fields.is_empty() {
            components.push(Component::Text {
                id: "empty_hint".into(),
                content: "No entries yet. Add your first entry to share with contacts.\n\nYou can add phone numbers, email addresses, websites, social profiles, and more. Tap \"Add Entry\" to get started.".into(),
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
                    a11y: None,
                    info_key: None,
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
                a11y: None,
                info_key: None,
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
                        a11y: None,
                        info_key: None,
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

    fn build_preview_view(&self, contact_name: &str) -> Vec<Component> {
        let mut components = Vec::new();

        // Banner at top — informs the user they are in preview mode
        components.push(Component::Banner {
            text: format!("Viewing as {contact_name}"),
            action_label: "Exit Preview".into(),
            action_id: "exit-preview".into(),
            a11y: Some(A11y {
                label: Some(format!("Viewing as {contact_name}")),
                hint: Some("Action: Exit Preview".into()),
                role: Some(AccessibilityRole::Alert),
            }),
        });

        let Some(ref preview) = self.preview_data else {
            return components;
        };

        // Shared display name this contact sees
        components.push(Component::InfoPanel {
            id: "preview_shared_name".into(),
            icon: None,
            title: "They see you as".into(),
            items: vec![InfoItem {
                icon: None,
                title: "Display Name".into(),
                detail: preview.shared_display_name.clone(),
            }],
            a11y: Some(A11y {
                label: Some("They see you as".into()),
                hint: None,
                role: Some(AccessibilityRole::Heading),
            }),
        });

        // Render each field with its visibility state
        for field in &preview.my_fields {
            components.push(Component::FieldList {
                id: format!("preview_field_{}", field.id),
                fields: vec![field.clone()],
                visibility_mode: VisibilityMode::ReadOnly,
                available_groups: vec![],
                a11y: Some(A11y {
                    label: Some("Contact fields".into()),
                    hint: None,
                    role: None,
                }),
            });
        }

        components
    }
}

impl WorkflowEngine for MyInfoEngine {
    fn current_screen(&self) -> ScreenModel {
        if let MyInfoViewMode::PreviewAs { contact_name } = &self.view_mode {
            let components = self.build_preview_view(contact_name);
            let actions = vec![ScreenAction {
                id: "exit-preview".into(),
                label: "Exit Preview".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }];
            return ScreenModel {
                screen_id: "my_info".into(),
                title: format!("Viewing as {contact_name}"),
                subtitle: None,
                components,
                actions,
                progress: None,
                ..Default::default()
            };
        }

        let mut components = Vec::new();

        // Avatar preview at top of MyInfo — editable (tap to open AvatarEditor)
        let initials: String = self
            .display_name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase();
        components.push(Component::AvatarPreview {
            id: "avatar".into(),
            image_data: self.avatar_data.clone(),
            initials,
            bg_color: None,
            brightness: 0.0,
            editable: true,
            a11y: Some(A11y {
                label: Some("Your avatar".into()),
                hint: Some("Tap to edit avatar".into()),
                role: Some(AccessibilityRole::Button),
            }),
        });

        // First-exchange prompt: shown when user has no contacts yet
        if self.show_exchange_prompt {
            components.push(Component::InfoPanel {
                id: "exchange_prompt".into(),
                icon: Some("exchange".into()),
                title: "Ready to exchange?".into(),
                items: vec![InfoItem {
                    icon: Some("people".into()),
                    title: "Find someone nearby".into(),
                    detail: "Share your contact card in person — private and secure.".into(),
                }],
                a11y: Some(A11y {
                    label: Some("Ready to exchange?".into()),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            });
        }

        components.extend(match &self.view_mode {
            MyInfoViewMode::EntryView => self.build_entry_view(),
            MyInfoViewMode::GroupView { selected_tab } => self.build_group_view(*selected_tab),
            MyInfoViewMode::PreviewAs { .. } => unreachable!("handled above"),
        });

        let view_label = match &self.view_mode {
            MyInfoViewMode::EntryView => "Group View",
            MyInfoViewMode::GroupView { .. } => "Entry View",
            MyInfoViewMode::PreviewAs { .. } => unreachable!("handled above"),
        };

        let at_field_limit = self.own_fields.len() >= vauchi_core::contact_card::MAX_FIELDS;
        let mut actions = Vec::new();

        // Exchange shortcut when user has no contacts
        if self.show_exchange_prompt {
            actions.push(ScreenAction {
                id: "go_exchange".into(),
                label: "Exchange Now".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            });
        }

        actions.extend([
            ScreenAction {
                id: "add_field".into(),
                label: if at_field_limit {
                    format!(
                        "Field limit reached ({})",
                        vauchi_core::contact_card::MAX_FIELDS
                    )
                } else {
                    "Add Entry".into()
                },
                style: if self.show_exchange_prompt {
                    ActionStyle::Secondary
                } else {
                    ActionStyle::Primary
                },
                enabled: !at_field_limit,
                a11y: None,
            },
            ScreenAction {
                id: "toggle_view".into(),
                label: view_label.into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
            ScreenAction {
                id: "preview-as-picker".into(),
                label: "Preview as...".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
        ]);

        ScreenModel {
            screen_id: "my_info".into(),
            title: self.display_name.clone(),
            subtitle: None,
            components,
            actions,
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "add_field" => {
                // Signal to AppEngine to navigate to AddField form
                ActionResult::NavigateTo(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "preview-as-picker" => {
                // Signal to AppEngine to navigate to the Contacts screen (contact picker)
                ActionResult::ShowContactPicker
            }
            UserAction::ActionPressed { action_id } if action_id == "toggle_view" => {
                self.view_mode = match &self.view_mode {
                    MyInfoViewMode::EntryView => MyInfoViewMode::GroupView { selected_tab: 0 },
                    MyInfoViewMode::GroupView { .. } => MyInfoViewMode::EntryView,
                    // toggle_view is not available in preview mode — ignore
                    MyInfoViewMode::PreviewAs { .. } => {
                        return ActionResult::UpdateScreen(self.current_screen());
                    }
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

// INLINE_TEST_REQUIRED: MyInfoViewMode is module-private, cannot be tested from external tests/
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_info_has_preview_as_action_in_entry_view() {
        let engine = MyInfoEngine::new(MyInfoProgress::default());
        let screen = engine.current_screen();

        let action = screen.actions.iter().find(|a| a.id == "preview-as-picker");
        assert!(
            action.is_some(),
            "MyInfo (EntryView) should have 'preview-as-picker' action"
        );
        assert_eq!(action.unwrap().label, "Preview as...");
    }

    #[test]
    fn test_my_info_has_preview_as_action_in_group_view() {
        let engine = MyInfoEngine::new(MyInfoProgress::default())
            .with_view_mode(MyInfoViewMode::GroupView { selected_tab: 0 });
        let screen = engine.current_screen();

        let action = screen.actions.iter().find(|a| a.id == "preview-as-picker");
        assert!(
            action.is_some(),
            "MyInfo (GroupView) should have 'preview-as-picker' action"
        );
    }

    #[test]
    fn test_my_info_preview_mode_has_no_preview_as_picker_action() {
        let engine = MyInfoEngine::new(MyInfoProgress::default()).with_view_mode(
            MyInfoViewMode::PreviewAs {
                contact_name: "Alice".into(),
            },
        );
        let screen = engine.current_screen();

        let action = screen.actions.iter().find(|a| a.id == "preview-as-picker");
        assert!(
            action.is_none(),
            "MyInfo in PreviewAs mode should NOT have 'preview-as-picker' action"
        );
    }

    #[test]
    fn test_preview_as_picker_returns_show_contact_picker() {
        let mut engine = MyInfoEngine::new(MyInfoProgress::default());
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "preview-as-picker".into(),
        });
        assert_eq!(result, ActionResult::ShowContactPicker);
    }
}
