// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Nothing on the device ages out on its own: years later, after the
//! housekeeping a launch runs, everything is still there.

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use vauchi_core::clock::FakeClock;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey, Vauchi};

const TEN_YEARS: Duration = Duration::from_secs(10 * 365 * 24 * 60 * 60);

// @scenario: privacy_compliance :: Local data persists until deleted
#[test]
fn ten_years_of_launch_housekeeping_keep_my_card_contacts_groups_and_notes() {
    let clock = Arc::new(FakeClock::new(
        UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut vauchi = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 000 00 00",
            0,
        ))
        .unwrap();
    let peer = Identity::create("Bob", 0);
    let bob = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        1_700_000_000,
    );
    let bob_id = bob.id().to_string();
    vauchi.add_contact(bob).unwrap();
    let family = vauchi.create_group("Family").unwrap();
    vauchi.add_contact_to_group(family.id(), &bob_id).unwrap();
    vauchi.add_personal_note(&bob_id, "Met at FOSDEM").unwrap();

    clock.advance(TEN_YEARS);
    vauchi.cleanup_stale_soft_deletes().unwrap();
    vauchi.expire_pending_reciprocity().unwrap();

    assert_eq!(vauchi.own_card().unwrap().unwrap().fields().len(), 1);
    assert!(vauchi.get_contact(&bob_id).unwrap().is_some());
    assert_eq!(vauchi.get_group_members(family.id()).unwrap().len(), 1);
    assert_eq!(
        vauchi.read_personal_note(&bob_id).unwrap().as_deref(),
        Some("Met at FOSDEM")
    );
}
