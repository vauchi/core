// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Block is an owner-level decision: it reaches every linked device, so a
//! sibling stops exchanging with the blocked contact too (#295, owner
//! decision 2026-10-05; RG-15 "only blocking ends continuity").

use std::sync::{Arc, Mutex};

use vauchi_core::api::VauchiEvent;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::SyncItem;
use vauchi_core::{Identity, Vauchi};

use crate::common::device_sync::{journal_for_tablet, link_tablet};

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

// @scenario: release_privacy_multidevice_certification :: Only blocking ends long-lived contact continuity
#[test]
fn block_contact_is_journaled_for_linked_devices() {
    let (wb, bob) = vauchi_with_contact("Bob");
    let (registry, tablet_id) = link_tablet(&wb, [21u8; 32]);

    wb.block_contact(&bob).unwrap();

    let journal = journal_for_tablet(&wb, registry, &tablet_id);
    assert!(
        journal.iter().any(|item| matches!(
            item,
            SyncItem::ContactBlocked { contact_id, .. } if *contact_id == bob
        )),
        "block_contact must journal SyncItem::ContactBlocked, got {journal:?}"
    );
}

// @scenario: contacts_management :: Unblock a contact
#[test]
fn unblock_contact_is_journaled_for_linked_devices() {
    let (wb, bob) = vauchi_with_contact("Bob");
    wb.block_contact(&bob).unwrap();
    let (registry, tablet_id) = link_tablet(&wb, [22u8; 32]);

    wb.unblock_contact(&bob).unwrap();

    let journal = journal_for_tablet(&wb, registry, &tablet_id);
    assert!(
        journal.iter().any(|item| matches!(
            item,
            SyncItem::ContactUnblocked { contact_id, .. } if *contact_id == bob
        )),
        "unblock_contact must journal SyncItem::ContactUnblocked, got {journal:?}"
    );
}

// @scenario: release_privacy_multidevice_certification :: Only blocking ends long-lived contact continuity
#[test]
fn sync_items_for_block_round_trip_with_timestamp() {
    for item in [
        SyncItem::ContactBlocked {
            contact_id: "abc123".to_string(),
            timestamp: 1_700_000_000,
        },
        SyncItem::ContactUnblocked {
            contact_id: "abc123".to_string(),
            timestamp: 1_700_000_500,
        },
    ] {
        let back = SyncItem::from_json(&item.to_json()).unwrap();
        assert_eq!(back, item);
        assert_eq!(back.timestamp(), item.timestamp());
    }
}

// @scenario: release_privacy_multidevice_certification :: Only blocking ends long-lived contact continuity
#[test]
fn applying_synced_block_blocks_the_contact_and_dispatches() {
    let (wb, bob) = vauchi_with_contact("Bob");
    let events = capture_events(&wb);

    let applied = wb
        .apply_sync_items(vec![SyncItem::ContactBlocked {
            contact_id: bob.clone(),
            timestamp: 1_700_000_000,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    assert!(wb.get_contact(&bob).unwrap().unwrap().is_blocked());
    assert!(
        matches!(
            events.lock().unwrap().as_slice(),
            [VauchiEvent::ContactBlocked { contact_id }] if *contact_id == bob
        ),
        "a synced block must invalidate screens like a local one, got {:?}",
        events.lock().unwrap()
    );
}

// @scenario: contacts_management :: Unblock a contact
#[test]
fn applying_synced_unblock_unblocks_the_contact_and_dispatches() {
    let (wb, bob) = vauchi_with_contact("Bob");
    wb.block_contact(&bob).unwrap();
    let events = capture_events(&wb);

    let applied = wb
        .apply_sync_items(vec![SyncItem::ContactUnblocked {
            contact_id: bob.clone(),
            timestamp: 1_700_001_000,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    assert!(!wb.get_contact(&bob).unwrap().unwrap().is_blocked());
    assert!(
        matches!(
            events.lock().unwrap().as_slice(),
            [VauchiEvent::ContactUnblocked { contact_id }] if *contact_id == bob
        ),
        "a synced unblock must invalidate screens like a local one, got {:?}",
        events.lock().unwrap()
    );
}

// @scenario: release_privacy_multidevice_certification :: Only blocking ends long-lived contact continuity
#[test]
fn applying_synced_block_for_unknown_contact_is_skipped_not_fatal() {
    let (wb, bob) = vauchi_with_contact("Bob");

    let applied = wb
        .apply_sync_items(vec![SyncItem::ContactBlocked {
            contact_id: "nonexistent".to_string(),
            timestamp: 1_700_000_000,
        }])
        .unwrap();

    assert_eq!(applied, 1);
    assert!(!wb.get_contact(&bob).unwrap().unwrap().is_blocked());
}
