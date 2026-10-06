// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Retry backoff, reciprocity expiry, contact-override loading and
//! migration-backup cleanup, each pinned to exact values
//! (vauchi/private#522).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::api::sync::SyncManager;
use vauchi_core::clock::FakeClock;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::Reciprocity;
use vauchi_core::storage::UpdateStatus;
use vauchi_core::{Contact, ContactCard, ContactField, FieldType, Storage, Vauchi};

const T: u64 = 1_700_000_000;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn retry_at(manager: &SyncManager<'_>, update_id: &str) -> u64 {
    match manager
        .get_pending("contact-1")
        .unwrap()
        .into_iter()
        .find(|u| u.id == update_id)
        .unwrap()
        .status
    {
        UpdateStatus::Failed { retry_at, .. } => retry_at,
        other => panic!("expected Failed, got {other:?}"),
    }
}

// @internal
#[test]
fn a_failed_update_backs_off_exponentially_up_to_five_minutes() {
    let storage = Storage::in_memory(SymmetricKey::generate())
        .unwrap()
        .with_clock(Arc::new(FakeClock::new(at(T))));
    let mut manager = SyncManager::new(&storage);
    let rng = vauchi_core::rng::OsSecureRng::new();
    let old_card = ContactCard::new("Alice");
    let mut new_card = ContactCard::new("Alice");
    let _ = new_card.add_field(ContactField::new(FieldType::Email, "email", "a@x.ch", 0));

    for (retry_count, delay) in [(0, 2), (3, 16), (10, 300)] {
        let id = manager
            .queue_card_update(&rng, "contact-1", &old_card, &new_card)
            .unwrap();
        manager.mark_failed(&id, "refused", retry_count).unwrap();
        assert_eq!(retry_at(&manager, &id), T + delay, "retry {retry_count}");
    }
}

fn pending_contact(seed: u8, exchanged_at: u64) -> Contact {
    let mut contact = Contact::from_exchange(
        [seed; 32],
        ContactCard::new(&format!("C{seed}")),
        SymmetricKey::generate(),
        exchanged_at,
    );
    contact.set_reciprocity(Reciprocity::Pending);
    contact
}

// @internal
#[test]
fn only_exchanges_older_than_seven_days_stop_waiting_for_reciprocity() {
    let wb = Vauchi::in_memory_with_clock(FakeClock::new(at(T)).shared()).unwrap();
    let day = 86_400;
    for (seed, age) in [(1u8, day), (2, 7 * day), (3, 8 * day)] {
        wb.storage()
            .contacts()
            .save_contact(&pending_contact(seed, T - age))
            .unwrap();
    }

    assert_eq!(wb.expire_pending_reciprocity().unwrap(), 1);
    let reciprocity = |name: &str| {
        wb.storage()
            .contacts()
            .list_contacts()
            .unwrap()
            .into_iter()
            .find(|c| c.display_name() == name)
            .unwrap()
            .reciprocity(0)
    };
    assert_eq!(reciprocity("C1"), Reciprocity::Pending);
    assert_eq!(
        reciprocity("C2"),
        Reciprocity::Pending,
        "exactly seven days still waits"
    );
    assert_eq!(reciprocity("C3"), Reciprocity::Unreciprocated);
}

// @internal
#[test]
fn all_contact_overrides_load_exactly_as_saved() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    assert!(
        storage
            .labels()
            .load_all_contact_overrides()
            .unwrap()
            .is_empty()
    );

    storage
        .labels()
        .save_contact_override("bob", "email", false)
        .unwrap();
    storage
        .labels()
        .save_contact_override("bob", "phone", true)
        .unwrap();

    let all = storage.labels().load_all_contact_overrides().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all["bob"].get("email"), Some(&false));
    assert_eq!(all["bob"].get("phone"), Some(&true));
}

// @internal
#[test]
fn opening_storage_removes_leftover_migration_backups() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("vauchi.db");
    let backup = dir.path().join("vauchi.db.pre-migration-v3.bak");
    let unrelated = dir.path().join("notes.bak");
    std::fs::write(&backup, b"old").unwrap();
    std::fs::write(&unrelated, b"keep").unwrap();

    let _storage = Storage::open(&db, SymmetricKey::generate()).unwrap();

    assert!(!backup.exists());
    assert!(unrelated.exists());
}
