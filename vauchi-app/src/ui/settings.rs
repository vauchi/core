// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings screen engine — displays app settings grouped by category.

use crate::ui::*;
use serde::{Deserialize, Serialize};

/// Configuration values displayed and toggled on the settings screen.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SettingsConfig {
    pub display_name: String,
    pub delivery_receipts_enabled: bool,
    pub suppress_presence: bool,
    #[serde(default)]
    pub contact_added_notifications: bool,
    pub relay_url: String,
    pub device_count: usize,
    pub password_set: bool,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub available_themes: Vec<DropdownOption>,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub available_languages: Vec<DropdownOption>,
    #[serde(default)]
    pub reduce_motion: bool,
    #[serde(default)]
    pub high_contrast: bool,
    #[serde(default)]
    pub large_touch: bool,
    #[serde(default = "default_true")]
    pub show_help_icons: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub build: String,
    #[serde(default)]
    pub sync_status: String,
    #[serde(default)]
    pub pending_updates: u32,
    #[serde(default)]
    pub failed_deliveries: u32,
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default)]
    pub backup_reminder_frequency: String,
    #[serde(default)]
    pub last_backup_display: String,
}

fn default_true() -> bool {
    true
}

/// Settings screen engine.
pub struct SettingsEngine {
    config: SettingsConfig,
    pending_wipe: bool,
}

impl SettingsEngine {
    pub fn new(config: SettingsConfig) -> Self {
        Self {
            config,
            pending_wipe: false,
        }
    }
}

impl WorkflowEngine for SettingsEngine {
    fn current_screen(&self) -> ScreenModel {
        let show_help = self.config.show_help_icons;

        let mut components = vec![
            Component::SettingsGroup {
                id: "profile".into(),
                label: "Profile".into(),
                items: vec![
                    SettingsItem {
                        id: "display_name".into(),
                        label: "Display Name".into(),
                        kind: SettingsItemKind::Value {
                            value: self.config.display_name.clone(),
                        },
                        a11y: Some(A11y {
                            label: Some("Display Name".into()),
                            hint: Some("Your visible name shown to contacts".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "edit_profile".into(),
                        label: "Edit Profile".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Edit Profile".into()),
                            hint: Some("Opens the profile editor".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "privacy".into(),
                label: "Privacy".into(),
                items: vec![
                    SettingsItem {
                        id: "delivery_receipts".into(),
                        label: "Delivery Receipts".into(),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.config.delivery_receipts_enabled,
                        },
                        a11y: Some(A11y {
                            label: Some("Delivery Receipts toggle".into()),
                            hint: Some(
                                "When enabled, notifies senders when their messages are delivered"
                                    .into(),
                            ),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "suppress_presence".into(),
                        label: "Suppress Presence".into(),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.config.suppress_presence,
                        },
                        a11y: Some(A11y {
                            label: Some("Suppress Presence toggle".into()),
                            hint: Some(
                                "When enabled, hides your online status from contacts".into(),
                            ),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "notifications".into(),
                label: "Notifications".into(),
                items: vec![SettingsItem {
                    id: "contact_added".into(),
                    label: "New Contact Added".into(),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.contact_added_notifications,
                    },
                    a11y: Some(A11y {
                        label: Some("New Contact Added toggle".into()),
                        hint: Some(
                            "When enabled, sends a notification when a new contact is added".into(),
                        ),
                        role: None,
                    }),
                    info_key: None,
                }],
            },
            Component::SettingsGroup {
                id: "appearance".into(),
                label: "Appearance".into(),
                items: vec![
                    SettingsItem {
                        id: "theme".into(),
                        label: "Theme".into(),
                        kind: SettingsItemKind::Value {
                            value: self.config.theme.clone(),
                        },
                        a11y: Some(A11y {
                            label: Some("Theme".into()),
                            hint: Some("The current color theme of the app".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "language".into(),
                        label: "Language".into(),
                        kind: SettingsItemKind::Value {
                            value: self.config.language.clone(),
                        },
                        a11y: Some(A11y {
                            label: Some("Language".into()),
                            hint: Some("The current display language of the app".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "show_help_icons".into(),
                        label: "Show help icons".into(),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.config.show_help_icons,
                        },
                        a11y: Some(A11y {
                            label: None,
                            hint: Some("Toggle contextual help icons on form fields".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "accessibility".into(),
                label: "Accessibility".into(),
                items: vec![
                    SettingsItem {
                        id: "reduce_motion".into(),
                        label: "Reduce Motion".into(),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.config.reduce_motion,
                        },
                        a11y: Some(A11y {
                            label: Some("Reduce Motion toggle".into()),
                            hint: Some(
                                "When enabled, minimizes animations and motion effects".into(),
                            ),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "high_contrast".into(),
                        label: "High Contrast".into(),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.config.high_contrast,
                        },
                        a11y: Some(A11y {
                            label: Some("High Contrast toggle".into()),
                            hint: Some(
                                "When enabled, increases color contrast for better visibility"
                                    .into(),
                            ),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "large_touch".into(),
                        label: "Large Touch Targets".into(),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.config.large_touch,
                        },
                        a11y: Some(A11y {
                            label: Some("Large Touch Targets toggle".into()),
                            hint: Some(
                                "When enabled, increases the size of interactive elements".into(),
                            ),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "security".into(),
                label: "Security".into(),
                items: vec![
                    SettingsItem {
                        id: "change_password".into(),
                        label: "Change Password".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Change Password".into()),
                            hint: Some("Opens the password change screen".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "devices".into(),
                        label: "Devices".into(),
                        kind: SettingsItemKind::Link {
                            detail: Some(if self.config.device_count == 1 {
                                "1 device".into()
                            } else {
                                format!("{} devices", self.config.device_count)
                            }),
                        },
                        a11y: Some(A11y {
                            label: Some("Devices".into()),
                            hint: Some("Opens the list of linked devices".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "duress_pin".into(),
                        label: "Duress PIN".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Duress PIN".into()),
                            hint: Some("Opens the duress PIN configuration screen".into()),
                            role: None,
                        }),
                        info_key: if show_help {
                            Some("duress_pin".into())
                        } else {
                            None
                        },
                    },
                ],
            },
            Component::SettingsGroup {
                id: "backup".into(),
                label: "Backup & Recovery".into(),
                items: vec![
                    SettingsItem {
                        id: "backup_export".into(),
                        label: "Create Backup".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Create Backup".into()),
                            hint: Some("Opens the backup export screen".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "backup_import".into(),
                        label: "Restore Backup".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Restore Backup".into()),
                            hint: Some("Opens the backup restore screen".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "setup_new_device".into(),
                        label: "Set Up New Device".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Set Up New Device".into()),
                            hint: Some("Transfer your contacts to a new device via QR code".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "last_backup".into(),
                        label: "Last backup".into(),
                        kind: SettingsItemKind::Value {
                            value: self.config.last_backup_display.clone(),
                        },
                        a11y: Some(A11y {
                            label: Some("Last backup date".into()),
                            hint: None,
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "backup_reminders".into(),
                        label: "Backup reminders".into(),
                        kind: SettingsItemKind::Value {
                            value: self.config.backup_reminder_frequency.clone(),
                        },
                        a11y: Some(A11y {
                            label: Some("Backup reminder frequency".into()),
                            hint: Some("Tap to change frequency".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "network".into(),
                label: "Network".into(),
                items: vec![SettingsItem {
                    id: "relay_url".into(),
                    label: "Relay URL".into(),
                    kind: SettingsItemKind::Value {
                        value: self.config.relay_url.clone(),
                    },
                    a11y: Some(A11y {
                        label: Some("Relay URL".into()),
                        hint: Some("The relay server used for message delivery".into()),
                        role: None,
                    }),
                    info_key: None,
                }],
            },
            Component::SettingsGroup {
                id: "delivery".into(),
                label: "Message Delivery".into(),
                items: vec![
                    SettingsItem {
                        id: "sync".into(),
                        label: "Sync Status".into(),
                        kind: SettingsItemKind::Link {
                            detail: Some(self.config.sync_status.clone()),
                        },
                        a11y: Some(A11y {
                            label: Some("Sync Status".into()),
                            hint: Some("Opens the sync status detail screen".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "pending_updates".into(),
                        label: "Pending Updates".into(),
                        kind: SettingsItemKind::Value {
                            value: self.config.pending_updates.to_string(),
                        },
                        a11y: Some(A11y {
                            label: Some("Pending Updates".into()),
                            hint: Some("Number of contact updates waiting to be delivered".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "failed_deliveries".into(),
                        label: "Failed Deliveries".into(),
                        kind: SettingsItemKind::Value {
                            value: self.config.failed_deliveries.to_string(),
                        },
                        a11y: Some(A11y {
                            label: Some("Failed Deliveries".into()),
                            hint: Some("Number of updates that failed to deliver".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "help".into(),
                label: "Help & Support".into(),
                items: vec![
                    SettingsItem {
                        id: "help_center".into(),
                        label: "Help Center".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Help Center".into()),
                            hint: Some("Opens the help center".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "funding".into(),
                        label: "Support Development".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Support Development".into()),
                            hint: Some("Opens the funding and support page".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "privacy_policy".into(),
                        label: "Privacy Policy".into(),
                        kind: SettingsItemKind::Link { detail: None },
                        a11y: Some(A11y {
                            label: Some("Privacy Policy".into()),
                            hint: Some("Opens the privacy policy".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "about".into(),
                label: "About".into(),
                items: vec![
                    SettingsItem {
                        id: "version".into(),
                        label: "Version".into(),
                        kind: SettingsItemKind::Value {
                            value: if self.config.build.is_empty() {
                                self.config.version.clone()
                            } else {
                                format!("{} ({})", self.config.version, self.config.build)
                            },
                        },
                        a11y: Some(A11y {
                            label: Some("Version".into()),
                            hint: Some("The current app version and build number".into()),
                            role: None,
                        }),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "debug_mode".into(),
                        label: "Debug Mode".into(),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.config.debug_mode,
                        },
                        a11y: Some(A11y {
                            label: Some("Debug Mode toggle".into()),
                            hint: Some(
                                "When enabled, shows developer diagnostic information".into(),
                            ),
                            role: None,
                        }),
                        info_key: None,
                    },
                ],
            },
            Component::SettingsGroup {
                id: "danger".into(),
                label: "Danger Zone".into(),
                items: vec![SettingsItem {
                    id: "emergency_wipe".into(),
                    label: "Emergency Wipe".into(),
                    kind: SettingsItemKind::Destructive {
                        label: "Wipe All Data".into(),
                    },
                    a11y: Some(A11y {
                        label: Some("Emergency Wipe".into()),
                        hint: Some(
                            "Permanently deletes all app data. This action cannot be undone".into(),
                        ),
                        role: None,
                    }),
                    info_key: None,
                }],
            },
        ];

        if self.pending_wipe {
            components.push(Component::InlineConfirm {
                id: "emergency_wipe".into(),
                warning: "This will permanently delete all data. This action cannot be undone."
                    .into(),
                confirm_text: "Wipe All Data".into(),
                cancel_text: "Cancel".into(),
                destructive: true,
                a11y: Some(A11y {
                    label: Some("Confirm emergency data wipe".into()),
                    hint: Some("This action is irreversible and will destroy all data".into()),
                    role: None,
                }),
            });
        }

        ScreenModel {
            screen_id: "settings".into(),
            title: "Settings".into(),
            subtitle: None,
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "privacy" && item_id == "delivery_receipts" => {
                self.config.delivery_receipts_enabled = !self.config.delivery_receipts_enabled;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "privacy" && item_id == "suppress_presence" => {
                self.config.suppress_presence = !self.config.suppress_presence;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "notifications" && item_id == "contact_added" => {
                self.config.contact_added_notifications = !self.config.contact_added_notifications;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "accessibility" => {
                match item_id.as_str() {
                    "reduce_motion" => self.config.reduce_motion = !self.config.reduce_motion,
                    "high_contrast" => self.config.high_contrast = !self.config.high_contrast,
                    "large_touch" => self.config.large_touch = !self.config.large_touch,
                    _ => {}
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "appearance" && item_id == "show_help_icons" => {
                self.config.show_help_icons = !self.config.show_help_icons;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "about" && item_id == "debug_mode" => {
                self.config.debug_mode = !self.config.debug_mode;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "backup" && item_id == "backup_reminders" => {
                let current = vauchi_core::types::ReminderFrequency::from_label(
                    &self.config.backup_reminder_frequency,
                );
                let next = current.next();
                self.config.backup_reminder_frequency = next.label().to_string();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected { ref item_id, .. } if item_id == "emergency_wipe" => {
                self.pending_wipe = true;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { ref action_id }
                if action_id == "confirm_emergency_wipe" =>
            {
                self.pending_wipe = false;
                ActionResult::Complete
            }
            UserAction::ActionPressed { ref action_id } if action_id == "cancel_emergency_wipe" => {
                self.pending_wipe = false;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected { .. } => ActionResult::NavigateTo(self.current_screen()),
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
