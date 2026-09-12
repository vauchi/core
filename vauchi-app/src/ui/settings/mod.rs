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

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

/// Which Settings surface this engine renders. The main list is the
/// design canvas's three sections; the sub-screens carry the theme and
/// language pickers, the accessibility toggles, and the rare/technical
/// rows plus the emergency wipe, kept behind deliberate navigation
/// (M6 D6.1 — danger far from the thumb-reachable bottom).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsMode {
    Main,
    Advanced,
    Appearance,
    Accessibility,
}

/// Settings screen engine.
pub struct SettingsEngine {
    config: SettingsConfig,
    mode: SettingsMode,
    pending_wipe: bool,
}

impl SettingsEngine {
    pub fn new(config: SettingsConfig) -> Self {
        Self::with_mode(config, SettingsMode::Main)
    }

    pub fn new_advanced(config: SettingsConfig) -> Self {
        Self::with_mode(config, SettingsMode::Advanced)
    }

    pub fn new_appearance(config: SettingsConfig) -> Self {
        Self::with_mode(config, SettingsMode::Appearance)
    }

    pub fn new_accessibility(config: SettingsConfig) -> Self {
        Self::with_mode(config, SettingsMode::Accessibility)
    }

    pub fn with_mode(config: SettingsConfig, mode: SettingsMode) -> Self {
        Self {
            config,
            mode,
            pending_wipe: false,
        }
    }

    fn locale(&self) -> Locale {
        Locale::from_code(&self.config.language_id).unwrap_or_default()
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale(), key)
    }

    fn screen(&self, screen_id: &str, title_key: &str, components: Vec<Component>) -> ScreenModel {
        ScreenModel {
            screen_id: screen_id.into(),
            title: self.t(title_key),
            subtitle: None,
            components,
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    /// The design canvas: My identity / Privacy / App.
    fn main_screen(&self) -> ScreenModel {
        self.screen(
            "settings",
            "settings.title",
            vec![
                self.identity_group(),
                self.privacy_group(),
                self.app_group(),
            ],
        )
    }

    fn appearance_screen(&self) -> ScreenModel {
        self.screen(
            "settings_appearance",
            "settings.appearance_language",
            vec![
                self.theme_dropdown(),
                self.language_dropdown(),
                self.appearance_group(),
            ],
        )
    }

    fn accessibility_screen(&self) -> ScreenModel {
        self.screen(
            "settings_accessibility",
            "settings.accessibility",
            vec![self.accessibility_group()],
        )
    }

    /// Everything the canvas does not show, plus the emergency wipe,
    /// reached only by deliberate navigation. Back is the generic
    /// nav-stack pop (parent stamped by the overlay layer).
    fn advanced_screen(&self) -> ScreenModel {
        let mut components = vec![
            self.security_group(),
            self.backup_group(),
            self.network_group(),
            self.delivery_group(),
            self.about_group(),
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

        self.screen("settings_advanced", "settings.advanced_title", components)
    }

    fn flip_toggle(&mut self, component_id: &str, item_id: &str) {
        let config = &mut self.config;
        match (component_id, item_id) {
            ("privacy", "delivery_receipts") => {
                config.delivery_receipts_enabled = !config.delivery_receipts_enabled;
            }
            ("privacy", "suppress_presence") => {
                config.suppress_presence = !config.suppress_presence;
            }
            ("privacy", "new_field_default") => {
                config.new_field_default_visible = !config.new_field_default_visible;
            }
            ("privacy", "contact_added") => {
                config.contact_added_notifications = !config.contact_added_notifications;
            }
            ("privacy", "card_update") => {
                config.card_update_notifications = !config.card_update_notifications;
            }
            ("accessibility", "reduce_motion") => config.reduce_motion = !config.reduce_motion,
            ("accessibility", "large_touch") => config.large_touch = !config.large_touch,
            ("appearance", "show_help_icons") => config.show_help_icons = !config.show_help_icons,
            ("about", "debug_mode") => config.debug_mode = !config.debug_mode,
            _ => {}
        }
    }

    fn cycle_backup_reminders(&mut self) {
        let current = vauchi_core::types::ReminderFrequency::from_label(
            &self.config.backup_reminder_frequency,
        );
        self.config.backup_reminder_frequency = current.next().label().to_string();
    }
}

impl WorkflowEngine for SettingsEngine {
    fn current_screen(&self) -> ScreenModel {
        match self.mode {
            SettingsMode::Main => self.main_screen(),
            SettingsMode::Advanced => self.advanced_screen(),
            SettingsMode::Appearance => self.appearance_screen(),
            SettingsMode::Accessibility => self.accessibility_screen(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } => {
                self.flip_toggle(component_id, item_id);
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "backup" && item_id == "backup_reminders" => {
                self.cycle_backup_reminders();
                ActionResult::UpdateScreen(self.current_screen())
            }
            // Theme + Language Dropdown selections — persistence happens
            // in `app_engine::intercept::persist_settings_toggle`, which
            // writes the new value into the engine's RenderContext.
            // Engine mirrors the new id locally so the screen reflects
            // the pick on the very next render; the fresh config built
            // on re-entry re-derives the id from the engine's
            // RenderContext (ADR-047).
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
            } if component_id == "about" && item_id == "what_is_vauchi" => {
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
