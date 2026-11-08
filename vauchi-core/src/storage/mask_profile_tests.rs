// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A seal in the legacy decoy-table shape converts into the mask profile's
//! regular tables when it loads, and is sealed again in the new shape
//! (ADR-080 point 4, amended 2026-11-08).

use super::Storage;
use super::duress_content::{LEGACY_TABLES, decode_tables, mask_key, open_duress_db, open_seal};
use super::mask::SealedSlot;
use crate::card::ContactCard;
use crate::crypto::{SymmetricKey, encrypt};

/// A database whose seal is in the legacy shape, its fields under the mask
/// key, as every open between ADR-080 and 2026-11-08 left it.
fn with_a_legacy_seal() -> (Storage, SymmetricKey) {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key.clone()).unwrap();
    let mask = mask_key(&key);
    let legacy = open_duress_db().unwrap();
    let card = serde_json::to_vec(&ContactCard::new("Sam")).unwrap();
    legacy
        .execute(
            "INSERT INTO decoy_contacts (id, card_encrypted, created_at, updated_at, display_name_encrypted)
             VALUES ('decoy-sam', ?1, 100, 200, ?2)",
            rusqlite::params![encrypt(&mask, &card).unwrap(), encrypt(&mask, b"Sam").unwrap()],
        )
        .unwrap();
    legacy
        .execute(
            "INSERT INTO duress_tags (id, name_encrypted, created_at) VALUES ('t1', ?1, 1)",
            [encrypt(&mask, b"Book club").unwrap()],
        )
        .unwrap();
    super::duress_content::write_sealed(&storage.conn, &legacy, &mask).unwrap();
    (storage, key)
}

// @internal
#[test]
fn a_legacy_seal_under_the_mask_key_converts_and_is_sealed_again_in_the_new_shape() {
    let (storage, key) = with_a_legacy_seal();

    storage.load_duress_content().unwrap();

    let decoys = storage.decoy().load_decoy_contacts().unwrap();
    assert_eq!(
        decoys
            .iter()
            .map(|(id, name, card)| (id.as_str(), name.as_str(), card.display_name()))
            .collect::<Vec<_>>(),
        [("decoy-sam", "Sam", "Sam")]
    );
    let tags = storage.duress_tags().list_tags().unwrap();
    assert_eq!(
        tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Book club"]
    );
    let sealed = SealedSlot::of(&storage.conn).read().unwrap().unwrap();
    let names: Vec<String> = decode_tables(&open_seal(&mask_key(&key), &sealed).unwrap())
        .unwrap()
        .into_iter()
        .map(|table| table.name)
        .collect();
    assert!(
        names
            .iter()
            .all(|name| !LEGACY_TABLES.contains(&name.as_str())),
        "{names:?}"
    );
    assert!(names.contains(&"contacts".to_string()), "{names:?}");
}
