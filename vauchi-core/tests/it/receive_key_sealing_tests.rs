// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Ratchet state is sealed under the Background Receive Key, in the same
//! file as the card it commits with, so a locked phone can advance it
//! (ADR-033 amendment 2026-11-07, vauchi/private#1093 slice 5). Rows
//! written before the key was set move under it when it is set; a rekey of
//! the storage key leaves them alone.
//!
//! Traces to: features/storage_key_upgrade.feature

use std::path::Path;
use std::sync::Arc;

use proptest::prelude::*;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{DoubleRatchetState, ShreddingMasterKey, SymmetricKey};
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::identity::Identity;
use vauchi_core::storage::{KeyAccess, MemoryKeyStorage, SecureStorage, Storage, StorageError};
use vauchi_core::{Vauchi, VauchiConfig};

const MESSAGE: &[u8] = b"an update from the contact";

/// A ratchet pair: the contact's sending side and our receiving side.
fn pair() -> (DoubleRatchetState, DoubleRatchetState) {
    let secret = SymmetricKey::generate();
    let ours = X3DHKeyPair::generate();
    let theirs = DoubleRatchetState::initialize_initiator(&secret, *ours.public_key()).unwrap();
    (
        theirs,
        DoubleRatchetState::initialize_responder(&secret, ours),
    )
}

/// The stored state is the one that was saved when it still decrypts what
/// the contact sends next.
fn opens_next_message(
    storage: &Storage,
    contact: &str,
    theirs: &mut DoubleRatchetState,
) -> Result<Vec<u8>, StorageError> {
    let message = theirs.encrypt(MESSAGE).unwrap();
    let (mut ours, _) = storage.ratchets().load_ratchet_state(contact)?.unwrap();
    Ok(ours.decrypt(&message).unwrap())
}

/// A contact row for the ratchet to belong to; returns its id.
fn contact_in(storage: &Storage) -> String {
    let peer = Identity::create("Bob", 0);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    storage.contacts().save_contact(&contact).unwrap();
    contact.id().to_string()
}

/// Saves our side of the ratchet with a new contact; returns its id.
fn save(storage: &Storage, ours: &DoubleRatchetState) -> String {
    let contact = contact_in(storage);
    storage
        .ratchets()
        .save_ratchet_state(&contact, ours, false)
        .unwrap();
    contact
}

fn db(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("vauchi.db")
}

// @internal
#[test]
fn a_ratchet_saved_under_the_receive_key_needs_that_key_to_load() {
    let dir = tempfile::tempdir().unwrap();
    let sek = SymmetricKey::generate();
    let brk = SymmetricKey::generate();
    let (mut theirs, ours) = pair();
    let contact = {
        let mut storage = Storage::open(db(&dir), sek.clone()).unwrap();
        storage.set_receive_key(brk.clone()).unwrap();
        save(&storage, &ours)
    };

    let mut storage = Storage::open(db(&dir), sek).unwrap();
    let without = storage.ratchets().load_ratchet_state(&contact);
    storage.set_receive_key(brk).unwrap();

    assert!(
        matches!(without, Err(StorageError::ReceiveKeyUnavailable)),
        "the storage key alone must not open receive state, got {:?}",
        without.map(|state| state.is_some())
    );
    assert_eq!(
        opens_next_message(&storage, &contact, &mut theirs).unwrap(),
        MESSAGE
    );
}

// @internal
#[test]
fn ratchets_saved_before_the_receive_key_move_under_it_when_it_is_set() {
    let dir = tempfile::tempdir().unwrap();
    let sek = SymmetricKey::generate();
    let brk = SymmetricKey::generate();
    let (mut theirs, ours) = pair();
    let contact = {
        let mut storage = Storage::open(db(&dir), sek.clone()).unwrap();
        let contact = save(&storage, &ours);
        storage.set_receive_key(brk.clone()).unwrap();
        contact
    };

    let mut storage = Storage::open(db(&dir), sek).unwrap();
    let without = storage.ratchets().load_ratchet_state(&contact);
    storage.set_receive_key(brk).unwrap();

    assert!(matches!(without, Err(StorageError::ReceiveKeyUnavailable)));
    assert_eq!(
        opens_next_message(&storage, &contact, &mut theirs).unwrap(),
        MESSAGE
    );
}

// @internal
#[test]
fn without_a_receive_key_ratchets_stay_under_the_storage_key() {
    let dir = tempfile::tempdir().unwrap();
    let sek = SymmetricKey::generate();
    let (mut theirs, ours) = pair();
    let contact = save(&Storage::open(db(&dir), sek.clone()).unwrap(), &ours);

    let storage = Storage::open(db(&dir), sek).unwrap();

    assert_eq!(
        opens_next_message(&storage, &contact, &mut theirs).unwrap(),
        MESSAGE
    );
}

// @internal
#[test]
fn a_different_receive_key_is_refused_and_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let sek = SymmetricKey::generate();
    let (_, ours) = pair();
    let contact = {
        let mut storage = Storage::open(db(&dir), sek.clone()).unwrap();
        storage.set_receive_key(SymmetricKey::generate()).unwrap();
        save(&storage, &ours)
    };

    let mut storage = Storage::open(db(&dir), sek).unwrap();
    let refused = storage.set_receive_key(SymmetricKey::generate());

    assert!(
        matches!(refused, Err(StorageError::WrongKey)),
        "got {refused:?}"
    );
    assert!(matches!(
        storage.ratchets().load_ratchet_state(&contact),
        Err(StorageError::ReceiveKeyUnavailable)
    ));
}

// @internal
#[test]
fn a_rekey_of_the_storage_key_leaves_receive_state_readable() {
    let dir = tempfile::tempdir().unwrap();
    let brk = SymmetricKey::generate();
    let (mut theirs, ours) = pair();
    let mut storage = Storage::open(db(&dir), SymmetricKey::generate()).unwrap();
    storage.set_receive_key(brk).unwrap();
    let contact = save(&storage, &ours);

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        opens_next_message(&storage, &contact, &mut theirs).unwrap(),
        MESSAGE
    );
}

/// The keychain an identity's open fills: SMK in the default class, BRK in
/// the after-first-unlock class.
fn keychain_after_create(dir: &Path) -> Arc<MemoryKeyStorage> {
    let keychain = Arc::new(MemoryKeyStorage::new());
    Vauchi::with_secure_storage(
        VauchiConfig::with_storage_path(dir.join("vauchi.db")),
        keychain.clone(),
    )
    .unwrap()
    .create_identity("Alice")
    .unwrap();
    keychain
}

fn sek_of(keychain: &MemoryKeyStorage) -> SymmetricKey {
    let smk: [u8; 32] = keychain
        .load_key("smk")
        .unwrap()
        .unwrap()
        .try_into()
        .unwrap();
    ShreddingMasterKey::from_bytes(smk).derive_sek()
}

// @internal
#[test]
fn an_opened_identity_seals_ratchets_under_its_receive_key() {
    let dir = tempfile::tempdir().unwrap();
    let keychain = keychain_after_create(dir.path());
    let (mut theirs, ours) = pair();
    let contact = {
        let vauchi = Vauchi::with_secure_storage(
            VauchiConfig::with_storage_path(dir.path().join("vauchi.db")),
            keychain.clone(),
        )
        .unwrap();
        save(vauchi.storage(), &ours)
    };
    let brk = keychain
        .load_key_with_access("brk", KeyAccess::AfterFirstUnlock)
        .unwrap()
        .unwrap();

    let mut storage = Storage::open(dir.path().join("vauchi.db"), sek_of(&keychain)).unwrap();
    let without = storage.ratchets().load_ratchet_state(&contact);
    storage
        .set_receive_key(SymmetricKey::from_bytes(brk.try_into().unwrap()))
        .unwrap();

    assert!(matches!(without, Err(StorageError::ReceiveKeyUnavailable)));
    assert_eq!(
        opens_next_message(&storage, &contact, &mut theirs).unwrap(),
        MESSAGE
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // @internal
    #[test]
    fn any_device_roundtrips_under_the_receive_key(
        device in proptest::array::uniform32(any::<u8>()),
    ) {
        let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
        storage.set_receive_key(SymmetricKey::generate()).unwrap();
        let contact = contact_in(&storage);
        let (mut theirs, ours) = pair();
        storage
            .ratchets()
            .save_ratchet_state_for_device(&contact, &device, &ours, true)
            .unwrap();

        let message = theirs.encrypt(MESSAGE).unwrap();
        let (mut loaded, initiator) = storage
            .ratchets()
            .load_ratchet_state_for_device(&contact, &device)
            .unwrap()
            .unwrap();

        prop_assert!(initiator);
        prop_assert_eq!(loaded.decrypt(&message).unwrap(), MESSAGE.to_vec());
    }
}
