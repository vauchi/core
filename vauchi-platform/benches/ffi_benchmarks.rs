// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! FFI Performance Benchmarks
//!
//! Measures FFI overhead and critical path performance for mobile operations
//! through the live `PlatformAppEngine` surface (the legacy `VauchiPlatform`
//! identity methods were retired with the collapse-into-engine work). Identity
//! reads go through the sanctioned `dispatch_domain_command` router.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;
use tempfile::TempDir;
use vauchi_platform::{DomainCommand, PlatformAppEngine};

/// Setup helper to create a fresh engine instance (no identity).
fn create_test_instance() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let instance = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .unwrap();
    (instance, dir)
}

/// Setup helper to create an engine with an identity, via the onboarding
/// `UserAction` flow (the production path; mirrors the integration tests).
fn create_instance_with_identity(name: &str) -> (Arc<PlatformAppEngine>, TempDir) {
    let (instance, dir) = create_test_instance();
    instance
        .dispatch_domain_command(DomainCommand::CreateIdentity {
            display_name: name.to_string(),
        })
        .unwrap();
    (instance, dir)
}

/// Benchmark identity creation overhead.
fn bench_identity_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("identity");

    group.bench_function("create", |b| {
        b.iter_with_setup(create_test_instance, |(instance, _dir)| {
            black_box(
                instance
                    .dispatch_domain_command(DomainCommand::CreateIdentity {
                        display_name: "Benchmark User".to_string(),
                    })
                    .unwrap(),
            );
        })
    });

    group.bench_function("has_identity_cold", |b| {
        b.iter_with_setup(
            || create_instance_with_identity("Test User"),
            |(instance, _dir)| {
                black_box(instance.has_identity().unwrap());
            },
        )
    });

    group.bench_function("get_public_id", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(
                instance
                    .dispatch_domain_command(DomainCommand::GetPublicId)
                    .unwrap(),
            );
        })
    });

    group.bench_function("get_display_name", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(
                instance
                    .dispatch_domain_command(DomainCommand::GetDisplayName)
                    .unwrap(),
            );
        })
    });

    group.finish();
}

/// Benchmark storage operations overhead.
fn bench_storage(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage");

    // Measure the overhead of opening storage connections, indirectly via
    // has_identity (which reads through the engine's storage layer).
    group.bench_function("open_storage_overhead", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(instance.has_identity().unwrap());
        })
    });

    group.finish();
}

criterion_group!(benches, bench_identity_creation, bench_storage,);

criterion_main!(benches);
