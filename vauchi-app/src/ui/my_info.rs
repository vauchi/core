// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! MyInfo screen engine — shows user's own card, entries, and visibility controls.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::relative_time::format_relative_time;
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
    /// Avatar image bytes (WebP) for the ImageCircle component.
    avatar_data: Option<Vec<u8>>,
    /// Outbound updates queued for the next sync (per
    /// `Vauchi::pending_update_count`). Rendered as a caption only when > 0.
    pending_updates: u32,
    /// Wall-clock unix seconds of the last successful sync (per
    /// `Vauchi::last_sync_time`). Rendered as a caption when present.
    last_sync_seconds: Option<u64>,
    /// Wall-clock unix seconds at render time, used to compute the
    /// `last_sync_seconds` relative-time caption.
    now_seconds: u64,
    locale: Locale,
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
            pending_updates: 0,
            last_sync_seconds: None,
            now_seconds: 0,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S6a).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
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

    /// Set the avatar image data for the ImageCircle component.
    pub fn with_avatar_data(mut self, data: Option<Vec<u8>>) -> Self {
        self.avatar_data = data;
        self
    }

    /// Set the view mode directly (used for testing and navigation).
    pub fn with_view_mode(mut self, mode: MyInfoViewMode) -> Self {
        self.view_mode = mode;
        self
    }

    /// Set the pending-updates count for the sync caption.
    pub fn with_pending_updates(mut self, count: u32) -> Self {
        self.pending_updates = count;
        self
    }

    /// Set the wall-clock unix timestamp of the last successful sync.
    pub fn with_last_sync_seconds(mut self, seconds: Option<u64>) -> Self {
        self.last_sync_seconds = seconds;
        self
    }

    /// Set the wall-clock "now" used to compute the last-sync relative caption.
    pub fn with_now_seconds(mut self, seconds: u64) -> Self {
        self.now_seconds = seconds;
        self
    }

    fn sync_status_components(&self) -> Vec<Component> {
        let mut out = Vec::new();
        if self.pending_updates > 0 {
            let label = if self.pending_updates == 1 {
                get_string(self.locale, "sync.pending_updates_one")
            } else {
                get_string_with_args(
                    self.locale,
                    "sync.pending_updates",
                    &[("count", &self.pending_updates.to_string())],
                )
            };
            out.push(Component::Text {
                id: "pending_updates_caption".into(),
                content: label,
                style: TextStyle::Caption,
            });
        }
        if let Some(then) = self.last_sync_seconds {
            let relative = format_relative_time(self.now_seconds, then, self.locale);
            out.push(Component::Text {
                id: "last_sync_caption".into(),
                content: get_string_with_args(
                    self.locale,
                    "sync.last_synced",
                    &[("time", &relative)],
                ),
                style: TextStyle::Caption,
            });
        }
        out
    }

    fn build_entry_view(&self) -> Vec<Component> {
        let mut components = Vec::new();

        if self.own_fields.is_empty() {
            components.push(Component::Text {
                id: "empty_hint".into(),
                content: self.t("my_info.empty_entries"),
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
                content: self.t("my_info.empty_groups"),
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
                hint: Some(self.t("my_info.exit_preview_a11y_hint")),
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
            title: self.t("my_info.preview.title"),
            items: vec![InfoItem {
                icon: None,
                title: self.t("settings.display_name"),
                detail: preview.shared_display_name.clone(),
            }],
            a11y: Some(A11y {
                label: Some(self.t("my_info.preview.title")),
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
                    label: Some(self.t("my_info.preview.fields_a11y_label")),
                    hint: None,
                    role: None,
                }),
            });
        }

        components
    }

    fn build_actions(&self) -> Vec<ScreenAction> {
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
                label: self.t("my_info.exchange_now_button"),
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
                label: self.t("my_info.preview_as_button"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            },
        ]);
        actions
    }
}

impl WorkflowEngine for MyInfoEngine {
    fn current_screen(&self) -> ScreenModel {
        if let MyInfoViewMode::PreviewAs { contact_name } = &self.view_mode {
            let components = self.build_preview_view(contact_name);
            let actions = vec![ScreenAction {
                id: "exit-preview".into(),
                label: self.t("my_info.exit_preview_button"),
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
        components.push(Component::ImageCircle {
            id: "avatar".into(),
            image_data: self.avatar_data.clone(),
            initials: crate::ui::component::initials(&self.display_name),
            bg_color: None,
            brightness: 0.0,
            editable: true,
            edit_action_id: Some("edit_avatar".into()),
            a11y: Some(A11y {
                label: Some(self.t("my_info.avatar_a11y_label")),
                hint: Some(self.t("my_info.avatar_a11y_hint")),
                role: Some(AccessibilityRole::Button),
            }),
        });

        // First-exchange prompt: shown when user has no contacts yet
        if self.show_exchange_prompt {
            components.push(Component::InfoPanel {
                id: "exchange_prompt".into(),
                icon: Some("exchange".into()),
                title: self.t("my_info.exchange_prompt_title"),
                items: vec![InfoItem {
                    icon: Some("people".into()),
                    title: self.t("my_info.exchange_prompt_action"),
                    detail: self.t("my_info.exchange_prompt_detail"),
                }],
                a11y: Some(A11y {
                    label: Some(self.t("my_info.exchange_prompt_a11y_label")),
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

        components.extend(self.sync_status_components());

        let actions = self.build_actions();

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

    fn caption_content(screen: &ScreenModel, id: &str) -> Option<String> {
        screen.components.iter().find_map(|c| match c {
            Component::Text {
                id: cid, content, ..
            } if cid == id => Some(content.clone()),
            _ => None,
        })
    }

    // @internal
    #[test]
    fn test_my_info_emits_pending_updates_caption() {
        let engine = MyInfoEngine::new(MyInfoProgress::default()).with_pending_updates(3);
        let screen = engine.current_screen();
        assert_eq!(
            caption_content(&screen, "pending_updates_caption").as_deref(),
            Some("3 pending updates"),
        );
    }

    // @internal
    #[test]
    fn test_my_info_pending_updates_caption_uses_singular_for_one() {
        let engine = MyInfoEngine::new(MyInfoProgress::default()).with_pending_updates(1);
        let screen = engine.current_screen();
        assert_eq!(
            caption_content(&screen, "pending_updates_caption").as_deref(),
            Some("1 pending update"),
        );
    }

    // @internal
    #[test]
    fn test_my_info_omits_pending_updates_caption_when_zero() {
        let engine = MyInfoEngine::new(MyInfoProgress::default()).with_pending_updates(0);
        let screen = engine.current_screen();
        assert!(caption_content(&screen, "pending_updates_caption").is_none());
    }

    // @internal
    #[test]
    fn test_my_info_emits_last_sync_caption() {
        // 5 minutes ago — format_relative_time renders "5 minutes ago"
        let now = 1_700_000_000u64;
        let engine = MyInfoEngine::new(MyInfoProgress::default())
            .with_last_sync_seconds(Some(now - 5 * 60))
            .with_now_seconds(now);
        let screen = engine.current_screen();
        assert_eq!(
            caption_content(&screen, "last_sync_caption").as_deref(),
            Some("Last synced 5 minutes ago"),
        );
    }

    // @internal
    #[test]
    fn test_my_info_omits_last_sync_caption_when_none() {
        let engine = MyInfoEngine::new(MyInfoProgress::default()).with_now_seconds(1_700_000_000);
        let screen = engine.current_screen();
        assert!(caption_content(&screen, "last_sync_caption").is_none());
    }

    // @internal
    #[test]
    fn test_my_info_emits_both_captions_in_order_pending_then_sync() {
        let now = 1_700_000_000u64;
        let engine = MyInfoEngine::new(MyInfoProgress::default())
            .with_pending_updates(2)
            .with_last_sync_seconds(Some(now - 120))
            .with_now_seconds(now);
        let screen = engine.current_screen();
        let positions: Vec<usize> = screen
            .components
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                Component::Text { id, .. }
                    if id == "pending_updates_caption" || id == "last_sync_caption" =>
                {
                    Some(i)
                }
                _ => None,
            })
            .collect();
        assert_eq!(positions.len(), 2, "expected both captions in the screen");
        assert!(
            positions[0] < positions[1],
            "pending_updates_caption must appear before last_sync_caption"
        );
    }

    // @internal
    #[test]
    fn test_my_info_preview_mode_omits_sync_status_captions() {
        let now = 1_700_000_000u64;
        let engine = MyInfoEngine::new(MyInfoProgress::default())
            .with_pending_updates(5)
            .with_last_sync_seconds(Some(now - 60))
            .with_now_seconds(now)
            .with_view_mode(MyInfoViewMode::PreviewAs {
                contact_name: "Alice".into(),
            });
        let screen = engine.current_screen();
        assert!(
            caption_content(&screen, "pending_updates_caption").is_none(),
            "PreviewAs renders the card as the contact sees it — owner-only sync status must not leak"
        );
        assert!(caption_content(&screen, "last_sync_caption").is_none());
    }
}
