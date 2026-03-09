// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync status engine — shows relay connection and pending updates.

use crate::ui::*;

/// Engine that displays sync status with the relay.
#[derive(Clone, Debug)]
pub struct SyncStatusEngine {
    relay_url: String,
    contact_count: usize,
    pending_updates: usize,
}

impl SyncStatusEngine {
    pub fn new(relay_url: String, contact_count: usize, pending_updates: usize) -> Self {
        Self {
            relay_url,
            contact_count,
            pending_updates,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "sync_status".into(),
            title: "Sync".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "sync_info".into(),
                icon: Some("sync".into()),
                title: "Sync Status".into(),
                items: vec![
                    InfoItem {
                        icon: Some("relay".into()),
                        title: "Relay".into(),
                        detail: self.relay_url.clone(),
                    },
                    InfoItem {
                        icon: Some("contacts".into()),
                        title: "Contacts".into(),
                        detail: format!("{}", self.contact_count),
                    },
                    InfoItem {
                        icon: Some("pending".into()),
                        title: "Pending Updates".into(),
                        detail: format!("{}", self.pending_updates),
                    },
                ],
            }],
            actions: vec![
                ScreenAction {
                    id: "sync_now".into(),
                    label: "Sync Now".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "test_connection".into(),
                    label: "Test Connection".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: None,
        }
    }
}

impl WorkflowEngine for SyncStatusEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "sync_now" | "test_connection" => ActionResult::UpdateScreen(self.build_screen()),
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
