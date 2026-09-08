// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ignore silences notifications (ADR-072): a card update from an
//! ignored contact is still applied and logged, but never surfaces as
//! an OS notification.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::notification_types::NotificationCategory;
use vauchi_app::ui::AppEngine;
use vauchi_core::Identity;
use vauchi_core::api::{Vauchi, VauchiEvent};
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn vauchi_with_contact(name: &str) -> (Vauchi, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Owner").unwrap();
    let id = add_exchanged_contact(&vauchi, name);
    (vauchi, id)
}

fn add_exchanged_contact(vauchi: &Vauchi, name: &str) -> String {
    let identity = Identity::create(name, 0);
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    id
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn card_update_from_an_ignored_contact_drains_to_no_notification() {
    let (vauchi, bob) = vauchi_with_contact("Bob");
    vauchi.ignore_contact(&bob).unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::IncomingUpdate {
            contact_id: bob.clone(),
        });

    let notifications = engine.drain_pending_notifications();
    assert!(
        notifications.is_empty(),
        "an ignored contact must not notify, got {notifications:?}"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn card_update_from_an_active_contact_still_notifies() {
    let (vauchi, bob) = vauchi_with_contact("Bob");
    let mut engine = AppEngine::new(vauchi);

    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::IncomingUpdate {
            contact_id: bob.clone(),
        });

    let notifications = engine.drain_pending_notifications();
    assert_eq!(notifications.len(), 1, "control: active contact notifies");
    assert_eq!(notifications[0].category, NotificationCategory::CardUpdate);
    assert_eq!(notifications[0].contact_id, bob);
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn un_ignoring_restores_notifications_without_replaying_missed_ones() {
    // Activity-log keys carry the second of receipt; a fake clock lets the
    // second update land on a distinct key without waiting (CC-06).
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut vauchi = Vauchi::in_memory_with_clock(clock.clone() as Arc<dyn Clock>).unwrap();
    vauchi.create_identity("Owner").unwrap();
    let bob = add_exchanged_contact(&vauchi, "Bob");
    vauchi.ignore_contact(&bob).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::IncomingUpdate {
            contact_id: bob.clone(),
        });
    assert!(engine.drain_pending_notifications().is_empty());

    engine.vauchi().unignore_contact(&bob).unwrap();
    assert!(
        engine.drain_pending_notifications().is_empty(),
        "unignore must not replay the update that arrived while ignored"
    );

    clock.advance(Duration::from_secs(1));
    engine
        .vauchi()
        .events()
        .dispatch(VauchiEvent::IncomingUpdate {
            contact_id: bob.clone(),
        });
    assert_eq!(
        engine.drain_pending_notifications().len(),
        1,
        "after unignore the next update notifies again"
    );
}
