// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI notification types for mobile platforms.

/// OS notification category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileNotificationCategory {
    EmergencyAlert,
    /// A contact sent a duress alert (they entered their duress PIN). Distinct
    /// from `EmergencyAlert` so the frontend can convey the coercion context.
    DuressAlert,
    ContactAdded,
    /// A contact updated their card (M4 S3). Default-on heartbeat.
    CardUpdate,
}

/// Presentation urgency mirrored across the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileNotificationPriority {
    Default,
    High,
    Urgent,
}

/// A pending OS notification for the frontend to render.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobilePendingNotification {
    pub event_key: String,
    pub category: MobileNotificationCategory,
    pub title: String,
    pub body: String,
    pub contact_id: String,
    /// Deep-link URI the frontend should open when the user taps the
    /// notification. `None` when the notification has no navigable target.
    pub deep_link_uri: Option<String>,
    /// Opaque OS category identifier (iOS/macOS `UNNotificationCategory`).
    pub os_category_id: String,
    /// Opaque OS channel identifier (Android `NotificationChannel`).
    pub os_channel_id: String,
    /// Presentation urgency; the shell maps this to OS-specific priority.
    pub priority: MobileNotificationPriority,
    /// Opaque tokens that control OS-specific category options.
    pub os_category_options: Vec<String>,
}
