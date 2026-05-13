// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Performance Regression Benchmarks for Vauchi Core
//!
//! These benchmarks establish performance baselines and detect regressions for
//! critical user-facing operations. Each benchmark has a documented performance
//! target that should be maintained across releases.
//!
//! # Performance Targets
//!
//! | Benchmark                      | Target          | Rationale                          |
//! |--------------------------------|-----------------|-------------------------------------|
//! | `bench_cold_start`             | < 2 seconds     | App launch responsiveness           |
//! | `bench_warm_start`             | < 500 ms        | Resume/reopen responsiveness        |
//! | `bench_contact_list_scroll`    | < 16 ms/page    | 60 FPS scroll performance           |
//! | `bench_memory_500_contacts`    | < 50 MB         | Reasonable memory footprint         |
//! | `bench_sync_latency_10_devices`| < 5 seconds     | Multi-device sync user experience   |
//!
//! # Running Benchmarks
//!
//! ```bash
//! cargo bench -p vauchi-core --bench performance_regression
//! ```
//!
//! To compare against a baseline:
//! ```bash
//! cargo bench -p vauchi-core --bench performance_regression -- --save-baseline main
//! # ... make changes ...
//! cargo bench -p vauchi-core --bench performance_regression -- --baseline main
//! ```

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::hint::black_box as hint_black_box;

use vauchi_core::contact::Contact;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::sync::{DeviceSyncPayload, InterDeviceSyncState, SyncItem, VersionVector};
use vauchi_core::{ContactCard, ContactField, FieldType};

// =============================================================================
// TEST DATA HELPERS
// =============================================================================

/// Creates a test contact with a unique index for benchmarking.
fn create_test_contact(index: usize) -> Contact {
    let mut card = ContactCard::new(&format!("Contact {:04}", index));

    // Add typical fields a real contact would have
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        &format!("contact{}@example.com", index),
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        &format!("+1-555-{:03}-{:04}", index / 10000, index % 10000),
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Social,
        "Twitter",
        &format!("@user{}", index),
        0,
    ))
    .unwrap();

    // Create unique public key based on index
    let mut pk = [0u8; 32];
    pk[..8].copy_from_slice(&(index as u64).to_be_bytes());

    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key, 0)
}

/// Creates a storage instance pre-populated with the given number of contacts.
fn create_populated_storage(contact_count: usize) -> Storage {
    let key = SymmetricKey::generate();
    let storage = Storage::in_memory(key).unwrap();

    for i in 0..contact_count {
        let contact = create_test_contact(i);
        storage.save_contact(&contact).unwrap();
    }

    storage
}

/// Creates a test ContactCard for own card operations.
fn create_own_card() -> ContactCard {
    let mut card = ContactCard::new("Test User");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Personal",
        "user@example.com",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-000-0000",
        0,
    ))
    .unwrap();
    card
}

// =============================================================================
// COLD START BENCHMARK
// =============================================================================
// Performance Target: < 2 seconds
//
// Cold start measures the time to initialize a fresh storage instance,
// run all schema migrations, and configure SQLite pragmas. This simulates
// the first app launch or after clearing app data.

fn bench_cold_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("startup");

    // Cold start: create new storage from scratch
    group.bench_function("cold_start_empty", |b| {
        b.iter(|| {
            let key = SymmetricKey::generate();
            let storage = Storage::in_memory(black_box(key)).unwrap();
            hint_black_box(storage);
        })
    });

    // Cold start with existing data (simulates app update scenario)
    // Pre-create a storage file, then measure opening it
    group.bench_function("cold_start_with_100_contacts", |b| {
        b.iter_batched(
            || {
                // Setup: create storage with contacts, get the encryption key
                let key = SymmetricKey::generate();
                let storage = Storage::in_memory(key.clone()).unwrap();
                for i in 0..100 {
                    storage.save_contact(&create_test_contact(i)).unwrap();
                }
                // Return storage for measurement (simulating "reopening")
                // In real file-based test, we'd reopen the file
                (storage, key)
            },
            |(storage, _key)| {
                // Measure: load all contacts (simulates app startup data load)
                let contacts = storage.list_contacts().unwrap();
                hint_black_box(contacts);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// =============================================================================
// WARM START BENCHMARK
// =============================================================================
// Performance Target: < 500 milliseconds
//
// Warm start measures the time to resume an already-initialized storage
// and load the contact list. This simulates returning to the app after
// backgrounding.

fn bench_warm_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("startup");

    // Pre-create storage with contacts
    let storage = create_populated_storage(100);

    group.bench_function("warm_start_list_100_contacts", |b| {
        b.iter(|| {
            let contacts = storage.list_contacts().unwrap();
            hint_black_box(contacts);
        })
    });

    // Warm start with paginated list (more realistic UI scenario)
    group.bench_function("warm_start_first_page_50", |b| {
        b.iter(|| {
            let contacts = storage.list_contacts_paginated(0, 50).unwrap();
            hint_black_box(contacts);
        })
    });

    group.finish();
}

// =============================================================================
// CONTACT LIST SCROLL BENCHMARK
// =============================================================================
// Performance Target: < 16ms per page (60 FPS)
//
// Measures pagination performance with 500 contacts. Each page load should
// complete within a single frame budget to maintain smooth scrolling.

fn bench_contact_list_scroll(c: &mut Criterion) {
    let mut group = c.benchmark_group("scroll_performance");

    // Pre-create storage with 500 contacts
    let storage = create_populated_storage(500);

    // Benchmark loading different pages
    for page in [0, 5, 9] {
        // First, middle, and last pages
        let offset = page * 50;
        group.bench_with_input(
            BenchmarkId::new("page_50", format!("offset_{}", offset)),
            &offset,
            |b, &offset| {
                b.iter(|| {
                    let contacts = storage.list_contacts_paginated(offset, 50).unwrap();
                    hint_black_box(contacts);
                })
            },
        );
    }

    // Benchmark search within 500 contacts
    group.bench_function("search_500_contacts", |b| {
        b.iter(|| {
            let contacts = storage.search_contacts(black_box("Contact 25")).unwrap();
            hint_black_box(contacts);
        })
    });

    // Benchmark scrolling simulation (sequential page loads)
    group.bench_function("scroll_10_pages_sequential", |b| {
        b.iter(|| {
            for page in 0..10 {
                let contacts = storage.list_contacts_paginated(page * 50, 50).unwrap();
                hint_black_box(contacts);
            }
        })
    });

    group.finish();
}

// =============================================================================
// MEMORY BENCHMARK WITH 500 CONTACTS
// =============================================================================
// Performance Target: < 50 MB total
//
// Measures memory allocation patterns when working with large contact lists.
// This uses Criterion's throughput tracking to monitor data volume.

fn bench_memory_500_contacts(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory");

    // Measure contact serialization size
    let contacts: Vec<Contact> = (0..500).map(create_test_contact).collect();
    let total_json_size: usize = contacts
        .iter()
        .map(|c| serde_json::to_string(c.card()).unwrap().len())
        .sum();

    group.throughput(Throughput::Bytes(total_json_size as u64));

    // Benchmark: load all 500 contacts into memory
    group.bench_function("load_500_contacts", |b| {
        b.iter_batched(
            || create_populated_storage(500),
            |storage| {
                let contacts = storage.list_contacts().unwrap();
                hint_black_box(contacts);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Benchmark: create DeviceSyncPayload with 500 contacts
    // This exercises the serialization path used in device sync
    let own_card = create_own_card();
    group.bench_function("create_sync_payload_500", |b| {
        b.iter(|| {
            let payload = DeviceSyncPayload::new(black_box(&contacts), black_box(&own_card), 1);
            hint_black_box(payload);
        })
    });

    // Benchmark: serialize and deserialize sync payload
    let payload = DeviceSyncPayload::new(&contacts, &own_card, 1);
    group.bench_function("serialize_sync_payload_500", |b| {
        b.iter(|| {
            let json = payload.to_json();
            hint_black_box(json);
        })
    });

    let payload_json = payload.to_json();
    group.bench_function("deserialize_sync_payload_500", |b| {
        b.iter(|| {
            let payload = DeviceSyncPayload::from_json(black_box(&payload_json)).unwrap();
            hint_black_box(payload);
        })
    });

    group.finish();
}

// =============================================================================
// SYNC LATENCY BENCHMARK (10 DEVICES)
// =============================================================================
// Performance Target: < 5 seconds for full sync
//
// Measures the time to synchronize state across 10 devices. This includes:
// - Creating sync payloads
// - Version vector operations
// - Conflict resolution
// - State tracking per device

fn bench_sync_latency_10_devices(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync");

    // Create test data
    let contacts: Vec<Contact> = (0..100).map(create_test_contact).collect();
    let own_card = create_own_card();

    // Create 10 device IDs
    let device_ids: Vec<[u8; 32]> = (0..10)
        .map(|i| {
            let mut id = [0u8; 32];
            id[0] = i;
            id
        })
        .collect();

    // Benchmark: create sync states for 10 devices
    group.bench_function("create_10_device_sync_states", |b| {
        b.iter(|| {
            let states: Vec<InterDeviceSyncState> = device_ids
                .iter()
                .map(|id| InterDeviceSyncState::new(*id))
                .collect();
            hint_black_box(states);
        })
    });

    // Benchmark: queue sync items to 10 devices
    group.bench_function("queue_100_items_to_10_devices", |b| {
        b.iter_batched(
            || {
                let states: Vec<InterDeviceSyncState> = device_ids
                    .iter()
                    .map(|id| InterDeviceSyncState::new(*id))
                    .collect();
                let items: Vec<SyncItem> = contacts
                    .iter()
                    .map(|c| SyncItem::ContactAdded {
                        contact_data: vauchi_core::sync::ContactSyncData::from_contact(c),
                        timestamp: 1234567890,
                    })
                    .collect();
                (states, items)
            },
            |(mut states, items)| {
                for item in items {
                    for state in &mut states {
                        state.queue_item(item.clone());
                    }
                }
                hint_black_box(states);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Benchmark: version vector operations with 10 devices
    group.bench_function("version_vector_merge_10_devices", |b| {
        b.iter_batched(
            || {
                // Create 10 version vectors with different versions
                let vectors: Vec<VersionVector> = (0..10)
                    .map(|i| {
                        let mut vv = VersionVector::new();
                        for (j, device_id) in device_ids.iter().enumerate() {
                            // Each device has seen different versions
                            for _ in 0..(i + j) % 5 {
                                vv.increment(device_id);
                            }
                        }
                        vv
                    })
                    .collect();
                vectors
            },
            |vectors| {
                // Merge all vectors together
                let mut merged = vectors[0].clone();
                for vv in &vectors[1..] {
                    merged = VersionVector::merge(&merged, vv);
                }
                hint_black_box(merged);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Benchmark: conflict resolution with 10 devices
    group.bench_function("conflict_resolution_100_items", |b| {
        b.iter_batched(
            || {
                // Create conflicting items from different devices
                let items: Vec<(SyncItem, [u8; 32])> = (0..100)
                    .map(|i| {
                        let device_idx = i % 10;
                        let item = SyncItem::CardUpdated {
                            field_label: format!("field_{}", i / 10),
                            new_value: format!("value_{}_{}", device_idx, i),
                            timestamp: 1234567890 + (i as u64 % 5), // Some conflicts on timestamp
                        };
                        (item, device_ids[device_idx])
                    })
                    .collect();
                items
            },
            |items| {
                // Resolve conflicts between adjacent pairs
                for pair in items.windows(2) {
                    let winner =
                        SyncItem::resolve_conflict(&pair[0].0, &pair[1].0, &pair[0].1, &pair[1].1);
                    hint_black_box(winner);
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Benchmark: full sync simulation (create payload, serialize, deserialize)
    group.bench_function("full_sync_simulation_100_contacts_10_devices", |b| {
        b.iter(|| {
            // Step 1: Create payload
            let payload = DeviceSyncPayload::new(&contacts, &own_card, 1);

            // Step 2: Serialize for each device (simulates sending)
            for _ in 0..10 {
                let json = payload.to_json();
                hint_black_box(&json);
            }

            // Step 3: Deserialize on each device (simulates receiving)
            let json = payload.to_json();
            for _ in 0..10 {
                let received = DeviceSyncPayload::from_json(&json).unwrap();
                hint_black_box(received);
            }

            // Step 4: Update version vectors
            let mut vv = VersionVector::new();
            for device_id in &device_ids {
                vv.increment(device_id);
            }
            hint_black_box(vv);
        })
    });

    group.finish();
}

// =============================================================================
// ADDITIONAL REGRESSION BENCHMARKS
// =============================================================================

/// Benchmarks that help track specific operations prone to regression.
fn bench_regression_markers(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression_markers");

    // Contact save/load cycle - critical path for data persistence
    group.bench_function("contact_save_load_cycle", |b| {
        b.iter_batched(
            || {
                let key = SymmetricKey::generate();
                let storage = Storage::in_memory(key).unwrap();
                let contact = create_test_contact(0);
                (storage, contact)
            },
            |(storage, contact)| {
                storage.save_contact(&contact).unwrap();
                let loaded = storage.load_contact(contact.id()).unwrap();
                hint_black_box(loaded);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Own card save/load - frequently accessed data
    group.bench_function("own_card_save_load_cycle", |b| {
        b.iter_batched(
            || {
                let key = SymmetricKey::generate();
                let storage = Storage::in_memory(key).unwrap();
                let card = create_own_card();
                (storage, card)
            },
            |(storage, card)| {
                storage.save_own_card(&card).unwrap();
                let loaded = storage.load_own_card().unwrap();
                hint_black_box(loaded);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Contact deletion - should be fast for user responsiveness
    group.bench_function("contact_delete", |b| {
        b.iter_batched(
            || {
                let storage = create_populated_storage(100);
                let contacts = storage.list_contacts().unwrap();
                let id = contacts[50].id().to_string();
                (storage, id)
            },
            |(storage, id)| {
                let deleted = storage.delete_contact(&id).unwrap();
                hint_black_box(deleted);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// =============================================================================
// MAIN
// =============================================================================

criterion_group!(
    benches,
    bench_cold_start,
    bench_warm_start,
    bench_contact_list_scroll,
    bench_memory_500_contacts,
    bench_sync_latency_10_devices,
    bench_regression_markers,
);

criterion_main!(benches);
