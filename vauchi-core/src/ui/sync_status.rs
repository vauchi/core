// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync status engine — shows relay connection state and pending updates.

use crate::network::ConnectionState;
use crate::ui::*;

/// Engine that displays sync status with the relay.
#[derive(Clone, Debug)]
pub struct SyncStatusEngine {
    relay_url: String,
    contact_count: usize,
    pending_updates: usize,
    connection_state: ConnectionState,
    last_action: Option<String>,
}

impl SyncStatusEngine {
    pub fn new(
        relay_url: String,
        contact_count: usize,
        pending_updates: usize,
        connection_state: ConnectionState,
    ) -> Self {
        Self {
            relay_url,
            contact_count,
            pending_updates,
            connection_state,
            last_action: None,
        }
    }

    fn connection_status_text(&self) -> (&str, Status) {
        match self.connection_state {
            ConnectionState::Connected => ("Connected", Status::Success),
            ConnectionState::Connecting => ("Connecting...", Status::InProgress),
            ConnectionState::Reconnecting { attempt } => {
                // Can't use format! in a &str return, so use a fixed message
                if attempt > 3 {
                    ("Reconnecting (slow network)...", Status::InProgress)
                } else {
                    ("Reconnecting...", Status::InProgress)
                }
            }
            ConnectionState::Disconnected => ("Offline", Status::Failed),
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let (status_text, status) = self.connection_status_text();
        let is_offline = matches!(self.connection_state, ConnectionState::Disconnected);

        let mut components = vec![Component::StatusIndicator {
            id: "connection_status".into(),
            icon: None,
            title: format!("Relay: {status_text}"),
            detail: if is_offline {
                Some(
                    "Changes will sync automatically when connected. \
                     Check your internet connection."
                        .into(),
                )
            } else {
                None
            },
            status,
        }];

        components.push(Component::InfoPanel {
            id: "sync_info".into(),
            icon: Some("sync".into()),
            title: "Details".into(),
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
                    detail: if self.pending_updates == 0 {
                        "All up to date".into()
                    } else {
                        format!("{} update(s) waiting to sync", self.pending_updates)
                    },
                },
            ],
        });

        ScreenModel {
            screen_id: "sync_status".into(),
            title: "Sync".into(),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "sync_now".into(),
                    label: "Sync Now".into(),
                    style: ActionStyle::Primary,
                    enabled: !is_offline,
                },
                ScreenAction {
                    id: "test_connection".into(),
                    label: if is_offline {
                        "Retry Connection".into()
                    } else {
                        "Test Connection".into()
                    },
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
                "sync_now" => {
                    // Signal to AppEngine that user wants to sync
                    self.last_action = Some("sync_now".into());
                    ActionResult::Complete
                }
                "test_connection" => {
                    self.last_action = Some("test_connection".into());
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
