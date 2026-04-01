// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rate limit error handling tests.
//!
//! Verifies that `NetworkError::RateLimited` carries the correct retry
//! information and converts correctly across crate boundaries.

use vauchi_core::network::NetworkError;

// @scenario: rate_limit :: NetworkError::RateLimited variant can be constructed
#[test]
fn test_rate_limited_error_variant_exists() {
    let err = NetworkError::RateLimited {
        retry_after_secs: 30,
    };

    // Verify the variant carries the correct value (CC-03: specific assertion)
    match &err {
        NetworkError::RateLimited { retry_after_secs } => {
            assert_eq!(*retry_after_secs, 30, "retry_after_secs must be preserved");
        }
        other => panic!("Expected RateLimited variant, got: {other:?}"),
    }
}

// @scenario: rate_limit :: RateLimited Display includes retry seconds
#[test]
fn test_rate_limited_display_includes_retry_seconds() {
    let err = NetworkError::RateLimited {
        retry_after_secs: 10,
    };
    let display = err.to_string();

    assert!(
        display.contains("10"),
        "Display should include retry seconds, got: {display}"
    );
    assert!(
        display.contains("Rate limited"),
        "Display should mention rate limiting, got: {display}"
    );
}

// @scenario: rate_limit :: RateLimited with zero retry_after_secs
#[test]
fn test_rate_limited_zero_retry_after() {
    let err = NetworkError::RateLimited {
        retry_after_secs: 0,
    };

    match &err {
        NetworkError::RateLimited { retry_after_secs } => {
            assert_eq!(
                *retry_after_secs, 0,
                "zero retry_after_secs must be representable"
            );
        }
        other => panic!("Expected RateLimited, got: {other:?}"),
    }
}

// @scenario: rate_limit :: RateLimited with large retry_after_secs
#[test]
fn test_rate_limited_large_retry_after() {
    let err = NetworkError::RateLimited {
        retry_after_secs: 3600,
    };

    match &err {
        NetworkError::RateLimited { retry_after_secs } => {
            assert_eq!(
                *retry_after_secs, 3600,
                "large retry_after_secs must be preserved"
            );
        }
        other => panic!("Expected RateLimited, got: {other:?}"),
    }
}

// @scenario: rate_limit :: RateLimited implements Clone
#[test]
fn test_rate_limited_clone_preserves_value() {
    let original = NetworkError::RateLimited {
        retry_after_secs: 42,
    };
    let cloned = original.clone();

    match (&original, &cloned) {
        (
            NetworkError::RateLimited {
                retry_after_secs: orig,
            },
            NetworkError::RateLimited {
                retry_after_secs: copy,
            },
        ) => {
            assert_eq!(orig, copy, "cloned value must match original");
        }
        _ => panic!("Both should be RateLimited variants"),
    }
}

// @scenario: rate_limit :: RateLimited is distinct from other error variants (CC-11)
#[test]
fn test_rate_limited_is_not_other_variants() {
    let rate_limited = NetworkError::RateLimited {
        retry_after_secs: 10,
    };

    // Verify it does NOT match other variants — failure path testing
    assert!(
        !matches!(rate_limited, NetworkError::ConnectionFailed(_)),
        "RateLimited must not match ConnectionFailed"
    );
    assert!(
        !matches!(rate_limited, NetworkError::Timeout),
        "RateLimited must not match Timeout"
    );
    assert!(
        !matches!(rate_limited, NetworkError::MaxRetriesExceeded),
        "RateLimited must not match MaxRetriesExceeded"
    );
}
