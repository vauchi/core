// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact merge preview engine — side-by-side comparison before merging.

use crate::ui::*;

/// Configuration for the merge preview.
#[derive(Clone, Debug)]
pub struct MergePreview {
    pub primary_name: String,
    pub primary_fields: Vec<String>,
    pub secondary_name: String,
    pub secondary_fields: Vec<String>,
}

/// Engine displaying a side-by-side merge preview of two contacts.
#[derive(Clone, Debug)]
pub struct ContactMergeEngine {
    preview: MergePreview,
}

impl ContactMergeEngine {
    pub fn new(preview: MergePreview) -> Self {
        Self { preview }
    }

    fn build_screen(&self) -> ScreenModel {
        let primary_items: Vec<InfoItem> = self
            .preview
            .primary_fields
            .iter()
            .map(|f| InfoItem {
                icon: None,
                title: f.clone(),
                detail: String::new(),
                accessible_label: None,
                accessible_hint: None,
            })
            .collect();

        let secondary_items: Vec<InfoItem> = self
            .preview
            .secondary_fields
            .iter()
            .map(|f| InfoItem {
                icon: None,
                title: f.clone(),
                detail: String::new(),
                accessible_label: None,
                accessible_hint: None,
            })
            .collect();

        let components = vec![
            Component::Text {
                id: "merge_title".into(),
                content: format!(
                    "{} (keep) <- {} (remove)",
                    self.preview.primary_name, self.preview.secondary_name
                ),
                style: TextStyle::Subtitle,
                accessible_label: None,
                accessible_hint: None,
            },
            Component::InfoPanel {
                id: "primary_fields".into(),
                icon: None,
                title: format!("{} (keep)", self.preview.primary_name),
                items: primary_items,
                accessible_label: None,
                accessible_hint: None,
            },
            Component::InfoPanel {
                id: "secondary_fields".into(),
                icon: None,
                title: format!("{} (remove)", self.preview.secondary_name),
                items: secondary_items,
                accessible_label: None,
                accessible_hint: None,
            },
            Component::Text {
                id: "merge_note".into(),
                content: "Unique fields from the secondary will be added to the primary. The secondary will be deleted.".into(),
                style: TextStyle::Body,
                accessible_label: None,
                accessible_hint: None,
            },
        ];

        ScreenModel {
            screen_id: "contact_merge".into(),
            title: "Merge Contacts".into(),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "confirm".into(),
                    label: "Confirm Merge".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }
}

impl WorkflowEngine for ContactMergeEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "confirm" => ActionResult::Complete,
                "cancel" => ActionResult::UpdateScreen(self.build_screen()),
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
