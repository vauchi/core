// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings row and group builders — the `SettingsGroup`s and dropdowns
//! the `SettingsEngine` screens are assembled from. The main screen is
//! the design canvas's three sections; everything the canvas does not
//! show is built here for the Advanced, Appearance & Language and
//! Accessibility sub-screens.

use super::SettingsEngine;
use crate::i18n::get_string_with_args;
use crate::ui::*;

impl SettingsEngine {
    /// A tappable row. `subtitle` is the canvas's one-line description
    /// under the label; `hint` is the assistive-tech description.
    fn link_row(
        &self,
        id: &str,
        label: String,
        hint: Option<String>,
        subtitle: Option<String>,
        detail: Option<String>,
    ) -> SettingsItem {
        SettingsItem {
            id: id.into(),
            a11y: Some(A11y {
                label: Some(label.clone()),
                hint,
                role: None,
            }),
            label,
            subtitle,
            kind: SettingsItemKind::Link { detail },
            info_key: None,
        }
    }

    /// A toggle row whose hint doubles as the visible description, so a
    /// sighted reader and a screen-reader user get the same explanation.
    fn toggle_row(
        &self,
        id: &str,
        label_key: &str,
        enabled: bool,
        a11y_label: String,
        hint_key: &str,
    ) -> SettingsItem {
        let hint = self.t(hint_key);
        SettingsItem {
            id: id.into(),
            label: self.t(label_key),
            subtitle: Some(hint.clone()),
            kind: SettingsItemKind::Toggle { enabled },
            a11y: Some(A11y {
                label: Some(a11y_label),
                hint: Some(hint),
                role: None,
            }),
            info_key: None,
        }
    }

    fn generic_toggle_label(&self, label_key: &str) -> String {
        get_string_with_args(
            self.locale(),
            "a11y.toggle_label",
            &[("name", &self.t(label_key))],
        )
    }

    /// A row whose only job is to reach a screen the navigation no longer
    /// offers.
    ///
    /// The label reuses the destination's own locale key instead of
    /// minting a second name for one screen — two names for Recovery drift
    /// apart in translation. The route lives in
    /// `intercept_settings_action`, keyed on `id`.
    fn destination_row(&self, id: &str, label_key: &str, subtitle: Option<String>) -> SettingsItem {
        self.link_row(id, self.t(label_key), None, subtitle, None)
    }

    fn device_count_detail(&self) -> String {
        if self.config.device_count == 1 {
            self.t("settings.device_count_one")
        } else {
            get_string_with_args(
                self.locale(),
                "settings.device_count",
                &[("count", &self.config.device_count.to_string())],
            )
        }
    }

    pub(super) fn identity_group(&self) -> Component {
        Component::SettingsGroup {
            id: "identity".into(),
            label: self.t("settings.my_identity_group"),
            items: vec![
                // Link (not Value): Value rows are non-tappable in every
                // renderer, which orphaned the rename handler
                // (2026-04-06-display-name-rename-fails). Link emits
                // ListItemSelected{display_name} → EditName dialog.
                self.link_row(
                    "display_name",
                    self.t("settings.display_name"),
                    Some(self.t("settings.display_name_hint")),
                    None,
                    Some(self.config.display_name.clone()),
                ),
                self.link_row(
                    "edit_profile",
                    self.t("settings.my_contact_info"),
                    Some(self.t("settings.edit_profile_hint")),
                    None,
                    None,
                ),
                self.link_row(
                    "devices",
                    self.t("settings.my_devices"),
                    Some(self.t("settings.devices_hint")),
                    None,
                    Some(self.device_count_detail()),
                ),
                self.link_row(
                    "backup_export",
                    self.t("settings.backup"),
                    Some(self.t("settings.backup_export_hint")),
                    Some(get_string_with_args(
                        self.locale(),
                        "settings.backup_last_hint",
                        &[("when", &self.config.last_backup_display)],
                    )),
                    None,
                ),
                self.destination_row(
                    "recovery",
                    "nav.recovery",
                    Some(self.t("settings.recovery_hint")),
                ),
            ],
        }
    }

    pub(super) fn privacy_group(&self) -> Component {
        Component::SettingsGroup {
            id: "privacy".into(),
            label: self.t("settings.privacy"),
            items: vec![
                self.link_row(
                    "privacy",
                    self.t("settings.your_data"),
                    None,
                    Some(self.t("settings.your_data_hint")),
                    None,
                ),
                self.toggle_row(
                    "suppress_presence",
                    "settings.suppress_presence",
                    self.config.suppress_presence,
                    self.t("settings.suppress_presence_a11y"),
                    "settings.suppress_presence_hint",
                ),
                self.toggle_row(
                    "delivery_receipts",
                    "settings.delivery_receipts",
                    self.config.delivery_receipts_enabled,
                    self.t("settings.delivery_receipts_a11y"),
                    "settings.delivery_receipts_hint",
                ),
                self.toggle_row(
                    "new_field_default",
                    "settings.new_field_default",
                    self.config.new_field_default_visible,
                    self.t("settings.new_field_default_a11y"),
                    "settings.new_field_default_hint",
                ),
                // The card-update heartbeat (M4 S3). Default-on; this is the
                // toggle that makes the notification honestly disable-able.
                self.toggle_row(
                    "card_update",
                    "settings.card_updates",
                    self.config.card_update_notifications,
                    self.t("settings.card_updates_a11y"),
                    "settings.card_updates_hint",
                ),
                self.toggle_row(
                    "contact_added",
                    "settings.contact_added",
                    self.config.contact_added_notifications,
                    self.t("settings.contact_added_a11y"),
                    "settings.contact_added_hint",
                ),
                self.destination_row("activity_log", "nav.activity", None),
            ],
        }
    }

    pub(super) fn app_group(&self) -> Component {
        Component::SettingsGroup {
            id: "app".into(),
            label: self.t("settings.app_group"),
            items: vec![
                self.link_row(
                    "appearance",
                    self.t("settings.appearance_language"),
                    None,
                    None,
                    None,
                ),
                self.link_row(
                    "accessibility",
                    self.t("settings.accessibility"),
                    None,
                    Some(self.t("settings.accessibility_hint")),
                    None,
                ),
                self.link_row(
                    "help_center",
                    self.t("settings.help"),
                    Some(self.t("settings.help_center_hint")),
                    None,
                    None,
                ),
                // Opens the buried sub-screen (M6 D6.1): network, delivery
                // status, emergency wipe and the rows the canvas dropped.
                self.link_row(
                    "advanced",
                    self.t("settings.advanced"),
                    Some(self.t("settings.advanced_hint")),
                    None,
                    None,
                ),
            ],
        }
    }

    pub(super) fn appearance_group(&self) -> Component {
        Component::SettingsGroup {
            id: "appearance".into(),
            label: self.t("settings.appearance"),
            items: vec![self.toggle_row(
                "show_help_icons",
                "settings.show_help_icons",
                self.config.show_help_icons,
                self.generic_toggle_label("settings.show_help_icons"),
                "settings.show_help_icons_hint",
            )],
        }
    }

    fn follow_system_dropdown(
        &self,
        id: &str,
        label_key: &str,
        hint_key: &str,
        selected: &str,
        options: &[DropdownOption],
    ) -> Component {
        // Theme + Language are first-class Component::Dropdown so they
        // render inline (no separate picker). The selected id mirrors
        // RenderContext: the reserved "follow_system" id stands in for
        // None (ADR-047 absence-is-follow-system). Action dispatch is
        // `UserAction::ListItemSelected` with component_id matching the
        // dropdown id.
        let mut opts = vec![DropdownOption {
            id: "follow_system".into(),
            label: self.t("theme.system"),
        }];
        opts.extend(options.iter().cloned());
        Component::Dropdown {
            id: id.into(),
            label: self.t(label_key),
            selected: Some(selected.into()),
            options: opts,
            a11y: Some(A11y {
                label: Some(self.t(label_key)),
                hint: Some(self.t(hint_key)),
                role: None,
            }),
        }
    }

    pub(super) fn theme_dropdown(&self) -> Component {
        self.follow_system_dropdown(
            "theme",
            "settings.theme",
            "settings.theme_hint",
            &self.config.theme_id,
            &self.config.available_themes,
        )
    }

    pub(super) fn language_dropdown(&self) -> Component {
        self.follow_system_dropdown(
            "language",
            "settings.language",
            "settings.language_hint",
            &self.config.language_id,
            &self.config.available_languages,
        )
    }

    pub(super) fn accessibility_group(&self) -> Component {
        Component::SettingsGroup {
            id: "accessibility".into(),
            label: self.t("settings.accessibility"),
            items: vec![
                self.toggle_row(
                    "reduce_motion",
                    "a11y.reduce_motion",
                    self.config.reduce_motion,
                    self.generic_toggle_label("a11y.reduce_motion"),
                    "settings.reduce_motion_hint",
                ),
                // high_contrast deferred to M4 S1b: its effect is theme
                // colors (frontend-applied via theme_id), so it needs core
                // effective-theme resolution + per-platform wiring. Removed
                // here rather than shipped as a persisted-but-inert toggle
                // (ship-or-delete, design D4.1).
                self.toggle_row(
                    "large_touch",
                    "settings.large_touch_targets",
                    self.config.large_touch,
                    self.generic_toggle_label("settings.large_touch_targets"),
                    "settings.large_touch_targets_hint",
                ),
            ],
        }
    }

    pub(super) fn security_group(&self) -> Component {
        let mut duress_pin = self.link_row(
            "duress_pin",
            self.t("info.duress_pin.title"),
            Some(self.t("settings.duress_pin_hint")),
            None,
            None,
        );
        duress_pin.info_key = self.config.show_help_icons.then(|| "duress_pin".into());
        Component::SettingsGroup {
            id: "security".into(),
            label: self.t("settings.security"),
            items: vec![
                self.link_row(
                    "change_password",
                    self.t("settings.change_password"),
                    Some(self.t("settings.change_password_hint")),
                    None,
                    None,
                ),
                duress_pin,
                self.link_row(
                    "decoy_contacts",
                    self.t("resistance.duress.decoy_contacts"),
                    Some(self.t("settings.decoy_contacts_hint")),
                    None,
                    None,
                ),
                self.link_row(
                    "setup_new_device",
                    self.t("settings.setup_new_device"),
                    Some(self.t("settings.setup_new_device_hint")),
                    None,
                    None,
                ),
            ],
        }
    }

    pub(super) fn backup_group(&self) -> Component {
        Component::SettingsGroup {
            id: "backup".into(),
            label: self.t("settings.backup"),
            // Link (not Value): same orphan class as display_name — Value
            // is non-tappable, so the frequency-cycle handler was
            // unreachable. Link emits ListItemSelected{backup_reminders}.
            items: vec![SettingsItem {
                id: "backup_reminders".into(),
                label: self.t("settings.backup_reminders"),
                subtitle: None,
                kind: SettingsItemKind::Link {
                    detail: Some(self.config.backup_reminder_frequency.clone()),
                },
                a11y: Some(A11y {
                    label: Some(self.t("settings.backup_reminders_a11y")),
                    hint: Some(self.t("settings.backup_reminders_hint")),
                    role: None,
                }),
                info_key: None,
            }],
        }
    }

    pub(super) fn network_group(&self) -> Component {
        Component::SettingsGroup {
            id: "network".into(),
            label: self.t("settings.network"),
            // Link, not Value: renderers wire taps (→ ListItemSelected →
            // EditRelayUrl dialog) only on Link rows; as a Value row the
            // editor was unreachable on mobile (device regression
            // 2026-06-10, `2026-06-10-mobile-relay-url-editor-noop`).
            items: vec![self.link_row(
                "relay_url",
                self.t("settings.relay_url"),
                Some(self.t("settings.relay_url_hint")),
                None,
                Some(self.config.relay_url.clone()),
            )],
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
                    subtitle: None,
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
                // Link into the DeliveryStatus retry screen (M4 S2 — was a
                // dead Value counter; the screen was a reachable-by-nothing
                // orphan). Detail shows the live failed count.
                self.link_row(
                    "failed_deliveries",
                    self.t("settings.failed_deliveries"),
                    Some(self.t("settings.failed_deliveries_hint")),
                    None,
                    Some(self.config.failed_deliveries.to_string()),
                ),
            ],
        }
    }

    pub(super) fn about_group(&self) -> Component {
        let version = if self.config.build.is_empty() {
            self.config.version.clone()
        } else {
            format!("{} ({})", self.config.version, self.config.build)
        };
        Component::SettingsGroup {
            id: "about".into(),
            label: self.t("settings.about"),
            items: vec![
                self.link_row(
                    "what_is_vauchi",
                    self.t("about.what_is_vauchi.title"),
                    Some(self.t("settings.what_is_vauchi_hint")),
                    None,
                    None,
                ),
                self.link_row(
                    "funding",
                    self.t("settings.funding"),
                    Some(self.t("settings.funding_hint")),
                    None,
                    None,
                ),
                self.destination_row("support", "nav.support", None),
                SettingsItem {
                    id: "version".into(),
                    label: self.t("settings.version"),
                    subtitle: None,
                    kind: SettingsItemKind::Value { value: version },
                    a11y: Some(A11y {
                        label: Some(self.t("settings.version")),
                        hint: Some(self.t("settings.version_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
                self.toggle_row(
                    "debug_mode",
                    "settings.debug_mode",
                    self.config.debug_mode,
                    self.generic_toggle_label("settings.debug_mode"),
                    "settings.debug_mode_hint",
                ),
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
                subtitle: None,
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
}
