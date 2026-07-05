// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stateless evaluator that converts new activity log entries into OS notifications.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::notification_types::{
    ActivityLogEntry, EventOrigin, NotificationCategory, NotificationPreferences,
    PendingNotification,
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
                })
            }
            ActivityLogEntry::DuressAlertReceived { contact_id } => {
                let name = name_resolver(contact_id);
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
                })
            }
            ActivityLogEntry::ContactAdded {
                contact_id,
                origin: EventOrigin::Synced,
            } if prefs.contact_added_enabled => Some(PendingNotification {
                event_key: event_key.to_string(),
                category: NotificationCategory::ContactAdded,
                title: get_string(locale, "notification.app_name"),
                body: get_string(locale, "notification.contact_added.body"),
                contact_id: contact_id.clone(),
            }),
            ActivityLogEntry::CardUpdateReceived { contact_id, .. }
                if prefs.card_update_enabled =>
            {
                let name = name_resolver(contact_id);
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
                })
            }
            _ => None,
        }
    }
}
