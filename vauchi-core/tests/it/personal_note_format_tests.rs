// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! One personal-note format (private#452): plain text, encrypted at rest by
//! the storage key. The app wrote that format, while add_personal_note, the
//! linked-device apply and read_personal_note used an extra layer under the
//! contact's shared key, so a note crossing between the two read as empty or
//! failed to decrypt. Notes written under the shared key stay readable.

use vauchi_core::crypto::encrypt;
use vauchi_core::sync::SyncItem;
use vauchi_core::{Contact, ContactCard, SearchFacets, SymmetricKey, Vauchi};

fn alice_with_bob() -> (Vauchi, String, SymmetricKey) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let shared_key = SymmetricKey::generate();
    let bob = Contact::from_exchange([1; 32], ContactCard::new("Bob"), shared_key.clone(), 0);
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();
    (wb, bob_id, shared_key)
}

/// What the app's contact screen shows: the stored bytes as text.
fn shown_by_the_app(wb: &Vauchi, contact_id: &str) -> Option<String> {
    wb.load_personal_notes(contact_id)
        .unwrap()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

// @scenario: contacts :: a personal note reads the same on every path
// @internal
#[test]
fn a_note_added_through_the_api_is_what_the_app_shows() {
    let (wb, bob_id, _) = alice_with_bob();

    wb.add_personal_note(&bob_id, "met at the conference")
        .unwrap();

    assert_eq!(
        shown_by_the_app(&wb, &bob_id).as_deref(),
        Some("met at the conference")
    );
}

// @scenario: contacts :: a personal note from a linked device shows in the app
// @internal
#[test]
fn a_note_from_a_linked_device_is_what_the_app_shows() {
    let (wb, bob_id, _) = alice_with_bob();

    wb.apply_sync_items(vec![SyncItem::PersonalNoteChanged {
        contact_id: bob_id.clone(),
        note: "plays the cello".into(),
        timestamp: 1,
    }])
    .unwrap();

    assert_eq!(
        shown_by_the_app(&wb, &bob_id).as_deref(),
        Some("plays the cello")
    );
}

// @internal
#[test]
fn a_note_saved_by_the_app_reads_and_is_searchable() {
    let (wb, bob_id, _) = alice_with_bob();
    wb.save_personal_notes(&bob_id, "allergic to cats".as_bytes())
        .unwrap();

    assert_eq!(
        wb.read_personal_note(&bob_id).unwrap().as_deref(),
        Some("allergic to cats")
    );
    let facets = SearchFacets {
        comment: true,
        ..Default::default()
    };
    let hits = wb.search_contacts_faceted("cats", &facets).unwrap();
    assert_eq!(hits.len(), 1, "the note is searchable");
}

// Notes written by add_personal_note before private#452 carry the shared-key
// layer; they must still read.
// @internal
#[test]
fn a_note_written_under_the_shared_key_still_reads() {
    let (wb, bob_id, shared_key) = alice_with_bob();
    let legacy = encrypt(&shared_key, "old note".as_bytes()).unwrap();
    wb.save_personal_notes(&bob_id, &legacy).unwrap();

    assert_eq!(
        wb.read_personal_note(&bob_id).unwrap().as_deref(),
        Some("old note")
    );
}
