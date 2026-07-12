// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persisted user-settings flags.
//!
//! A neutral leaf module: shared by `storage` (persistence) and `api`
//! (`VauchiConfig` seeding) without depending on either.

fn default_true() -> bool {
    true
}

/// Persisted user settings toggles, the core source of truth for the
/// three config-backed flags. Encrypted in the `ux_state` table and
/// self-seeded into `VauchiConfig` on construction so every `Vauchi`
/// instance (mobile PAE engine, `open_vauchi()` transients, desktop)
/// reads a consistent value that survives restart. Defaults mirror
/// `VauchiConfig`'s defaults (settings-toggle-not-persisting P1).
///
/// `#[non_exhaustive]` so adding a flag is a non-breaking change (mirrors
/// `VauchiConfig`): out-of-crate code builds it via `From<&VauchiConfig>`
/// or `default()` + field assignment, never a struct literal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct SettingsFlags {
    /// Send read/delivery receipts. Defaults to true.
    #[serde(default = "default_true")]
    pub delivery_receipts_enabled: bool,
    /// Suppress presence announcements to the relay. Defaults to false.
    #[serde(default)]
    pub suppress_presence: bool,
    /// Notify when a new contact is added. Defaults to false.
    #[serde(default)]
    pub contact_added_notifications: bool,
    /// Notify when a contact updates their card. Defaults to **true** — the
    /// product's core heartbeat (M4 S3). `default_true` for back-compat with
    /// flags stored before this field existed.
    #[serde(default = "default_true")]
    pub card_update_notifications: bool,
    /// Reduce/eliminate UI motion (zeroes animation durations). Defaults
    /// to false. Category-2 accessibility flag — core-owned so the
    /// accommodation follows the user across devices (ADR-047 Addendum
    /// 2026-07-05). `#[serde(default)]` for back-compat with flags stored
    /// before this field existed.
    #[serde(default)]
    pub reduce_motion: bool,
    /// Enlarge touch targets + list-item spacing. Defaults to false.
    /// Category-2 accessibility flag (see `reduce_motion`).
    #[serde(default)]
    pub large_touch: bool,
    /// One-time field-centric visibility grandfathering ran (or the install
    /// is fresh enough to never need it). `#[serde(default)]` = false is the
    /// trigger for installs that predate the model
    /// (2026-07-05-ungrouped-contacts-default-open).
    #[serde(default)]
    pub field_centric_visibility_migrated: bool,
    /// New contact-card entries start Visible (explicit `Everyone`
    /// materialized at `add_own_field` time). Defaults to false = hidden
    /// (2026-07-05-ungrouped-contacts-default-open, Decision 2).
    #[serde(default)]
    pub new_field_default_visible: bool,
    /// One-shot first-group education banner already shown (Decision 3,
    /// 2026-07-05-ungrouped-contacts-default-open).
    #[serde(default)]
    pub first_group_education_shown: bool,
}

impl Default for SettingsFlags {
    fn default() -> Self {
        Self {
            delivery_receipts_enabled: true,
            suppress_presence: false,
            contact_added_notifications: false,
            card_update_notifications: true,
            reduce_motion: false,
            large_touch: false,
            field_centric_visibility_migrated: false,
            new_field_default_visible: false,
            first_group_education_shown: false,
        }
    }
}

impl SettingsFlags {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
