// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Performance Benchmarks for Crypto and Storage Operations
//!
//! Run with: cargo bench -p vauchi-core

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

// =============================================================================
// SYMMETRIC ENCRYPTION BENCHMARKS
// =============================================================================

fn bench_symmetric_encryption(c: &mut Criterion) {
    use vauchi_core::crypto::{SymmetricKey, decrypt, encrypt};

    let key = SymmetricKey::generate();

    let mut group = c.benchmark_group("symmetric_encryption");

    // Small message (typical contact field)
    let small_msg = b"alice@example.com";
    group.throughput(Throughput::Bytes(small_msg.len() as u64));
    group.bench_function("encrypt_small_17B", |b| {
        b.iter(|| encrypt(black_box(&key), black_box(small_msg)))
    });

    // Medium message (typical contact card JSON)
    let medium_msg = vec![b'x'; 1024];
    group.throughput(Throughput::Bytes(1024));
    group.bench_function("encrypt_medium_1KB", |b| {
        b.iter(|| encrypt(black_box(&key), black_box(&medium_msg)))
    });

    // Large message (worst case sync payload)
    let large_msg = vec![b'x'; 64 * 1024];
    group.throughput(Throughput::Bytes(64 * 1024));
    group.bench_function("encrypt_large_64KB", |b| {
        b.iter(|| encrypt(black_box(&key), black_box(&large_msg)))
    });

    // Decryption benchmarks
    let small_encrypted = encrypt(&key, small_msg).unwrap();
    group.bench_function("decrypt_small_17B", |b| {
        b.iter(|| decrypt(black_box(&key), black_box(&small_encrypted)))
    });

    let medium_encrypted = encrypt(&key, &medium_msg).unwrap();
    group.bench_function("decrypt_medium_1KB", |b| {
        b.iter(|| decrypt(black_box(&key), black_box(&medium_encrypted)))
    });

    group.finish();
}

// =============================================================================
// KEY GENERATION BENCHMARKS
// =============================================================================

fn bench_key_generation(c: &mut Criterion) {
    use vauchi_core::crypto::{SigningKeyPair, SymmetricKey};
    use vauchi_core::exchange::X3DHKeyPair;

    let mut group = c.benchmark_group("key_generation");

    group.bench_function("symmetric_key", |b| b.iter(SymmetricKey::generate));

    group.bench_function("signing_keypair", |b| b.iter(SigningKeyPair::generate));

    group.bench_function("x3dh_keypair", |b| b.iter(X3DHKeyPair::generate));

    group.finish();
}

// =============================================================================
// SIGNING BENCHMARKS
// =============================================================================

fn bench_signing(c: &mut Criterion) {
    use vauchi_core::crypto::SigningKeyPair;

    let keypair = SigningKeyPair::generate();
    let message = b"This is a test message for signing benchmarks";

    let mut group = c.benchmark_group("signing");

    group.bench_function("sign", |b| b.iter(|| keypair.sign(black_box(message))));

    let signature = keypair.sign(message);
    group.bench_function("verify", |b| {
        b.iter(|| {
            keypair
                .public_key()
                .verify(black_box(message), black_box(&signature))
        })
    });

    group.finish();
}

// =============================================================================
// PASSWORD KDF BENCHMARKS (Argon2id)
// =============================================================================

// Baselines password-based key derivation under production parameters
// (M=64 MiB, T=3, P=4) so a silent weakening shows up as a perf drop
// in the bench-compare CI gate. A weakening of the *source constants*
// is caught at compile time by `password_kdf.rs::const _`, but a
// drift in the upstream `argon2` crate's implementation (e.g. an
// accidental optimization that effectively reduces work) only shows
// up here.
//
// Sample size reduced from Criterion's 100 default — each derivation
// is ~80-150ms on a typical machine; 10 samples ≈ ~3-5s of bench
// time. Sufficient for trend detection in the nightly bench job
// while keeping wall-clock cost acceptable.
fn bench_password_kdf(c: &mut Criterion) {
    use vauchi_core::crypto::password_kdf::derive_key_argon2id;

    let mut group = c.benchmark_group("password_kdf");
    group.sample_size(10);
    group.bench_function("argon2id_derive_key", |b| {
        b.iter(|| {
            derive_key_argon2id(
                black_box(b"correct-horse-battery-staple"),
                black_box(b"randomly_chosen!"),
            )
        })
    });
    group.finish();
}

// =============================================================================
// HKDF BENCHMARKS
// =============================================================================

fn bench_hkdf(c: &mut Criterion) {
    use vauchi_core::crypto::HKDF;

    let ikm = [0x42u8; 32];
    let salt = [0x00u8; 32];

    let mut group = c.benchmark_group("hkdf");

    group.bench_function("derive_32_bytes", |b| {
        b.iter(|| {
            HKDF::derive_key(
                black_box(Some(&salt)),
                black_box(&ikm),
                black_box(b"Vauchi_Test"),
            )
        })
    });

    group.bench_function("derive_64_bytes", |b| {
        b.iter(|| {
            HKDF::derive(
                black_box(Some(&salt)),
                black_box(&ikm),
                black_box(b"Vauchi_Test"),
                black_box(64),
            )
        })
    });

    group.finish();
}

// =============================================================================
// DOUBLE RATCHET BENCHMARKS
// =============================================================================

fn bench_double_ratchet(c: &mut Criterion) {
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::crypto::ratchet::DoubleRatchetState;
    use vauchi_core::exchange::X3DHKeyPair;

    let mut group = c.benchmark_group("double_ratchet");

    // Setup: fixed values for benchmarks
    let shared_secret = SymmetricKey::generate();
    let bob_public = *X3DHKeyPair::generate().public_key();

    group.bench_function("initialize_initiator", |b| {
        b.iter(|| {
            DoubleRatchetState::initialize_initiator(
                black_box(&shared_secret),
                black_box(bob_public),
            )
        })
    });

    group.bench_function("initialize_responder", |b| {
        b.iter(|| {
            let dh = X3DHKeyPair::generate();
            DoubleRatchetState::initialize_responder(black_box(&shared_secret), black_box(dh))
        })
    });

    // For encrypt/decrypt we need to create fresh states each iteration
    // since DoubleRatchetState doesn't implement Clone
    let message = b"Hello, Bob! This is a test message.";

    group.bench_function("encrypt_message", |b| {
        b.iter_batched(
            || {
                // Setup: create a fresh initiator state that can send
                let secret = SymmetricKey::generate();
                let bob = X3DHKeyPair::generate();
                DoubleRatchetState::initialize_initiator(&secret, *bob.public_key()).unwrap()
            },
            |mut ratchet| ratchet.encrypt(black_box(message)),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("decrypt_message", |b| {
        b.iter_batched(
            || {
                // Setup: create alice and bob, exchange messages so bob can decrypt
                let secret = SymmetricKey::generate();
                let bob_keys = X3DHKeyPair::generate();
                let bob_pub = *bob_keys.public_key();

                let mut alice = DoubleRatchetState::initialize_initiator(&secret, bob_pub).unwrap();
                let encrypted = alice.encrypt(message).unwrap();

                let bob = DoubleRatchetState::initialize_responder(&secret, bob_keys);
                (bob, encrypted)
            },
            |(mut ratchet, msg)| ratchet.decrypt(black_box(&msg)),
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// =============================================================================
// X3DH KEY EXCHANGE BENCHMARKS
// =============================================================================

fn bench_x3dh(c: &mut Criterion) {
    use vauchi_core::exchange::{X3DH, X3DHKeyPair};

    let mut group = c.benchmark_group("x3dh");

    let alice = X3DHKeyPair::generate();
    let bob = X3DHKeyPair::generate();

    group.bench_function("initiate", |b| {
        b.iter(|| X3DH::initiate(black_box(&alice), black_box(bob.public_key())))
    });

    let (_, ephemeral_public) = X3DH::initiate(&alice, bob.public_key()).unwrap();
    group.bench_function("respond", |b| {
        b.iter(|| {
            X3DH::respond(
                black_box(&bob),
                black_box(alice.public_key()),
                black_box(&ephemeral_public),
            )
        })
    });

    group.finish();
}

// =============================================================================
// STORAGE BENCHMARKS
// =============================================================================

fn bench_storage(c: &mut Criterion) {
    use vauchi_core::contact::Contact;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::storage::Storage;
    use vauchi_core::{ContactCard, ContactField, FieldType};

    let mut group = c.benchmark_group("storage");

    // Create test contact
    fn create_test_contact() -> Contact {
        let mut card = ContactCard::new("Benchmark User");
        card.add_field(ContactField::new(
            FieldType::Email,
            "Work",
            "bench@example.com",
            0,
        ))
        .unwrap();
        let shared_key = SymmetricKey::generate();
        Contact::from_exchange([0u8; 32], card, shared_key, 0)
    }

    group.bench_function("save_contact", |b| {
        b.iter_batched(
            || {
                let key = SymmetricKey::generate();
                let storage = Storage::in_memory(key).unwrap();
                let contact = create_test_contact();
                (storage, contact)
            },
            |(storage, contact)| storage.contacts().save_contact(black_box(&contact)),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("load_contact", |b| {
        b.iter_batched(
            || {
                let key = SymmetricKey::generate();
                let storage = Storage::in_memory(key).unwrap();
                let contact = create_test_contact();
                storage.contacts().save_contact(&contact).unwrap();
                (storage, contact.id().to_string())
            },
            |(storage, id)| storage.contacts().load_contact(black_box(&id)),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("list_contacts_10", |b| {
        b.iter_batched(
            || {
                let key = SymmetricKey::generate();
                let storage = Storage::in_memory(key).unwrap();
                for _ in 0..10 {
                    let contact = create_test_contact();
                    storage.contacts().save_contact(&contact).unwrap();
                }
                storage
            },
            |storage| storage.contacts().list_contacts(),
            criterion::BatchSize::SmallInput,
        )
    });

    // Own card benchmarks
    let card = ContactCard::new("Benchmark Owner");
    group.bench_function("save_own_card", |b| {
        b.iter_batched(
            || {
                let key = SymmetricKey::generate();
                Storage::in_memory(key).unwrap()
            },
            |storage| storage.contacts().save_own_card(black_box(&card)),
            criterion::BatchSize::SmallInput,
        )
    });

    group.bench_function("load_own_card", |b| {
        b.iter_batched(
            || {
                let key = SymmetricKey::generate();
                let storage = Storage::in_memory(key).unwrap();
                storage.contacts().save_own_card(&card).unwrap();
                storage
            },
            |storage| storage.contacts().load_own_card(),
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

// =============================================================================
// SERIALIZATION BENCHMARKS
// =============================================================================

fn bench_serialization(c: &mut Criterion) {
    use vauchi_core::{ContactCard, ContactField, FieldType};

    let mut group = c.benchmark_group("serialization");

    // Create a realistic contact card
    let mut card = ContactCard::new("Benchmark User");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "work@example.com",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "Personal",
        "personal@example.com",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-123-4567",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Social,
        "Twitter",
        "@benchuser",
        0,
    ))
    .unwrap();

    let card_json = serde_json::to_string(&card).unwrap();

    group.bench_function("serialize_contact_card", |b| {
        b.iter(|| serde_json::to_string(black_box(&card)))
    });

    group.bench_function("deserialize_contact_card", |b| {
        b.iter(|| serde_json::from_str::<ContactCard>(black_box(&card_json)))
    });

    group.finish();
}

// =============================================================================
// PAGINATION & SEARCH SCALING BENCHMARKS
// =============================================================================

fn bench_pagination(c: &mut Criterion) {
    use vauchi_core::contact::Contact;
    use vauchi_core::crypto::SymmetricKey;
    use vauchi_core::storage::Storage;
    use vauchi_core::{ContactCard, ContactField, FieldType};

    fn create_contact_n(n: usize) -> Contact {
        let mut card = ContactCard::new(&format!("Contact {:04}", n));
        card.add_field(ContactField::new(
            FieldType::Email,
            "Work",
            &format!("c{}@example.com", n),
            0,
        ))
        .unwrap();
        let mut pk = [0u8; 32];
        pk[..8].copy_from_slice(&(n as u64).to_be_bytes());
        Contact::from_exchange(pk, card, SymmetricKey::generate(), 0)
    }

    let mut group = c.benchmark_group("pagination");

    // Benchmark paginated listing at different scales
    for &count in &[100, 500, 1000] {
        group.bench_function(format!("list_paginated_{}_page50", count), |b| {
            b.iter_batched(
                || {
                    let key = SymmetricKey::generate();
                    let storage = Storage::in_memory(key).unwrap();
                    for i in 0..count {
                        storage
                            .contacts()
                            .save_contact(&create_contact_n(i))
                            .unwrap();
                    }
                    storage
                },
                |storage| {
                    storage
                        .contacts()
                        .list_contacts_paginated(black_box(0), black_box(50))
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }

    // Benchmark search at different scales
    for &count in &[100, 500, 1000] {
        group.bench_function(format!("search_{}_contacts", count), |b| {
            b.iter_batched(
                || {
                    let key = SymmetricKey::generate();
                    let storage = Storage::in_memory(key).unwrap();
                    for i in 0..count {
                        storage
                            .contacts()
                            .save_contact(&create_contact_n(i))
                            .unwrap();
                    }
                    storage
                },
                |storage| storage.contacts().search_contacts(black_box("Contact 05")),
                criterion::BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

// =============================================================================
// MAIN
// =============================================================================

criterion_group!(
    benches,
    bench_symmetric_encryption,
    bench_key_generation,
    bench_signing,
    bench_password_kdf,
    bench_hkdf,
    bench_double_ratchet,
    bench_x3dh,
    bench_storage,
    bench_serialization,
    bench_pagination,
);

criterion_main!(benches);
