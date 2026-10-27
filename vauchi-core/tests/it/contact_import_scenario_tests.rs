// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Imported contacts at their trust, limit, format, duplicate and
//! linked-device boundaries.

use crate::common;
use common::device_sync::{create_test_device, create_test_registry, create_test_storage};
use vauchi_core::api::{DeviceSyncOrchestrator, ImportWarning};
use vauchi_core::contact::{ContactError, ImportSource};
use vauchi_core::contact_card::FieldType;
use vauchi_core::sync::DeviceLinkIntent;
use vauchi_core::{Contact, ContactCard, Vauchi, VauchiError};

fn imported(name: &str, card: ContactCard) -> Contact {
    Contact::from_import(
        format!("contact-{name}"),
        card,
        ImportSource::VcardFile,
        None,
        0,
    )
}

// @scenario: contact_import :: Imported contacts cannot be trusted for recovery
#[test]
fn trusting_an_imported_contact_for_recovery_is_refused_as_needing_an_exchange() {
    let vauchi = Vauchi::in_memory().unwrap();
    let bob = imported("Bob", ContactCard::new("Bob"));
    let bob_id = bob.id().to_string();
    vauchi.add_contact(bob).unwrap();

    let refused = vauchi.toggle_recovery_trust(&bob_id);

    let Err(VauchiError::InvalidState(reason)) = refused else {
        panic!("expected a refusal, got {refused:?}");
    };
    assert!(reason.contains("requires an exchanged contact"), "{reason}");
    let bob = vauchi.get_contact(&bob_id).unwrap().unwrap();
    assert!(!bob.is_recovery_trusted());
}

// @scenario: contact_import :: Imported contacts cannot be trusted for proposals
#[test]
fn trusting_an_imported_contact_for_proposals_is_refused() {
    let mut bob = imported("Bob", ContactCard::new("Bob"));

    let refused = bob.set_proposal_trusted(true);

    assert!(matches!(
        refused,
        Err(ContactError::OperationRequiresExchangedContact)
    ));
    assert!(!bob.is_proposal_trusted());
}

// @scenario: contact_import :: Import respects contact limit
#[test]
fn at_the_contact_limit_an_import_of_five_adds_none_and_says_why() {
    let vauchi = Vauchi::in_memory().unwrap();
    vauchi
        .add_contact(imported("Ann", ContactCard::new("Ann")))
        .unwrap();
    vauchi.storage().contacts().set_contact_limit(1).unwrap();
    let five: String = ["Bea", "Cy", "Di", "Ed", "Flo"]
        .iter()
        .map(|n| format!("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:{n}\r\nEND:VCARD\r\n"))
        .collect();

    let result = vauchi.import_contacts_from_vcf(five.as_bytes()).unwrap();

    assert_eq!((result.imported, result.skipped), (0, 5));
    assert!(
        result
            .warnings
            .iter()
            .any(|w| matches!(w, ImportWarning::ContactLimitReached { .. })),
        "{:?}",
        result.warnings
    );
    assert_eq!(vauchi.list_contacts().unwrap().len(), 1);
}

/// The shape Google Contacts exports: vCard 3.0, typed and grouped
/// properties, and a structured ADR.
const GOOGLE_VCARD: &str = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:Grace Hopper\r\n\
N:Hopper;Grace;;;\r\n\
EMAIL;TYPE=INTERNET;TYPE=HOME:grace@example.com\r\n\
TEL;TYPE=CELL:+1 555 010 0100\r\n\
item1.ADR;TYPE=HOME:;;1 Navy Way;Arlington;VA;22201;USA\r\n\
item1.X-ABLabel:\r\n\
END:VCARD\r\n";

// @scenario: contact_import :: Import vCard 3.0 from Google Contacts
#[test]
fn a_google_contacts_vcard_arrives_with_phone_email_and_address() {
    let vauchi = Vauchi::in_memory().unwrap();

    let result = vauchi
        .import_contacts_from_vcf(GOOGLE_VCARD.as_bytes())
        .unwrap();

    assert_eq!(result.imported, 1);
    let grace = &vauchi.list_contacts().unwrap()[0];
    assert_eq!(grace.display_name(), "Grace Hopper");
    let mut kinds: Vec<FieldType> = grace
        .card()
        .fields()
        .iter()
        .map(|f| f.field_type())
        .collect();
    kinds.sort_by_key(|k| format!("{k:?}"));
    assert_eq!(
        kinds,
        [FieldType::Address, FieldType::Email, FieldType::Phone]
    );
}

// @scenario: contact_import :: Imported contacts sync to linked device
#[test]
fn a_linked_device_receives_an_imported_contact_with_its_flags() {
    let master_seed = [0x42u8; 32];
    let (storage_a, storage_b) = (create_test_storage(), create_test_storage());
    let mut bob = imported("Bob", ContactCard::new("Bob"));
    bob.set_hidden(true);
    bob.set_blocked(true);
    bob.set_favorite(true);
    storage_a.contacts().save_contact(&bob).unwrap();
    let device_a = create_test_device(&master_seed, 0, "Device A");
    let registry_a = create_test_registry(&master_seed, &device_a);
    let device_b = create_test_device(&master_seed, 1, "Device B");
    let registry_b = create_test_registry(&master_seed, &device_b);
    let payload = DeviceSyncOrchestrator::new(&storage_a, device_a, registry_a)
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();

    DeviceSyncOrchestrator::new(&storage_b, device_b, registry_b)
        .apply_full_sync(payload)
        .unwrap();

    let synced = storage_b
        .contacts()
        .load_contact(bob.id())
        .unwrap()
        .expect("Bob reached device B");
    assert_eq!(
        (
            synced.display_name().to_string(),
            synced.is_imported(),
            synced.is_hidden(),
            synced.is_blocked(),
            synced.is_favorite()
        ),
        ("Bob".to_string(), true, true, true, true)
    );
}
