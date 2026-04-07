// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based tests for the notification / activity log system (CC-04).
//!
//! Covers:
//! - No duplicate event_keys survive INSERT OR IGNORE deduplication
//! - Prune never keeps entries older than the retention window
//! - Notification count is always ≤ new entry count

use proptest::prelude::*;
use vauchi_app::notification_emitter::NotificationEmitter;
use vauchi_app::notification_types::{ActivityLogEntry, EventOrigin, NotificationPreferences};
use vauchi_core::Storage;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::ActivityLogRow;

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).expect("in-memory storage")
}

// @scenario: activity-log.feature - No duplicate event_keys in activity_log
proptest! {
    #[test]
    fn no_duplicate_event_keys(
        keys in prop::collection::vec("[a-z]{3,10}:[a-z0-9]{3,10}", 1..50),
    ) {
        let storage = test_storage();
        let now: u64 = 1_000_000;

        for key in &keys {
            let _ = storage.activity_log_insert(&ActivityLogRow {
                event_key: key.clone(),
                category: "test".into(),
                contact_id: None,
                payload: "{}".into(),
                created_at: now,
            });
        }

        let entries = storage.activity_log_query_recent(now, 86400).unwrap();
        let unique_keys: std::collections::HashSet<_> =
            entries.iter().map(|e| &e.event_key).collect();
        prop_assert_eq!(entries.len(), unique_keys.len());
    }
}

// @scenario: activity-log.feature - Prune never keeps entries older than retention window
proptest! {
    #[test]
    fn prune_never_keeps_old_entries(
        ages_days in prop::collection::vec(0u64..30, 1..20),
    ) {
        let storage = test_storage();
        let now: u64 = 30 * 86400; // anchor point: day 30

        for (i, age) in ages_days.iter().enumerate() {
            let _ = storage.activity_log_insert(&ActivityLogRow {
                event_key: format!("test:{i}"),
                category: "test".into(),
                contact_id: None,
                payload: "{}".into(),
                created_at: now - age * 86400,
            });
        }

        storage.activity_log_prune(now, 7 * 86400).unwrap();
        // Query ALL remaining entries (use very large window)
        let remaining = storage.activity_log_query_recent(now, now).unwrap();

        for entry in &remaining {
            let age_secs = now - entry.created_at;
            // Prune uses `DELETE WHERE created_at < cutoff` (strict), so entries
            // exactly at the boundary (age == max_age) are retained. The invariant
            // is therefore age_secs <= max_age, not age_secs < max_age.
            prop_assert!(
                age_secs <= 7 * 86400,
                "entry {} is {} days old, should have been pruned",
                entry.event_key,
                age_secs / 86400
            );
        }
    }
}

// @scenario: activity-log.feature - Notification count never exceeds new entry count
proptest! {
    #[test]
    fn notification_count_lte_new_entries(
        count in 0usize..20,
        pref_on in any::<bool>(),
    ) {
        let prefs = NotificationPreferences { contact_added_enabled: pref_on };
        let entries: Vec<(String, ActivityLogEntry)> = (0..count)
            .map(|i| {
                (format!("contact_added:c{i}"), ActivityLogEntry::ContactAdded {
                    contact_id: format!("c{i}"),
                    origin: EventOrigin::Synced,
                })
            })
            .collect();

        let notifications = NotificationEmitter::evaluate(
            &entries, &prefs, |_| "Test".into()
        );
        prop_assert!(notifications.len() <= entries.len());
    }
}
