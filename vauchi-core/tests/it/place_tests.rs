// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for named places (owner-private `name → coords` vocabulary) — the
//! `Place` type, encrypted storage CRUD (`places` table, migration v50), and
//! proximity lookup. See `ADR-051`.

use vauchi_core::contact::place::{PLACE_MATCH_RADIUS_M, Place, haversine_m};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;

fn open_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// Berlin landmarks for proximity fixtures.
const ANCHOR_LAT: f64 = 52.5200;
const ANCHOR_LON: f64 = 13.4050;

// @scenario: contact-annotations.feature - Name a place
// @internal
#[test]
fn haversine_known_distance_is_accurate() {
    // Berlin → Hamburg is ~255 km; allow 2% tolerance.
    let d = haversine_m(52.5200, 13.4050, 53.5511, 9.9937);
    assert!(
        (250_000.0..260_000.0).contains(&d),
        "expected ~255 km, got {d:.0} m"
    );
    // Zero distance to itself.
    assert!(haversine_m(ANCHOR_LAT, ANCHOR_LON, ANCHOR_LAT, ANCHOR_LON) < 0.001);
}

// @scenario: contact-annotations.feature - Name a place
// @internal
#[test]
fn create_place_round_trips_through_get() {
    let storage = open_storage();
    let created = storage
        .places()
        .create_place("The Anchor Bar", ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    let loaded = storage.places().get_place(&created.id).unwrap().unwrap();
    assert_eq!(loaded.name, "The Anchor Bar", "name must decrypt back");
    assert!((loaded.latitude - ANCHOR_LAT).abs() < 1e-9);
    assert!((loaded.longitude - ANCHOR_LON).abs() < 1e-9);
}

// @scenario: contact-annotations.feature - Name a place and have it auto-suggest by proximity
// @internal
#[test]
fn find_place_near_matches_within_radius_only() {
    let storage = open_storage();
    let anchor = storage
        .places()
        .create_place("The Anchor Bar", ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    // ~30 m north (0.00027° lat) — within the 100 m radius.
    let near = storage
        .places()
        .find_place_near(ANCHOR_LAT + 0.00027, ANCHOR_LON)
        .unwrap();
    assert_eq!(
        near.map(|p| p.id),
        Some(anchor.id),
        "a point ~30 m away matches the named place"
    );

    // ~1 km away — outside the radius, no match.
    let far = storage
        .places()
        .find_place_near(ANCHOR_LAT + 0.01, ANCHOR_LON)
        .unwrap();
    assert!(far.is_none(), "a point ~1 km away must not match");
}

// @scenario: contact-annotations.feature - Name a place and have it auto-suggest by proximity
// @internal
#[test]
fn find_place_near_returns_closest_of_several() {
    let storage = open_storage();
    let _far = storage
        .places()
        .create_place("Far Cafe", ANCHOR_LAT + 0.0008, ANCHOR_LON)
        .unwrap(); // ~89 m
    let close = storage
        .places()
        .create_place("Close Bar", ANCHOR_LAT + 0.0001, ANCHOR_LON)
        .unwrap(); // ~11 m

    let found = storage
        .places()
        .find_place_near(ANCHOR_LAT, ANCHOR_LON)
        .unwrap();
    assert_eq!(
        found.map(|p| p.id),
        Some(close.id),
        "the nearest place within radius wins"
    );
    assert!(PLACE_MATCH_RADIUS_M > 0.0);
}

// @scenario: contact-annotations.feature - Name a place
// @internal
#[test]
fn delete_and_list_places() {
    let storage = open_storage();
    let a = storage
        .places()
        .create_place("A", ANCHOR_LAT, ANCHOR_LON)
        .unwrap();
    storage
        .places()
        .create_place("B", ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    assert_eq!(storage.places().list_places().unwrap().len(), 2);
    assert!(storage.places().delete_place(&a.id).unwrap());
    assert_eq!(storage.places().list_places().unwrap().len(), 1);
    assert!(
        !storage.places().delete_place(&a.id).unwrap(),
        "second delete absent"
    );
}

// @scenario: contact-annotations.feature - Tags are never shared (at-rest)
// @internal
#[test]
fn place_name_and_coords_encrypted_at_rest() {
    let storage = open_storage();
    let p = storage
        .places()
        .create_place("Secret Therapist Office", ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    let raw: Vec<u8> = storage
        .connection()
        .query_row(
            "SELECT data_encrypted FROM places WHERE id = ?1",
            rusqlite::params![p.id],
            |row| row.get(0),
        )
        .unwrap();

    let needle = b"Secret Therapist Office";
    assert!(
        !raw.windows(needle.len()).any(|w| w == needle),
        "plaintext place name must not appear in the stored BLOB"
    );
}

// @scenario: contact-annotations.feature - Name a place
// @internal
#[test]
fn place_survives_storage_rekey() {
    let mut storage = open_storage();
    let p = storage
        .places()
        .create_place("The Anchor Bar", ANCHOR_LAT, ANCHOR_LON)
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    let loaded = storage.places().get_place(&p.id).unwrap().unwrap();
    assert_eq!(loaded.name, "The Anchor Bar", "name decrypts after rekey");
    assert!(
        (loaded.latitude - ANCHOR_LAT).abs() < 1e-9,
        "coords survive rekey"
    );
}

// @scenario: contact-annotations.feature - Name a place
// @internal
#[test]
fn save_place_preserves_id_for_sync() {
    let storage = open_storage();
    let original = Place::new("Synced Spot", ANCHOR_LAT, ANCHOR_LON, 42);

    storage.places().save_place(&original).unwrap();
    let loaded = storage.places().get_place(&original.id).unwrap().unwrap();

    assert_eq!(loaded.id, original.id, "save_place keeps the supplied id");
    assert_eq!(loaded.created_at, 42);
}
