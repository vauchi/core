// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tor settings engine — manages Tor privacy configuration.

use crate::ui::*;

/// Engine that displays and manages Tor privacy settings.
#[derive(Clone, Debug)]
pub struct TorSettingsEngine {
    enabled: bool,
    prefer_onion: bool,
}

impl TorSettingsEngine {
    pub fn new(enabled: bool, prefer_onion: bool) -> Self {
        Self {
            enabled,
            prefer_onion,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let status_text = if self.enabled { "Enabled" } else { "Disabled" };

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
                        detail: status_text.into(),
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
            actions: vec![
                ScreenAction {
                    id: if self.enabled { "disable" } else { "enable" }.into(),
                    label: if self.enabled {
                        "Disable Tor"
                    } else {
                        "Enable Tor"
                    }
                    .into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "new_circuit".into(),
                    label: "New Circuit".into(),
                    style: ActionStyle::Secondary,
                    enabled: self.enabled,
                },
            ],
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
                "enable" => {
                    self.enabled = true;
                    ActionResult::UpdateScreen(self.build_screen())
                }
                "disable" => {
                    self.enabled = false;
                    ActionResult::UpdateScreen(self.build_screen())
                }
                "new_circuit" => ActionResult::UpdateScreen(self.build_screen()),
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            UserAction::ItemToggled {
                component_id: _,
                item_id,
            } => {
                match item_id.as_str() {
                    "tor_enabled" => self.enabled = !self.enabled,
                    "prefer_onion" => self.prefer_onion = !self.prefer_onion,
                    _ => {}
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
