// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tor settings engine — manages Tor privacy configuration.
//!
//! Dispatches [`TorCommand`]s to the app layer when the user toggles
//! settings. The app layer decides whether to forward these to a real
//! `TorManager` (when the `tor` feature is enabled) or handle them
//! as no-ops.

use crate::tor_config::TorStatus;
use crate::ui::*;

/// Engine that displays and manages Tor privacy settings.
///
/// Holds local toggle state and an optional [`TorStatus`] that the app
/// layer can update via [`set_status`](Self::set_status) to reflect
/// real backend state (Connecting, Connected, Disconnected, etc.).
#[derive(Clone, Debug)]
pub struct TorSettingsEngine {
    enabled: bool,
    prefer_onion: bool,
    status: TorStatus,
}

impl TorSettingsEngine {
    pub fn new(enabled: bool, prefer_onion: bool) -> Self {
        Self {
            enabled,
            prefer_onion,
            status: TorStatus::Disabled,
        }
    }

    /// Update the displayed Tor status from the app/backend layer.
    pub fn set_status(&mut self, status: TorStatus) {
        self.status = status;
    }

    fn build_screen(&self) -> ScreenModel {
        let status_text = self.status.to_string();

        ScreenModel {
            screen_id: "tor_settings".into(),
            title: "Tor Privacy".into(),
            subtitle: None,
            components: vec![
                Component::InfoPanel {
                    id: "tor_status".into(),
                    icon: Some("tor".into()),
                    title: "Tor Status".into(),
                    items: vec![InfoItem {
                        icon: None,
                        title: "Status".into(),
                        detail: status_text,
                    }],
                },
                Component::ToggleList {
                    id: "tor_toggles".into(),
                    label: "Tor Settings".into(),
                    items: vec![
                        ToggleItem {
                            id: "tor_enabled".into(),
                            label: "Enable Tor".into(),
                            selected: self.enabled,
                            subtitle: Some("Route all traffic through Tor".into()),
                        },
                        ToggleItem {
                            id: "prefer_onion".into(),
                            label: "Prefer Onion Services".into(),
                            selected: self.prefer_onion,
                            subtitle: Some("Use .onion addresses when available".into()),
                        },
                    ],
                },
            ],
            actions: vec![ScreenAction {
                id: "new_circuit".into(),
                label: "New Circuit".into(),
                style: ActionStyle::Secondary,
                enabled: self.enabled,
            }],
            progress: None,
        }
    }
}

impl WorkflowEngine for TorSettingsEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "new_circuit" => {
                    if self.enabled {
                        ActionResult::TorCommand {
                            command: TorCommand::RotateCircuit,
                        }
                    } else {
                        ActionResult::UpdateScreen(self.build_screen())
                    }
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            UserAction::ItemToggled {
                component_id: _,
                item_id,
            } => match item_id.as_str() {
                "tor_enabled" => {
                    self.enabled = !self.enabled;
                    if self.enabled {
                        ActionResult::TorCommand {
                            command: TorCommand::Bootstrap,
                        }
                    } else {
                        ActionResult::TorCommand {
                            command: TorCommand::Shutdown,
                        }
                    }
                }
                "prefer_onion" => {
                    self.prefer_onion = !self.prefer_onion;
                    ActionResult::TorCommand {
                        command: TorCommand::UpdateConfig {
                            prefer_onion: self.prefer_onion,
                        },
                    }
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
