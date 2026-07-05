// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact merge preview engine — side-by-side comparison before merging.

use crate::i18n::{Locale, get_string, get_string_with_args};
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
    locale: Locale,
}

impl ContactMergeEngine {
    pub fn new(preview: MergePreview) -> Self {
        Self {
            preview,
            locale: Locale::English,
        }
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
        let primary_items: Vec<InfoItem> = self
            .preview
            .primary_fields
            .iter()
            .map(|f| InfoItem {
                icon: None,
                title: f.clone(),
                detail: String::new(),
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
            })
            .collect();

        let components = vec![
            Component::Text {
                id: "merge_title".into(),
                content: get_string_with_args(
                    self.locale,
                    "contact_merge.title_content",
                    &[
                        ("primary", &self.preview.primary_name),
                        ("secondary", &self.preview.secondary_name),
                    ],
                ),
                style: TextStyle::Subtitle,
            },
            Component::InfoPanel {
                id: "primary_fields".into(),
                icon: None,
                title: get_string_with_args(
                    self.locale,
                    "contact_merge.keep_label",
                    &[("name", &self.preview.primary_name)],
                ),
                items: primary_items,
                a11y: None,
            },
            Component::InfoPanel {
                id: "secondary_fields".into(),
                icon: None,
                title: get_string_with_args(
                    self.locale,
                    "contact_merge.remove_label",
                    &[("name", &self.preview.secondary_name)],
                ),
                items: secondary_items,
                a11y: None,
            },
            Component::Text {
                id: "merge_note".into(),
                content: self.t("contact_merge.note"),
                style: TextStyle::Body,
            },
        ];

        ScreenModel {
            screen_id: "contact_merge".into(),
            title: self.t("contact_merge.title"),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "confirm".into(),
                    label: self.t("contact_merge.confirm_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
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
