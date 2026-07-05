// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync status engine — shows relay connection state and pending updates.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use vauchi_core::network::ConnectionState;

/// Engine that displays sync status with the relay.
#[derive(Clone, Debug)]
pub struct SyncStatusEngine {
    relay_url: String,
    contact_count: usize,
    pending_updates: usize,
    connection_state: ConnectionState,
    last_action: Option<String>,
    locale: Locale,
}

impl SyncStatusEngine {
    /// Creates a sync status engine (defaults to Disconnected state).
    pub fn new(relay_url: String, contact_count: usize, pending_updates: usize) -> Self {
        Self {
            relay_url,
            contact_count,
            pending_updates,
            connection_state: ConnectionState::Disconnected,
            last_action: None,
            locale: Locale::English,
        }
    }

    /// Creates a sync status engine with a specific connection state.
    pub fn with_connection_state(mut self, state: ConnectionState) -> Self {
        self.connection_state = state;
        self
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S6b-3).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn connection_status_text(&self) -> (String, Status) {
        match self.connection_state {
            ConnectionState::Connected => (self.t("sync.connected"), Status::Success),
            ConnectionState::Connecting => (self.t("sync.connecting"), Status::InProgress),
            ConnectionState::Reconnecting { attempt } => {
                let key = if attempt > 3 {
                    "sync.reconnecting_slow"
                } else {
                    "sync.reconnecting"
                };
                (self.t(key), Status::InProgress)
            }
            ConnectionState::Disconnected => (self.t("sync.status_offline"), Status::Failed),
            _ => (self.t("sync.unknown_status"), Status::InProgress),
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let (status_text, status) = self.connection_status_text();
        let is_offline = matches!(self.connection_state, ConnectionState::Disconnected);

        let mut components = vec![Component::StatusIndicator {
            id: "connection_status".into(),
            icon: None,
            title: get_string_with_args(
                self.locale,
                "sync.relay_status_label",
                &[("status", &status_text)],
            ),
            detail: if is_offline {
                Some(self.t("sync.offline_detail"))
            } else {
                None
            },
            status,
            a11y: None,
        }];

        components.push(Component::InfoPanel {
            id: "sync_info".into(),
            icon: Some("sync".into()),
            title: self.t("sync.details"),
            items: vec![
                InfoItem {
                    icon: Some("relay".into()),
                    title: self.t("sync.relay"),
                    detail: self.relay_url.clone(),
                },
                InfoItem {
                    icon: Some("contacts".into()),
                    title: self.t("nav.contacts"),
                    detail: format!("{}", self.contact_count),
                },
                InfoItem {
                    icon: Some("pending".into()),
                    title: self.t("sync.pending_updates_title"),
                    detail: if self.pending_updates == 0 {
                        self.t("sync.all_up_to_date")
                    } else {
                        get_string_with_args(
                            self.locale,
                            "sync.pending_updates_waiting",
                            &[("count", &self.pending_updates.to_string())],
                        )
                    },
                },
            ],
            a11y: None,
        });

        ScreenModel {
            screen_id: "sync_status".into(),
            title: self.t("sync.title"),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "sync_now".into(),
                    label: self.t("sync.sync_now"),
                    style: ActionStyle::Primary,
                    enabled: !is_offline,
                    a11y: None,
                },
                ScreenAction {
                    id: "test_connection".into(),
                    label: if is_offline {
                        self.t("sync.retry_connection")
                    } else {
                        self.t("sync.test_connection")
                    },
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

    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        use crate::ui::{EngineOutput, SyncChoice};
        match self.last_action.as_deref() {
            Some("sync_now") => Some(EngineOutput::Sync(SyncChoice::SyncNow)),
            Some("test_connection") => Some(EngineOutput::Sync(SyncChoice::TestConnection)),
            _ => None,
        }
    }
}
