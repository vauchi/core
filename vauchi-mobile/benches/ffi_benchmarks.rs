//! FFI Performance Benchmarks
//!
//! Measures FFI overhead and critical path performance for mobile operations.
//! These benchmarks help identify bottlenecks in the UniFFI bridge layer.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use tempfile::TempDir;
use vauchi_mobile::VauchiMobile;

/// Setup helper to create a test instance
fn create_test_instance() -> (Arc<VauchiMobile>, TempDir) {
    let dir = TempDir::new().unwrap();
    let instance = VauchiMobile::new(
        dir.path().to_string_lossy().to_string(),
        "ws://localhost:8080".to_string(),
    )
    .unwrap();
    (instance, dir)
}

/// Setup helper to create an instance with identity
fn create_instance_with_identity(name: &str) -> (Arc<VauchiMobile>, TempDir) {
    let (instance, dir) = create_test_instance();
    instance.create_identity(name.to_string()).unwrap();
    (instance, dir)
}

/// Benchmark identity creation overhead
fn bench_identity_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("identity");

    group.bench_function("create", |b| {
        b.iter_with_setup(
            || create_test_instance(),
            |(instance, _dir)| {
                black_box(
                    instance
                        .create_identity("Benchmark User".to_string())
                        .unwrap(),
                );
            },
        )
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

/// Benchmark contact card operations
fn bench_card_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("card");

    group.bench_function("get_own_card", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(instance.get_own_card().unwrap());
        })
    });

    group.bench_function("add_field", |b| {
        b.iter_with_setup(
            || create_instance_with_identity("Test User"),
            |(instance, _dir)| {
                black_box(
                    instance
                        .add_field(
                            vauchi_mobile::MobileFieldType::Email,
                            "work".to_string(),
                            "test@example.com".to_string(),
                        )
                        .unwrap(),
                );
            },
        )
    });

    group.bench_function("update_field", |b| {
        b.iter_with_setup(
            || {
                let (instance, dir) = create_instance_with_identity("Test User");
                instance
                    .add_field(
                        vauchi_mobile::MobileFieldType::Email,
                        "work".to_string(),
                        "old@example.com".to_string(),
                    )
                    .unwrap();
                (instance, dir)
            },
            |(instance, _dir)| {
                black_box(
                    instance
                        .update_field("work".to_string(), "new@example.com".to_string())
                        .unwrap(),
                );
            },
        )
    });

    group.finish();
}

/// Benchmark contact list operations at various scales
fn bench_contact_list_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("contacts_scaling");
    group.sample_size(20); // Reduce sample size for slower benchmarks

    for count in [0, 10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*count as u64));

        group.bench_with_input(BenchmarkId::new("list_contacts", count), count, |b, &_| {
            let (instance, _dir) = create_instance_with_identity("Test User");
            // Note: We can't easily create contacts without full exchange,
            // so this benchmarks empty/near-empty list performance
            b.iter(|| {
                black_box(instance.list_contacts().unwrap());
            });
        });

        group.bench_with_input(BenchmarkId::new("contact_count", count), count, |b, &_| {
            let (instance, _dir) = create_instance_with_identity("Test User");
            b.iter(|| {
                black_box(instance.contact_count().unwrap());
            });
        });

        group.bench_with_input(
            BenchmarkId::new("search_contacts", count),
            count,
            |b, &_| {
                let (instance, _dir) = create_instance_with_identity("Test User");
                b.iter(|| {
                    black_box(instance.search_contacts("test".to_string()).unwrap());
                });
            },
        );
    }

    group.finish();
}

/// Benchmark exchange QR generation
fn bench_exchange_qr(c: &mut Criterion) {
    let mut group = c.benchmark_group("exchange");

    group.bench_function("generate_qr", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(instance.generate_exchange_qr().unwrap());
        })
    });

    group.finish();
}

/// Benchmark backup/restore operations
fn bench_backup(c: &mut Criterion) {
    let mut group = c.benchmark_group("backup");
    group.sample_size(20); // Backup is computationally expensive

    group.bench_function("export", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(
                instance
                    .export_backup("correct-horse-battery-staple".to_string())
                    .unwrap(),
            );
        })
    });

    group.bench_function("import", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        let backup = instance
            .export_backup("correct-horse-battery-staple".to_string())
            .unwrap();

        b.iter_with_setup(
            || create_test_instance(),
            |(new_instance, _dir)| {
                black_box(
                    new_instance
                        .import_backup(backup.clone(), "correct-horse-battery-staple".to_string())
                        .unwrap(),
                );
            },
        )
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

/// Benchmark visibility label operations
fn bench_labels(c: &mut Criterion) {
    let mut group = c.benchmark_group("labels");

    group.bench_function("list_labels", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(instance.list_labels().unwrap());
        })
    });

    group.bench_function("create_label", |b| {
        b.iter_with_setup(
            || create_instance_with_identity("Test User"),
            |(instance, _dir)| {
                black_box(instance.create_label("Test Label".to_string()).unwrap());
            },
        )
    });

    group.bench_function("get_suggested_labels", |b| {
        let (instance, _dir) = create_instance_with_identity("Test User");
        b.iter(|| {
            black_box(instance.get_suggested_labels());
        })
    });

    group.finish();
}

/// Benchmark social network lookup
fn bench_social_networks(c: &mut Criterion) {
    let mut group = c.benchmark_group("social");

    group.bench_function("list_all", |b| {
        let (instance, _dir) = create_test_instance();
        b.iter(|| {
            black_box(instance.list_social_networks());
        })
    });

    group.bench_function("search", |b| {
        let (instance, _dir) = create_test_instance();
        b.iter(|| {
            black_box(instance.search_social_networks("git".to_string()));
        })
    });

    group.bench_function("get_profile_url", |b| {
        let (instance, _dir) = create_test_instance();
        b.iter(|| {
            black_box(instance.get_profile_url("github".to_string(), "octocat".to_string()));
        })
    });

    group.finish();
}

/// Benchmark password strength checking (utility function)
fn bench_password_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("password");

    group.bench_function("check_weak", |b| {
        b.iter(|| {
            black_box(vauchi_mobile::check_password_strength(
                "password".to_string(),
            ));
        })
    });

    group.bench_function("check_strong", |b| {
        b.iter(|| {
            black_box(vauchi_mobile::check_password_strength(
                "correct-horse-battery-staple".to_string(),
            ));
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_identity_creation,
    bench_card_operations,
    bench_contact_list_scaling,
    bench_exchange_qr,
    bench_backup,
    bench_storage,
    bench_labels,
    bench_social_networks,
    bench_password_check,
);

criterion_main!(benches);
