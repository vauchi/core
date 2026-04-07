// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stateless evaluator that converts new activity log entries into OS notifications.

use crate::notification_types::{
    ActivityLogEntry, EventOrigin, NotificationCategory, NotificationPreferences,
    PendingNotification,
};

/// Stateless evaluator: converts new activity log entries into `PendingNotification`s.
pub struct NotificationEmitter;

impl NotificationEmitter {
    /// Evaluate new log entries against preferences.
    ///
    /// `name_resolver` maps `contact_id → display name` (used for emergency notifications).
    pub fn evaluate<F>(
        new_entries: &[(String, ActivityLogEntry)],
        prefs: &NotificationPreferences,
        name_resolver: F,
    ) -> Vec<PendingNotification>
    where
        F: Fn(&str) -> String,
    {
        let mut notifications = Vec::new();

        for (event_key, entry) in new_entries {
            if let Some(notification) =
                Self::evaluate_entry(event_key, entry, prefs, &name_resolver)
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
                    title: "Emergency Alert".to_string(),
                    body: format!("{name} sent an emergency alert"),
                    contact_id: contact_id.clone(),
                })
            }
            ActivityLogEntry::ContactAdded {
                contact_id,
                origin: EventOrigin::Synced,
            } if prefs.contact_added_enabled => Some(PendingNotification {
                event_key: event_key.to_string(),
                category: NotificationCategory::ContactAdded,
                title: "Vauchi".to_string(),
                body: "New contact added".to_string(),
                contact_id: contact_id.clone(),
            }),
            _ => None,
        }
    }
}
