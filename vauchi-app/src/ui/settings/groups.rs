// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings row and group builders — the `SettingsGroup`s and dropdowns
//! the `SettingsEngine` screens are assembled from.

use super::SettingsEngine;
use crate::i18n::get_string_with_args;
use crate::ui::*;

impl SettingsEngine {
    /// A row whose only job is to reach a screen the navigation no longer
    /// offers.
    ///
    /// The label reuses the destination's own locale key instead of
    /// minting a second name for one screen — two names for Recovery drift
    /// apart in translation. The route lives in
    /// `intercept_settings_action`, keyed on `id`.
    pub(super) fn destination_row(&self, id: &str, label_key: &str) -> SettingsItem {
        let label = self.t(label_key);
        SettingsItem {
            id: id.into(),
            label: label.clone(),
            kind: SettingsItemKind::Link { detail: None },
            a11y: Some(A11y {
                label: Some(label),
                hint: None,
                role: None,
            }),
            info_key: None,
        }
    }

    pub(super) fn profile_group(&self) -> Component {
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

    pub(super) fn privacy_group(&self) -> Component {
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
                self.destination_row("privacy", "nav.privacy"),
                self.destination_row("activity_log", "nav.activity"),
            ],
        }
    }

    pub(super) fn notifications_group(&self) -> Component {
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

    pub(super) fn appearance_group(&self) -> Component {
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

    pub(super) fn theme_dropdown(&self) -> Component {
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

    pub(super) fn language_dropdown(&self) -> Component {
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

    pub(super) fn accessibility_group(&self) -> Component {
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

    pub(super) fn security_group(&self) -> Component {
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
                self.destination_row("recovery", "nav.recovery"),
            ],
        }
    }

    pub(super) fn backup_group(&self) -> Component {
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

    pub(super) fn network_group(&self) -> Component {
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

    pub(super) fn delivery_group(&self) -> Component {
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

    pub(super) fn help_group(&self) -> Component {
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
                self.destination_row("support", "nav.support"),
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

    pub(super) fn about_group(&self) -> Component {
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

    pub(super) fn danger_group(&self) -> Component {
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
    pub(super) fn advanced_link(&self) -> Component {
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
    pub(super) fn merged_group(
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

    pub(super) fn privacy_notifications_group(&self) -> Component {
        self.merged_group(
            "privacy_notifications",
            "settings.privacy_notifications_group",
            self.privacy_group(),
            self.notifications_group(),
        )
    }

    pub(super) fn security_backup_group(&self) -> Component {
        self.merged_group(
            "security_backup",
            "settings.security_backup_group",
            self.security_group(),
            self.backup_group(),
        )
    }

    pub(super) fn help_about_group(&self) -> Component {
        self.merged_group(
            "help_about",
            "settings.help_about_group",
            self.help_group(),
            self.about_group(),
        )
    }
}
