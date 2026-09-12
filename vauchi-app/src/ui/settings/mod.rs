// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings screen engine — displays app settings grouped by category.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use serde::{Deserialize, Serialize};

mod groups;

/// Configuration values displayed and toggled on the settings screen.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SettingsConfig {
    pub display_name: String,
    pub delivery_receipts_enabled: bool,
    pub suppress_presence: bool,
    /// New contact-card entries start Visible (Decision 2,
    /// 2026-07-05-ungrouped-contacts-default-open). Default off = hidden.
    #[serde(default)]
    pub new_field_default_visible: bool,
    #[serde(default)]
    pub contact_added_notifications: bool,
    #[serde(default = "default_true")]
    pub card_update_notifications: bool,
    pub relay_url: String,
    pub device_count: usize,
    pub password_set: bool,
    /// Currently-selected theme dropdown option id.
    /// `"follow_system"` is the reserved id meaning "let the OS decide";
    /// every other value is a `DropdownOption.id` from `available_themes`.
    #[serde(default, alias = "theme")]
    pub theme_id: String,
    #[serde(default)]
    pub available_themes: Vec<DropdownOption>,
    /// Currently-selected language dropdown option id (mirror of `theme_id`).
    #[serde(default, alias = "language")]
    pub language_id: String,
    #[serde(default)]
    pub available_languages: Vec<DropdownOption>,
    #[serde(default)]
    pub reduce_motion: bool,
    #[serde(default)]
    pub large_touch: bool,
    #[serde(default = "default_true")]
    pub show_help_icons: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub build: String,
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
/// Which Settings surface this engine renders (M6 D6.1). The main list
/// carries the everyday groups + an "Advanced…" link; the advanced
/// sub-screen carries the rare/technical groups + emergency wipe, kept
/// behind deliberate navigation (danger far from the thumb-reachable
/// bottom).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsMode {
    Main,
    Advanced,
}

pub struct SettingsEngine {
    config: SettingsConfig,
    mode: SettingsMode,
    pending_wipe: bool,
}

impl SettingsEngine {
    pub fn new(config: SettingsConfig) -> Self {
        Self {
            config,
            mode: SettingsMode::Main,
            pending_wipe: false,
        }
    }

    pub fn new_advanced(config: SettingsConfig) -> Self {
        Self {
            config,
            mode: SettingsMode::Advanced,
            pending_wipe: false,
        }
    }

    fn locale(&self) -> Locale {
        Locale::from_code(&self.config.language_id).unwrap_or_default()
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale(), key)
    }

    /// The everyday-6 main list + the Advanced link. Network, delivery,
    /// and the emergency wipe live on the advanced sub-screen instead
    /// (M6 D6.1 — danger far from the thumb-reachable bottom).
    fn main_screen(&self) -> ScreenModel {
        let components = vec![
            self.profile_group(),
            self.privacy_notifications_group(),
            self.appearance_group(),
            self.theme_dropdown(),
            self.language_dropdown(),
            self.accessibility_group(),
            self.security_backup_group(),
            self.help_about_group(),
            self.advanced_link(),
        ];
        ScreenModel {
            screen_id: "settings".into(),
            title: self.t("settings.title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    /// The advanced sub-screen: rare/technical groups + the emergency
    /// wipe, reached only by deliberate navigation. Back is the generic
    /// nav-stack pop (parent stamped by the overlay layer).
    fn advanced_screen(&self) -> ScreenModel {
        let mut components = vec![
            self.network_group(),
            self.delivery_group(),
            self.danger_group(),
        ];

        if self.pending_wipe {
            components.push(Component::InlineConfirm {
                id: "emergency_wipe".into(),
                warning: self.t("settings.emergency_wipe_confirm_warning"),
                confirm_text: self.t("shred.wipe.wipe_all"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: "confirm_emergency_wipe".into(),
                cancel_action_id: "cancel_emergency_wipe".into(),
                destructive: true,
                a11y: Some(A11y {
                    label: Some(self.t("settings.emergency_wipe_confirm_a11y")),
                    hint: Some(self.t("settings.emergency_wipe_confirm_hint")),
                    role: None,
                }),
            });
        }

        ScreenModel {
            screen_id: "settings_advanced".into(),
            title: self.t("settings.advanced_title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for SettingsEngine {
    fn current_screen(&self) -> ScreenModel {
        match self.mode {
            SettingsMode::Main => self.main_screen(),
            SettingsMode::Advanced => self.advanced_screen(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "privacy_notifications" && item_id == "delivery_receipts" => {
                self.config.delivery_receipts_enabled = !self.config.delivery_receipts_enabled;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "privacy_notifications" && item_id == "suppress_presence" => {
                self.config.suppress_presence = !self.config.suppress_presence;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "privacy_notifications" && item_id == "contact_added" => {
                self.config.contact_added_notifications = !self.config.contact_added_notifications;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "privacy_notifications" && item_id == "card_update" => {
                self.config.card_update_notifications = !self.config.card_update_notifications;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "accessibility" => {
                match item_id.as_str() {
                    "reduce_motion" => self.config.reduce_motion = !self.config.reduce_motion,
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
            } if component_id == "help_about" && item_id == "debug_mode" => {
                self.config.debug_mode = !self.config.debug_mode;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "security_backup" && item_id == "backup_reminders" => {
                let current = vauchi_core::types::ReminderFrequency::from_label(
                    &self.config.backup_reminder_frequency,
                );
                let next = current.next();
                self.config.backup_reminder_frequency = next.label().to_string();
                ActionResult::UpdateScreen(self.current_screen())
            }
            // Theme + Language Dropdown selections — persistence happens
            // in `app_engine::intercept::persist_settings_toggle`, which
            // writes the new value into the engine's RenderContext.
            // Engine mirrors the new id locally so the screen reflects
            // the pick on the very next render; the fresh config built
            // on re-entry to AppScreen::Settings re-derives the id from
            // the engine's RenderContext (ADR-047).
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "theme" => {
                self.config.theme_id = item_id.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "language" => {
                self.config.language_id = item_id.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "help_about" && item_id == "what_is_vauchi" => {
                let title = self.t("about.what_is_vauchi.title");
                let body = self.t("about.what_is_vauchi.body");
                ActionResult::ShowInfoOverlay { title, body }
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
