// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Vauchi::add_personal_note() and read_personal_note()
//!
//! Verifies that note encryption/decryption is handled entirely
//! within core (ADR-021: no crypto in frontends).

use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

/// Helper: create Vauchi with identity and an exchanged contact.
fn setup_with_contact() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let mut pk = [0u8; 32];
    pk[0] = 1;
    let card = ContactCard::new("Bob");
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate(), 0);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    (wb, contact_id)
}

// @scenario: navigation.feature - Add and read personal note roundtrip
#[test]
fn test_add_and_read_note_roundtrip() {
    let (wb, contact_id) = setup_with_contact();

    wb.add_personal_note(&contact_id, "Met at conference 2026")
        .unwrap();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert_eq!(
        note.as_deref(),
        Some("Met at conference 2026"),
        "Decrypted note must match original plaintext"
    );
}

// @scenario: navigation.feature - Read note for contact without note
#[test]
fn test_read_note_returns_none_when_empty() {
    let (wb, contact_id) = setup_with_contact();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert!(note.is_none(), "No note should return None");
}

// @scenario: navigation.feature - Add note overwrites previous
#[test]
fn test_add_note_overwrites_previous() {
    let (wb, contact_id) = setup_with_contact();

    wb.add_personal_note(&contact_id, "First note").unwrap();
    wb.add_personal_note(&contact_id, "Second note").unwrap();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert_eq!(
        note.as_deref(),
        Some("Second note"),
        "Second add must overwrite first"
    );
}

// @scenario: navigation.feature - Note for nonexistent contact fails
#[test]
fn test_add_note_for_missing_contact_fails() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let result = wb.add_personal_note("nonexistent-id", "hello");
    assert!(
        result.is_err(),
        "Adding note to nonexistent contact must fail"
    );
}

// @scenario: navigation.feature - Delete note then read returns None
#[test]
fn test_delete_note_clears() {
    let (wb, contact_id) = setup_with_contact();

    wb.add_personal_note(&contact_id, "To be deleted").unwrap();
    wb.delete_personal_notes(&contact_id).unwrap();

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert!(note.is_none(), "Deleted note must return None");
}

// @scenario: sync_updates :: Personal notes converge across linked devices
#[test]
fn note_mutations_journal_for_linked_devices() {
    let (wb, contact_id) = setup_with_contact();
    let (registry, tablet_id) = install_linked_registry(&wb);

    wb.add_personal_note(&contact_id, "met at conference")
        .unwrap();

    let orchestrator = vauchi_core::api::sync::DeviceSyncOrchestrator::load(
        wb.storage(),
        wb.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap();
    let pending = orchestrator.pending_for_device(&tablet_id);
    assert!(
        pending.iter().any(|item| matches!(
            item,
            vauchi_core::sync::SyncItem::PersonalNoteChanged {
                contact_id: id,
                note,
                ..
            } if id == &contact_id && note == "met at conference"
        )),
        "add_personal_note must journal PersonalNoteChanged for linked devices, \
         got {pending:?}"
    );
}

/// A personal-note deletion must reach every linked device; otherwise an
/// offline sibling can retain owner-private data after the owner removed it.
// @scenario: sync_updates :: Personal note deletion converges across linked devices
#[test]
fn deleted_note_journals_tombstone_for_linked_devices() {
    let (wb, contact_id) = setup_with_contact();
    let (registry, tablet_id) = install_linked_registry(&wb);

    wb.add_personal_note(&contact_id, "met at conference")
        .unwrap();
    wb.delete_personal_notes(&contact_id).unwrap();

    let orchestrator = vauchi_core::api::sync::DeviceSyncOrchestrator::load(
        wb.storage(),
        wb.identity().unwrap().create_device_info(0),
        registry,
    )
    .unwrap();
    let pending = orchestrator.pending_for_device(&tablet_id);
    assert!(
        pending.iter().any(|item| matches!(
            item,
            vauchi_core::sync::SyncItem::PersonalNoteRemoved {
                contact_id: id,
                ..
            } if id == &contact_id
        )),
        "delete_personal_notes must journal PersonalNoteRemoved for linked devices, \
         got {pending:?}"
    );
}

// @scenario: sync_updates :: Personal notes converge across linked devices
#[test]
fn applied_synced_note_is_readable() {
    let (wb, contact_id) = setup_with_contact();

    let applied = wb
        .apply_sync_items(vec![vauchi_core::sync::SyncItem::PersonalNoteChanged {
            contact_id: contact_id.clone(),
            note: "synced from my tablet".to_string(),
            timestamp: 1000,
        }])
        .unwrap();
    assert_eq!(applied, 1);

    let note = wb.read_personal_note(&contact_id).unwrap();
    assert_eq!(
        note.as_deref(),
        Some("synced from my tablet"),
        "a note received via device sync must decrypt through read_personal_note"
    );
}

// @scenario: sync_updates :: Personal note deletion converges across linked devices
#[test]
fn applied_synced_note_tombstone_clears_local_note() {
    let (wb, contact_id) = setup_with_contact();
    wb.add_personal_note(&contact_id, "remove me from every device")
        .unwrap();

    let applied = wb
        .apply_sync_items(vec![vauchi_core::sync::SyncItem::PersonalNoteRemoved {
            contact_id: contact_id.clone(),
            timestamp: 2000,
        }])
        .unwrap();
    assert_eq!(applied, 1);
    assert!(
        wb.read_personal_note(&contact_id).unwrap().is_none(),
        "a synced personal-note tombstone must erase the local encrypted note"
    );
}

/// Copies the helper from api_tags_tests.rs: registers a second device so
/// record_sync_item has a peer to journal for.
fn install_linked_registry(wb: &Vauchi) -> (vauchi_core::identity::DeviceRegistry, [u8; 32]) {
    use vauchi_core::crypto::SigningKeyPair;
    use vauchi_core::identity::{DeviceInfo, DeviceRegistry};

    const SEED: [u8; 32] = [9u8; 32];
    let signing = SigningKeyPair::from_seed(&SEED);
    let mut registry = DeviceRegistry::new(
        DeviceInfo::derive(&SEED, 0, "phone".into(), 0).to_registered(&SEED),
        &signing,
    );
    let tablet = DeviceInfo::derive(&SEED, 1, "tablet".into(), 0);
    let tablet_id = *tablet.device_id();
    registry
        .add_device_unsigned(tablet.to_registered(&SEED))
        .unwrap();
    wb.storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    (registry, tablet_id)
}
