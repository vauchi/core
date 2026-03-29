// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Support engine — links to support and sponsor the project.

use crate::ui::*;

/// Engine that displays support and sponsorship information.
#[derive(Clone, Debug)]
pub struct SupportEngine;

impl Default for SupportEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SupportEngine {
    pub fn new() -> Self {
        Self
    }

    fn build_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "support".into(),
            title: "Support Vauchi".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "support_info".into(),
                icon: Some("heart".into()),
                title: "Support the Project".into(),
                items: vec![
                    InfoItem {
                        icon: Some("github".into()),
                        title: "GitHub Sponsors".into(),
                        detail: "https://github.com/sponsors/vauchi".into(),
                    },
                    InfoItem {
                        icon: Some("liberapay".into()),
                        title: "Liberapay".into(),
                        detail: "https://liberapay.com/vauchi".into(),
                    },
                ],
            }],
            actions: vec![
                ScreenAction {
                    id: "open_github_sponsors".into(),
                    label: "GitHub Sponsors".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "open_liberapay".into(),
                    label: "Liberapay".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for SupportEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "open_github_sponsors" => ActionResult::OpenUrl {
                    url: "https://github.com/sponsors/vauchi".into(),
                },
                "open_liberapay" => ActionResult::OpenUrl {
                    url: "https://liberapay.com/vauchi".into(),
                },
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
