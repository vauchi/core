// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings screen engine — displays app settings grouped by category.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use serde::{Deserialize, Serialize};

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

    fn profile_group(&self) -> Component {
        Component::SettingsGroup {
            id: "profile".into(),
            label: self.t("settings.profile_group"),
            items: vec![
                SettingsItem {
                    id: "display_name".into(),
                    label: self.t("settings.display_name"),
                    // Link (not Value): Value rows are non-tappable in every
                    // Humble UI renderer, which orphaned the rename handler
                    // (2026-04-06-display-name-rename-fails). Link emits
                    // ListItemSelected{display_name} → EditName dialog.
                    kind: SettingsItemKind::Link {
                        detail: Some(self.config.display_name.clone()),
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.display_name")),
                        hint: Some(self.t("settings.display_name_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "edit_profile".into(),
                    label: self.t("settings.edit_profile"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.edit_profile")),
                        hint: Some(self.t("settings.edit_profile_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn privacy_group(&self) -> Component {
        Component::SettingsGroup {
            id: "privacy".into(),
            label: self.t("settings.privacy"),
            items: vec![
                SettingsItem {
                    id: "delivery_receipts".into(),
                    label: self.t("settings.delivery_receipts"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.delivery_receipts_enabled,
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.delivery_receipts_a11y")),
                        hint: Some(self.t("settings.delivery_receipts_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "suppress_presence".into(),
                    label: self.t("settings.suppress_presence"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.suppress_presence,
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.suppress_presence_a11y")),
                        hint: Some(self.t("settings.suppress_presence_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "new_field_default".into(),
                    label: self.t("settings.new_field_default"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.new_field_default_visible,
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.new_field_default_a11y")),
                        hint: Some(self.t("settings.new_field_default_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn notifications_group(&self) -> Component {
        Component::SettingsGroup {
            id: "notifications".into(),
            label: self.t("settings.notifications_group"),
            items: vec![
                // The card-update heartbeat (M4 S3). Default-on; this is the
                // toggle that makes the notification honestly disable-able.
                SettingsItem {
                    id: "card_update".into(),
                    label: self.t("settings.card_updates"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.card_update_notifications,
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.card_updates_a11y")),
                        hint: Some(self.t("settings.card_updates_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "contact_added".into(),
                    label: self.t("settings.contact_added"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.contact_added_notifications,
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.contact_added_a11y")),
                        hint: Some(self.t("settings.contact_added_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn appearance_group(&self) -> Component {
        Component::SettingsGroup {
            id: "appearance".into(),
            label: self.t("settings.appearance"),
            items: vec![SettingsItem {
                id: "show_help_icons".into(),
                label: self.t("settings.show_help_icons"),
                kind: SettingsItemKind::Toggle {
                    enabled: self.config.show_help_icons,
                },
                a11y: Some(A11y {
                    label: None,
                    hint: Some(self.t("settings.show_help_icons_hint")),
                    role: None,
                }),
                info_key: None,
            }],
        }
    }

    fn theme_dropdown(&self) -> Component {
        // Theme + Language dropdowns are first-class Component::Dropdown
        // so they can render inline (no separate sub-screen). The
        // selected_id mirrors RenderContext: the reserved "follow_system"
        // id stands in for None (ADR-047 absence-is-follow-system).
        // Action dispatch is `UserAction::ListItemSelected` with
        // component_id matching the dropdown id.
        Component::Dropdown {
            id: "theme".into(),
            label: self.t("settings.theme"),
            selected: Some(self.config.theme_id.clone()),
            options: {
                let mut opts = vec![DropdownOption {
                    id: "follow_system".into(),
                    label: self.t("theme.system"),
                }];
                opts.extend(self.config.available_themes.iter().cloned());
                opts
            },
            a11y: Some(A11y {
                label: Some(self.t("settings.theme")),
                hint: Some(self.t("settings.theme_hint")),
                role: None,
            }),
        }
    }

    fn language_dropdown(&self) -> Component {
        Component::Dropdown {
            id: "language".into(),
            label: self.t("settings.language"),
            selected: Some(self.config.language_id.clone()),
            options: {
                let mut opts = vec![DropdownOption {
                    id: "follow_system".into(),
                    label: self.t("theme.system"),
                }];
                opts.extend(self.config.available_languages.iter().cloned());
                opts
            },
            a11y: Some(A11y {
                label: Some(self.t("settings.language")),
                hint: Some(self.t("settings.language_hint")),
                role: None,
            }),
        }
    }

    fn accessibility_group(&self) -> Component {
        Component::SettingsGroup {
            id: "accessibility".into(),
            label: self.t("settings.accessibility"),
            items: vec![
                SettingsItem {
                    id: "reduce_motion".into(),
                    label: self.t("a11y.reduce_motion"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.reduce_motion,
                    },
                    a11y: Some(A11y {
                        label: Some(get_string_with_args(
                            self.locale(),
                            "a11y.toggle_label",
                            &[("name", &self.t("a11y.reduce_motion"))],
                        )),
                        hint: Some(self.t("settings.reduce_motion_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                // high_contrast deferred to M4 S1b: its effect is theme
                // colors (frontend-applied via theme_id), so it needs core
                // effective-theme resolution + per-platform wiring. Removed
                // here rather than shipped as a persisted-but-inert toggle
                // (ship-or-delete, design D4.1).
                SettingsItem {
                    id: "large_touch".into(),
                    label: self.t("settings.large_touch_targets"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.large_touch,
                    },
                    a11y: Some(A11y {
                        label: Some(get_string_with_args(
                            self.locale(),
                            "a11y.toggle_label",
                            &[("name", &self.t("settings.large_touch_targets"))],
                        )),
                        hint: Some(self.t("settings.large_touch_targets_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn security_group(&self) -> Component {
        let show_help = self.config.show_help_icons;
        Component::SettingsGroup {
            id: "security".into(),
            label: self.t("settings.security"),
            items: vec![
                SettingsItem {
                    id: "change_password".into(),
                    label: self.t("settings.change_password"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.change_password")),
                        hint: Some(self.t("settings.change_password_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "devices".into(),
                    label: self.t("devices.count"),
                    kind: SettingsItemKind::Link {
                        detail: Some(if self.config.device_count == 1 {
                            self.t("settings.device_count_one")
                        } else {
                            get_string_with_args(
                                self.locale(),
                                "settings.device_count",
                                &[("count", &self.config.device_count.to_string())],
                            )
                        }),
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("devices.count")),
                        hint: Some(self.t("settings.devices_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "duress_pin".into(),
                    label: self.t("info.duress_pin.title"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("info.duress_pin.title")),
                        hint: Some(self.t("settings.duress_pin_hint")),
                        role: None,
                    }),
                    info_key: if show_help {
                        Some("duress_pin".into())
                    } else {
                        None
                    },
                },
                SettingsItem {
                    id: "decoy_contacts".into(),
                    label: self.t("resistance.duress.decoy_contacts"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("resistance.duress.decoy_contacts")),
                        hint: Some(self.t("settings.decoy_contacts_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn backup_group(&self) -> Component {
        Component::SettingsGroup {
            id: "backup".into(),
            label: self.t("backup.wizard.title"),
            items: vec![
                SettingsItem {
                    id: "backup_export".into(),
                    label: self.t("backup.wizard.create"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("backup.wizard.create")),
                        hint: Some(self.t("settings.backup_export_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "backup_import".into(),
                    label: self.t("backup.wizard.restore"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("backup.wizard.restore")),
                        hint: Some(self.t("settings.backup_import_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "setup_new_device".into(),
                    label: self.t("settings.setup_new_device"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.setup_new_device")),
                        hint: Some(self.t("settings.setup_new_device_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "last_backup".into(),
                    label: self.t("settings.last_backup"),
                    kind: SettingsItemKind::Value {
                        value: self.config.last_backup_display.clone(),
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.last_backup_a11y")),
                        hint: None,
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "backup_reminders".into(),
                    label: self.t("settings.backup_reminders"),
                    // Link (not Value): same orphan class as display_name —
                    // Value is non-tappable, so the frequency-cycle handler was
                    // unreachable. Link emits ListItemSelected{backup_reminders}.
                    kind: SettingsItemKind::Link {
                        detail: Some(self.config.backup_reminder_frequency.clone()),
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.backup_reminders_a11y")),
                        hint: Some(self.t("settings.backup_reminders_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn network_group(&self) -> Component {
        Component::SettingsGroup {
            id: "network".into(),
            label: self.t("settings.network"),
            items: vec![SettingsItem {
                id: "relay_url".into(),
                label: self.t("settings.relay_url"),
                // Link, not Value: renderers wire taps (→ ListItemSelected
                // → EditRelayUrl dialog) only on Link rows; as a Value row
                // the editor was unreachable on mobile (device regression
                // 2026-06-10, `2026-06-10-mobile-relay-url-editor-noop`).
                kind: SettingsItemKind::Link {
                    detail: Some(self.config.relay_url.clone()),
                },
                a11y: Some(A11y {
                    label: Some(self.t("settings.relay_url")),
                    hint: Some(self.t("settings.relay_url_hint")),
                    role: None,
                }),
                info_key: None,
            }],
        }
    }

    fn delivery_group(&self) -> Component {
        Component::SettingsGroup {
            id: "delivery".into(),
            label: self.t("settings.message_delivery"),
            items: vec![
                SettingsItem {
                    id: "pending_updates".into(),
                    label: self.t("sync.pending_updates_title"),
                    kind: SettingsItemKind::Value {
                        value: self.config.pending_updates.to_string(),
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("sync.pending_updates_title")),
                        hint: Some(self.t("settings.pending_updates_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "failed_deliveries".into(),
                    label: self.t("settings.failed_deliveries"),
                    // Link into the DeliveryStatus retry screen (M4 S2 — was a
                    // dead Value counter; the screen was a reachable-by-nothing
                    // orphan). Detail shows the live failed count.
                    kind: SettingsItemKind::Link {
                        detail: Some(self.config.failed_deliveries.to_string()),
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.failed_deliveries")),
                        hint: Some(self.t("settings.failed_deliveries_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn help_group(&self) -> Component {
        Component::SettingsGroup {
            id: "help".into(),
            label: self.t("settings.help_support"),
            items: vec![
                SettingsItem {
                    id: "help_center".into(),
                    label: self.t("settings.help_center"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.help_center")),
                        hint: Some(self.t("settings.help_center_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "funding".into(),
                    label: self.t("settings.funding"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.funding")),
                        hint: Some(self.t("settings.funding_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "privacy_policy".into(),
                    label: self.t("help.privacy_policy"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("help.privacy_policy")),
                        hint: Some(self.t("settings.privacy_policy_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn about_group(&self) -> Component {
        Component::SettingsGroup {
            id: "about".into(),
            label: self.t("settings.about"),
            items: vec![
                SettingsItem {
                    id: "what_is_vauchi".into(),
                    label: self.t("about.what_is_vauchi.title"),
                    kind: SettingsItemKind::Link { detail: None },
                    a11y: Some(A11y {
                        label: Some(self.t("about.what_is_vauchi.title")),
                        hint: Some(self.t("settings.what_is_vauchi_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "version".into(),
                    label: self.t("settings.version"),
                    kind: SettingsItemKind::Value {
                        value: if self.config.build.is_empty() {
                            self.config.version.clone()
                        } else {
                            format!("{} ({})", self.config.version, self.config.build)
                        },
                    },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.version")),
                        hint: Some(self.t("settings.version_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                SettingsItem {
                    id: "debug_mode".into(),
                    label: self.t("settings.debug_mode"),
                    kind: SettingsItemKind::Toggle {
                        enabled: self.config.debug_mode,
                    },
                    a11y: Some(A11y {
                        label: Some(get_string_with_args(
                            self.locale(),
                            "a11y.toggle_label",
                            &[("name", &self.t("settings.debug_mode"))],
                        )),
                        hint: Some(self.t("settings.debug_mode_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
        }
    }

    fn danger_group(&self) -> Component {
        Component::SettingsGroup {
            id: "danger".into(),
            label: self.t("settings.danger_zone"),
            items: vec![SettingsItem {
                id: "emergency_wipe".into(),
                label: self.t("emergency.wipe_button"),
                kind: SettingsItemKind::Destructive {
                    label: self.t("shred.wipe.wipe_all"),
                },
                a11y: Some(A11y {
                    label: Some(self.t("emergency.wipe_button")),
                    hint: Some(self.t("settings.emergency_wipe_hint")),
                    role: None,
                }),
                info_key: None,
            }],
        }
    }

    /// The "Advanced…" entry on the main list that navigates to the
    /// advanced sub-screen (M6 D6.1). Emits `ListItemSelected{advanced}`,
    /// routed to `AppScreen::SettingsAdvanced` in `intercept.rs`.
    fn advanced_link(&self) -> Component {
        Component::SettingsGroup {
            id: "advanced_nav".into(),
            label: String::new(),
            items: vec![SettingsItem {
                id: "advanced".into(),
                label: self.t("settings.advanced"),
                kind: SettingsItemKind::Link { detail: None },
                a11y: Some(A11y {
                    label: Some(self.t("settings.advanced")),
                    hint: Some(self.t("settings.advanced_hint")),
                    role: None,
                }),
                info_key: None,
            }],
        }
    }

    /// Merge two `SettingsGroup`s into one under a new id + label, keeping
    /// their items in order (M6 S1b toward the everyday-6 IA). Combining
    /// the existing builders avoids transcribing items; the toggle
    /// handlers match the merged component_id.
    fn merged_group(
        &self,
        id: &str,
        label_key: &str,
        first: Component,
        second: Component,
    ) -> Component {
        let items = |c: Component| match c {
            Component::SettingsGroup { items, .. } => items,
            _ => unreachable!("group builder must return a SettingsGroup"),
        };
        Component::SettingsGroup {
            id: id.into(),
            label: self.t(label_key),
            items: items(first).into_iter().chain(items(second)).collect(),
        }
    }

    fn privacy_notifications_group(&self) -> Component {
        self.merged_group(
            "privacy_notifications",
            "settings.privacy_notifications_group",
            self.privacy_group(),
            self.notifications_group(),
        )
    }

    fn security_backup_group(&self) -> Component {
        self.merged_group(
            "security_backup",
            "settings.security_backup_group",
            self.security_group(),
            self.backup_group(),
        )
    }

    fn help_about_group(&self) -> Component {
        self.merged_group(
            "help_about",
            "settings.help_about_group",
            self.help_group(),
            self.about_group(),
        )
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
            actions: vec![],
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
            actions: vec![],
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
