// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stateless evaluator that converts new activity log entries into OS notifications.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::notification_types::{
    ActivityLogEntry, EventOrigin, NotificationCategory, NotificationPreferences,
    NotificationPriority, PendingNotification,
};

/// Stateless evaluator: converts new activity log entries into `PendingNotification`s.
pub struct NotificationEmitter;

impl NotificationEmitter {
    /// Evaluate new log entries against preferences.
    ///
    /// `name_resolver` maps `contact_id → display name`. `locale` localizes
    /// the notification copy (M4 S3 — copy was hardcoded English).
    pub fn evaluate<F>(
        new_entries: &[(String, ActivityLogEntry)],
        prefs: &NotificationPreferences,
        locale: Locale,
        name_resolver: F,
    ) -> Vec<PendingNotification>
    where
        F: Fn(&str) -> String,
    {
        let mut notifications = Vec::new();

        for (event_key, entry) in new_entries {
            if let Some(notification) =
                Self::evaluate_entry(event_key, entry, prefs, locale, &name_resolver)
            {
                notifications.push(notification);
            }
        }

        notifications
    }

    fn evaluate_entry<F>(
        event_key: &str,
        entry: &ActivityLogEntry,
        prefs: &NotificationPreferences,
        locale: Locale,
        name_resolver: &F,
    ) -> Option<PendingNotification>
    where
        F: Fn(&str) -> String,
    {
        match entry {
            ActivityLogEntry::EmergencyAlertReceived { contact_id } => {
                let name = name_resolver(contact_id);
                let (os_category_id, os_channel_id, priority, os_category_options) =
                    category_hints(NotificationCategory::EmergencyAlert);
                Some(PendingNotification {
                    event_key: event_key.to_string(),
                    category: NotificationCategory::EmergencyAlert,
                    title: get_string(locale, "notification.emergency_alert.title"),
                    body: get_string_with_args(
                        locale,
                        "notification.emergency_alert.body",
                        &[("name", &name)],
                    ),
                    contact_id: contact_id.clone(),
                    deep_link_uri: Some(contact_deep_link(contact_id)),
                    os_category_id,
                    os_channel_id,
                    priority,
                    os_category_options,
                })
            }
            ActivityLogEntry::DuressAlertReceived { contact_id } => {
                let name = name_resolver(contact_id);
                let (os_category_id, os_channel_id, priority, os_category_options) =
                    category_hints(NotificationCategory::DuressAlert);
                Some(PendingNotification {
                    event_key: event_key.to_string(),
                    category: NotificationCategory::DuressAlert,
                    title: get_string(locale, "notification.duress_alert.title"),
                    body: get_string_with_args(
                        locale,
                        "notification.duress_alert.body",
                        &[("name", &name)],
                    ),
                    contact_id: contact_id.clone(),
                    deep_link_uri: Some(contact_deep_link(contact_id)),
                    os_category_id,
                    os_channel_id,
                    priority,
                    os_category_options,
                })
            }
            ActivityLogEntry::ContactAdded {
                contact_id,
                origin: EventOrigin::Synced,
            } if prefs.contact_added_enabled => {
                let (os_category_id, os_channel_id, priority, os_category_options) =
                    category_hints(NotificationCategory::ContactAdded);
                Some(PendingNotification {
                    event_key: event_key.to_string(),
                    category: NotificationCategory::ContactAdded,
                    title: get_string(locale, "notification.app_name"),
                    body: get_string(locale, "notification.contact_added.body"),
                    contact_id: contact_id.clone(),
                    deep_link_uri: Some(contact_deep_link(contact_id)),
                    os_category_id,
                    os_channel_id,
                    priority,
                    os_category_options,
                })
            }
            ActivityLogEntry::CardUpdateReceived { contact_id, .. }
                if prefs.card_update_enabled =>
            {
                let name = name_resolver(contact_id);
                let (os_category_id, os_channel_id, priority, os_category_options) =
                    category_hints(NotificationCategory::CardUpdate);
                Some(PendingNotification {
                    event_key: event_key.to_string(),
                    category: NotificationCategory::CardUpdate,
                    title: get_string(locale, "notification.app_name"),
                    body: get_string_with_args(
                        locale,
                        "activity_log.card_update_received",
                        &[("name", &name)],
                    ),
                    contact_id: contact_id.clone(),
                    deep_link_uri: Some(contact_deep_link(contact_id)),
                    os_category_id,
                    os_channel_id,
                    priority,
                    os_category_options,
                })
            }
            _ => None,
        }
    }
}

/// Core-owned deep-link URI for a contact.
///
/// The shell must not construct navigation targets (ADR-021, ADR-043 Am4).
/// The URI is emitted as part of the notification payload so the shell only
/// forwards it via `UserAction::LinkOpened` when the user taps.
fn contact_deep_link(contact_id: &str) -> String {
    format!("vauchi://contact/{contact_id}")
}

/// OS-specific presentation hints for a notification category.
///
/// The returned ids are opaque tokens minted by core. The shell uses them
/// verbatim and must not branch on the notification category to choose OS
/// channels, categories, or priorities (ADR-043 Am6).
fn category_hints(
    category: NotificationCategory,
) -> (String, String, NotificationPriority, Vec<String>) {
    match category {
        NotificationCategory::EmergencyAlert => (
            "emergency_alert".into(),
            "alerts".into(),
            NotificationPriority::Urgent,
            vec!["custom_dismiss_action".into()],
        ),
        NotificationCategory::DuressAlert => (
            "duress_alert".into(),
            "duress".into(),
            NotificationPriority::Urgent,
            vec![],
        ),
        NotificationCategory::ContactAdded => (
            "contact_added".into(),
            "updates".into(),
            NotificationPriority::Default,
            vec![],
        ),
        NotificationCategory::CardUpdate => (
            "card_update".into(),
            "updates".into(),
            NotificationPriority::Default,
            vec![],
        ),
    }
}
