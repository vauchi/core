// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! P1 (settings-toggle-not-persisting): core `SettingsFlags` persistence
//! and self-seed of `VauchiConfig` on construction. The durable store is
//! encrypted core Storage (mirrors `BackupReminderState`), so every
//! `Vauchi` instance — mobile PAE engine, `open_vauchi()` transients, and
//! desktop — reads a consistent value, and the choice survives restart.

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;

// @internal
#[test]
fn settings_flags_persist_and_seed_config_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    // First instance: defaults, then flip suppress_presence + contact_added.
    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let vauchi = Vauchi::new(config).unwrap();

        let mut flags = vauchi.load_settings_flags().unwrap();
        assert!(flags.delivery_receipts_enabled, "default is true");
        assert!(!flags.suppress_presence, "default is false");
        assert!(!flags.contact_added_notifications, "default is false");

        flags.suppress_presence = true;
        flags.contact_added_notifications = true;
        vauchi.save_settings_flags(&flags).unwrap();
    }

    // Reopen the same DB: config must be self-seeded from persisted flags,
    // and the persisted values must round-trip.
    {
        let config = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
        let vauchi = Vauchi::new(config).unwrap();

        assert!(
            vauchi.config().suppress_presence,
            "suppress_presence seeded into config on reopen"
        );
        assert!(
            vauchi.config().contact_added_notifications,
            "contact_added_notifications seeded into config on reopen"
        );
        assert!(
            vauchi.config().delivery_receipts_enabled,
            "untouched delivery_receipts default (true) preserved"
        );

        let flags = vauchi.load_settings_flags().unwrap();
        assert!(
            flags.suppress_presence,
            "persisted suppress survives reopen"
        );
        assert!(
            flags.contact_added_notifications,
            "persisted contact_added survives reopen"
        );
    }
}

// M4 S1a (2026-07-03-placebo-accessibility-toggles): accessibility flags
// are Category-2 core-owned (ADR-047 Addendum 2026-07-05) — they persist
// in SettingsFlags and self-seed into config so a person's accommodations
// follow them across every device / restart.
// @internal
#[test]
fn accessibility_flags_persist_and_seed_config_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let vauchi = Vauchi::new(config).unwrap();

        let mut flags = vauchi.load_settings_flags().unwrap();
        assert!(!flags.reduce_motion, "default is false");
        assert!(!flags.large_touch, "default is false");

        flags.reduce_motion = true;
        flags.large_touch = true;
        vauchi.save_settings_flags(&flags).unwrap();
    }

    {
        let config = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
        let vauchi = Vauchi::new(config).unwrap();

        assert!(
            vauchi.config().reduce_motion,
            "reduce_motion seeded into config on reopen"
        );
        assert!(
            vauchi.config().large_touch,
            "large_touch seeded into config on reopen"
        );

        let flags = vauchi.load_settings_flags().unwrap();
        assert!(
            flags.reduce_motion,
            "persisted reduce_motion survives reopen"
        );
        assert!(flags.large_touch, "persisted large_touch survives reopen");
    }
}

// M4 S3a2 (2026-07-03-notifications-never-authorized): the card-update
// notification is default-ON (the product's core heartbeat), and turning
// it off persists + follows the user across devices/restart.
// @internal
#[test]
fn card_update_notification_defaults_on_and_toggle_off_persists() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let vauchi = Vauchi::new(config).unwrap();

        // Default-on: both the freshly-seeded config and the flags read true.
        assert!(
            vauchi.config().card_update_notifications,
            "card-update notifications default ON"
        );
        let mut flags = vauchi.load_settings_flags().unwrap();
        assert!(flags.card_update_notifications, "flags default ON");

        flags.card_update_notifications = false;
        vauchi.save_settings_flags(&flags).unwrap();
    }

    {
        let config = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
        let vauchi = Vauchi::new(config).unwrap();
        assert!(
            !vauchi.config().card_update_notifications,
            "the off choice seeded into config on reopen"
        );
        assert!(
            !vauchi
                .load_settings_flags()
                .unwrap()
                .card_update_notifications,
            "the off choice survives restart"
        );
    }
}
