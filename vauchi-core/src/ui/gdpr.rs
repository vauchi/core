// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR / privacy engine — data export, deletion, and consent management.

use crate::ui::*;

/// Engine that manages privacy and data settings (GDPR).
#[derive(Clone, Debug)]
pub struct GdprEngine {
    deletion_status: Option<String>,
    consent_summary: String,
    /// Tracks which action triggered completion ("export" or "delete").
    last_action: Option<String>,
}

impl GdprEngine {
    pub fn new(deletion_status: Option<String>, consent_summary: String) -> Self {
        Self {
            deletion_status,
            consent_summary,
            last_action: None,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let deletion_detail = self
            .deletion_status
            .clone()
            .unwrap_or_else(|| "No deletion requested".into());

        ScreenModel {
            screen_id: "privacy_settings".into(),
            title: "Privacy & Data".into(),
            subtitle: None,
            components: vec![
                Component::InfoPanel {
                    id: "privacy_info".into(),
                    icon: Some("privacy".into()),
                    title: "Data Status".into(),
                    items: vec![
                        InfoItem {
                            icon: None,
                            title: "Deletion Status".into(),
                            detail: deletion_detail,
                        },
                        InfoItem {
                            icon: None,
                            title: "Consent".into(),
                            detail: self.consent_summary.clone(),
                        },
                    ],
                },
                Component::ActionList {
                    id: "consent_actions".into(),
                    items: vec![
                        ActionListItem {
                            id: "view_data".into(),
                            label: "View My Data".into(),
                            icon: Some("data".into()),
                            detail: Some("See what data is stored locally".into()),
                        },
                        ActionListItem {
                            id: "manage_consent".into(),
                            label: "Manage Consent".into(),
                            icon: Some("consent".into()),
                            detail: Some("Review and update data consent".into()),
                        },
                    ],
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "export".into(),
                    label: "Export Data".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "delete".into(),
                    label: "Delete All Data".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }
}

impl WorkflowEngine for GdprEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "export" => {
                    self.last_action = Some("export".into());
                    ActionResult::Complete
                }
                "delete" => {
                    self.last_action = Some("delete".into());
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn collected_input(&self) -> Option<String> {
        self.last_action.clone()
    }
}
