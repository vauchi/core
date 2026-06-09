// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the device-domain persistence view (`DeviceStore`).
//!
//! Part of problem `2026-06-09-storage-per-domain-store-boundaries` (Phase 1).
//! `DeviceStore` owns `device_info` and `device_registry`; sync state is split
//! out into `SyncStore`.

use vauchi_core::{DeviceStore, Storage, SymmetricKey};

fn test_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

// A consumer scoped to the device domain receives only `&DeviceStore` — it is
// statically unable to reach contacts, sync, or any other table.
fn device_index_via_scoped_view(store: &DeviceStore<'_>) -> Option<u32> {
    store
        .load_device_info()
        .unwrap()
        .map(|(_, index, _, _)| index)
}

// @internal
#[test]
fn test_device_store_info_roundtrip() {
    let storage = test_storage();
    let device_id = [7u8; 32];

    assert_eq!(device_index_via_scoped_view(&storage.device()), None);
    assert!(!storage.device().has_device_info().unwrap());

    storage
        .device()
        .save_device_info(&device_id, 3, "Pixel", 1000)
        .unwrap();

    assert_eq!(device_index_via_scoped_view(&storage.device()), Some(3));
    assert!(storage.device().has_device_info().unwrap());
    // Visible through the legacy forwarding API — one connection.
    let (id, index, name, created) = storage.load_device_info().unwrap().unwrap();
    assert_eq!(id, device_id);
    assert_eq!(index, 3);
    assert_eq!(name, "Pixel");
    assert_eq!(created, 1000);
}

// @internal
#[test]
fn test_device_store_clear_info_wipes_row() {
    let storage = test_storage();
    storage
        .device()
        .save_device_info(&[1u8; 32], 0, "Phone", 1)
        .unwrap();

    storage.device().clear_device_info().unwrap();

    assert!(!storage.device().has_device_info().unwrap());
}
