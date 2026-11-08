// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared types for the notification and activity log system.

use serde::{Deserialize, Serialize};

pub use vauchi_core::EventOrigin;
pub use vauchi_core::activity_log::ActivityLogEntry;

/// Notification categories that produce OS notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum NotificationCategory {
    /// Always fires, no toggle.
    EmergencyAlert,
    /// Opt-in, off by default.
    ContactAdded,
    /// A contact updated their card. The product's core heartbeat
    /// ("Anna updated her number"). Default **on** (M4 S3,
    /// 2026-07-03-notifications-never-authorized).
    CardUpdate,
}

/// Presentation urgency for an OS notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum NotificationPriority {
    Default,
    High,
    Urgent,
}

/// User preferences for OS notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct NotificationPreferences {
    /// Whether contact-added notifications are enabled. Default: false.
    #[serde(default)]
    pub contact_added_enabled: bool,
    /// Whether card-update notifications are enabled. Default: **true** —
    /// this is the app's reason to exist; the placebo bug was that no such
    /// notification existed at all (M4 S3). A per-contact mute is a planned
    /// follow-up (needs per-contact preference storage).
    #[serde(default = "default_true")]
    pub card_update_enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            contact_added_enabled: false,
            card_update_enabled: true,
        }
    }
}

/// An OS notification for the frontend to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PendingNotification {
    pub event_key: String,
    pub category: NotificationCategory,
    pub title: String,
    pub body: String,
    pub contact_id: String,
    /// Deep-link URI the frontend should open when the user taps the
    /// notification. Core owns the URI format (ADR-021); the shell forwards
    /// it verbatim via `UserAction::LinkOpened`.
    pub deep_link_uri: Option<String>,
    /// Opaque OS category identifier (iOS/macOS `UNNotificationCategory`
    /// identifier). Core mints the id; the shell must not interpret it as a
    /// domain concept (ADR-043 Am6).
    pub os_category_id: String,
    /// Opaque OS channel identifier (Android `NotificationChannel` id). Core
    /// mints the id; the shell must not interpret it as a domain concept
    /// (ADR-043 Am6).
    pub os_channel_id: String,
    /// Presentation urgency. The shell maps this to OS-specific priority /
    /// importance hints; it must not switch on the notification category.
    pub priority: NotificationPriority,
    /// Opaque tokens that control OS-specific category options. The shell maps
    /// each token to the corresponding platform option.
    pub os_category_options: Vec<String>,
}
