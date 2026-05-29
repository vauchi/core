// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! FFI Performance Benchmarks
//!
//! Measures FFI overhead and critical path performance for mobile operations.
//! These benchmarks help identify bottlenecks in the UniFFI bridge layer.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;
use tempfile::TempDir;
use vauchi_platform::VauchiPlatform;

/// Setup helper to create a test instance
fn create_test_instance() -> (Arc<VauchiPlatform>, TempDir) {
    let dir = TempDir::new().unwrap();
    let instance = VauchiPlatform::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
    )
    .unwrap();
    (instance, dir)
}

/// Setup helper to create an instance with identity
fn create_instance_with_identity(name: &str) -> (Arc<VauchiPlatform>, TempDir) {
    let (instance, dir) = create_test_instance();
    instance.create_identity(name.to_string()).unwrap();
    (instance, dir)
}

/// Benchmark identity creation overhead
fn bench_identity_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("identity");

    group.bench_function("create", |b| {
        b.iter_with_setup(create_test_instance, |(instance, _dir)| {
            let _: () = instance
                .create_identity("Benchmark User".to_string())
                .unwrap();
            black_box(());
        })
    });

    group.bench_function("has_identity_cold", |b| {
        b.iter_with_setup(
            || create_instance_with_identity("Test User"),
            |(instance, _dir)| {
                black_box(instance.has_identity());
            },
        )
    });

    group.bench_function("get_public_id", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(instance.get_public_id().unwrap());
        })
    });

    group.bench_function("get_display_name", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(instance.get_display_name().unwrap());
        })
    });

    group.finish();
}

/// Benchmark storage operations overhead
fn bench_storage(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage");

    // Measure the overhead of opening storage connections
    group.bench_function("open_storage_overhead", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        // This indirectly measures storage open overhead via has_identity
        b.iter(|| {
            black_box(instance.has_identity());
        })
    });

    group.finish();
}

criterion_group!(benches, bench_identity_creation, bench_storage,);

criterion_main!(benches);
