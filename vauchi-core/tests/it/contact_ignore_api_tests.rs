// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Vauchi::ignore_contact` / `unignore_contact` (ADR-072): the API half
//! of ignore, its events, and its linked-device sync item.

use std::sync::{Arc, Mutex};

use vauchi_core::api::VauchiEvent;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::device_sync::SyncItem;
use vauchi_core::{Identity, Vauchi, VauchiError};

fn vauchi_with_contact(name: &str) -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let identity = Identity::create(name, 0);
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    (wb, id)
}

fn capture_events(wb: &Vauchi) -> Arc<Mutex<Vec<VauchiEvent>>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let _ = wb.add_event_handler(Arc::new(move |event| sink.lock().unwrap().push(event)));
    events
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignore_contact_persists_flag_and_dispatches_contact_ignored() {
    let (wb, bob) = vauchi_with_contact("Bob");
    let events = capture_events(&wb);

    wb.ignore_contact(&bob).unwrap();

    let loaded = wb.get_contact(&bob).unwrap().unwrap();
    assert!(loaded.is_ignored());
    assert!(loaded.ignored_at().is_some(), "ignore must stamp a time");
    assert!(
        matches!(
            events.lock().unwrap().as_slice(),
            [VauchiEvent::ContactIgnored { contact_id }] if *contact_id == bob
        ),
        "expected exactly one ContactIgnored, got {:?}",
        events.lock().unwrap()
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn unignore_contact_clears_flag_and_dispatches_contact_unignored() {
    let (wb, bob) = vauchi_with_contact("Bob");
    wb.ignore_contact(&bob).unwrap();
    let events = capture_events(&wb);

    wb.unignore_contact(&bob).unwrap();

    let loaded = wb.get_contact(&bob).unwrap().unwrap();
    assert!(!loaded.is_ignored());
    assert_eq!(loaded.ignored_at(), None);
    assert!(
        matches!(
            events.lock().unwrap().as_slice(),
            [VauchiEvent::ContactUnignored { contact_id }] if *contact_id == bob
        ),
        "expected exactly one ContactUnignored, got {:?}",
        events.lock().unwrap()
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignore_unknown_contact_is_not_found() {
    let (wb, _) = vauchi_with_contact("Bob");
    let err = wb.ignore_contact("nonexistent").unwrap_err();
    assert!(matches!(err, VauchiError::NotFound(_)), "got {err:?}");
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn unignore_of_never_ignored_contact_is_idempotent() {
    let (wb, bob) = vauchi_with_contact("Bob");
    wb.unignore_contact(&bob).unwrap();
    assert!(!wb.get_contact(&bob).unwrap().unwrap().is_ignored());
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignore_keeps_the_contact_in_the_active_list() {
    let (wb, bob) = vauchi_with_contact("Bob");
    wb.ignore_contact(&bob).unwrap();
    let ids: Vec<String> = wb
        .list_contacts()
        .unwrap()
        .iter()
        .map(|c| c.id().to_string())
        .collect();
    assert_eq!(ids, vec![bob], "ignore must not hide the contact");
}

// ── Linked-device sync item ──────────────────────────────────────────

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn sync_item_contact_ignored_round_trips_with_timestamp() {
    let item = SyncItem::ContactIgnored {
        contact_id: "abc123".to_string(),
        timestamp: 1_700_000_000,
    };
    let json = item.to_json();
    let back = SyncItem::from_json(&json).unwrap();
    assert_eq!(back, item);
    assert_eq!(back.timestamp(), 1_700_000_000);
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn sync_item_contact_unignored_round_trips_with_timestamp() {
    let item = SyncItem::ContactUnignored {
        contact_id: "abc123".to_string(),
        timestamp: 1_700_000_500,
    };
    let json = item.to_json();
    let back = SyncItem::from_json(&json).unwrap();
    assert_eq!(back, item);
    assert_eq!(back.timestamp(), 1_700_000_500);
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn applying_synced_ignore_sets_flag_with_sibling_timestamp_and_dispatches() {
    let (wb, bob) = vauchi_with_contact("Bob");
    let events = capture_events(&wb);

    let applied = wb
        .apply_sync_items(vec![SyncItem::ContactIgnored {
            contact_id: bob.clone(),
            timestamp: 1_700_000_000,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    let loaded = wb.get_contact(&bob).unwrap().unwrap();
    assert_eq!(loaded.ignored_at(), Some(1_700_000_000));
    assert!(
        matches!(
            events.lock().unwrap().as_slice(),
            [VauchiEvent::ContactIgnored { contact_id }] if *contact_id == bob
        ),
        "a synced ignore must invalidate screens like a local one"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn applying_synced_unignore_clears_flag_and_dispatches() {
    let (wb, bob) = vauchi_with_contact("Bob");
    wb.ignore_contact(&bob).unwrap();
    let events = capture_events(&wb);

    let applied = wb
        .apply_sync_items(vec![SyncItem::ContactUnignored {
            contact_id: bob.clone(),
            timestamp: 1_700_001_000,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    assert!(!wb.get_contact(&bob).unwrap().unwrap().is_ignored());
    assert!(
        matches!(
            events.lock().unwrap().as_slice(),
            [VauchiEvent::ContactUnignored { contact_id }] if *contact_id == bob
        ),
        "a synced unignore must invalidate screens like a local one"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn applying_synced_ignore_for_unknown_contact_is_skipped_not_fatal() {
    let (wb, _) = vauchi_with_contact("Bob");
    let applied = wb
        .apply_sync_items(vec![SyncItem::ContactIgnored {
            contact_id: "nonexistent".to_string(),
            timestamp: 1_700_000_000,
        }])
        .unwrap();
    assert_eq!(applied, 1, "skip is non-fatal, matching archive");
}
