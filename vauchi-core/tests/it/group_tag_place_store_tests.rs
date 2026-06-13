// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the tag/place persistence store-boundary views.
//! (GroupStore retired in `ADR-054`; this file keeps its historical name.)
//!
//! Part of problem `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).
//! Each store owns a single self-contained table; per-contact exchange
//! locations stay on `Storage` (they live on the `contacts` row).

use vauchi_core::{PlaceStore, Storage, SymmetricKey, TagStore};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// Consumers scoped to one domain receive only that store — statically unable to
// reach any other table.
fn tag_count_via_scoped_view(store: &TagStore<'_>) -> usize {
    store.list_tags().unwrap().len()
}
fn place_count_via_scoped_view(store: &PlaceStore<'_>) -> usize {
    store.list_places().unwrap().len()
}

// @internal
#[test]
fn test_tag_store_name_roundtrip_encrypted() {
    let storage = test_storage();
    assert_eq!(tag_count_via_scoped_view(&storage.tags()), 0);

    let tag = storage.tags().create_tag("VIP").unwrap();

    let loaded = storage.tags().get_tag(&tag.id).unwrap().unwrap();
    assert_eq!(loaded.name, "VIP");
    assert_eq!(storage.tags().list_tags().unwrap().len(), 1);
}

// @internal
#[test]
fn test_place_store_create_and_find_near() {
    let storage = test_storage();
    assert_eq!(place_count_via_scoped_view(&storage.places()), 0);

    let place = storage.places().create_place("Home", 47.0, 8.0).unwrap();

    let found = storage.places().find_place_near(47.0, 8.0).unwrap();
    assert_eq!(found.map(|p| p.id), Some(place.id));
    assert_eq!(storage.places().list_places().unwrap().len(), 1);
}
