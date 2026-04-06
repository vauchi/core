// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the version module: APP_COMPAT_VERSION constant, VersionPolicy, and AppUpdateStatus.

use proptest::prelude::*;
use vauchi_core::version::{APP_COMPAT_VERSION, AppUpdateStatus, VersionPolicy};

// ---------------------------------------------------------------------------
// APP_COMPAT_VERSION constant
// ---------------------------------------------------------------------------

#[test]
fn app_compat_version_starts_at_one() {
    assert_eq!(APP_COMPAT_VERSION, 1);
}

// ---------------------------------------------------------------------------
// VersionPolicy::evaluate
// ---------------------------------------------------------------------------

#[test]
fn evaluate_up_to_date_when_at_warn_version() {
    let policy = VersionPolicy {
        min_version: 1,
        warn_version: 2,
        grace_deadline: None,
    };
    assert_eq!(policy.evaluate(2), AppUpdateStatus::UpToDate);
}

#[test]
fn evaluate_up_to_date_when_above_warn_version() {
    let policy = VersionPolicy {
        min_version: 1,
        warn_version: 2,
        grace_deadline: None,
    };
    assert_eq!(policy.evaluate(3), AppUpdateStatus::UpToDate);
}

#[test]
fn evaluate_update_available_when_at_min_but_below_warn() {
    let policy = VersionPolicy {
        min_version: 2,
        warn_version: 4,
        grace_deadline: None,
    };
    assert_eq!(policy.evaluate(3), AppUpdateStatus::UpdateAvailable);
}

#[test]
fn evaluate_update_available_when_exactly_at_min() {
    let policy = VersionPolicy {
        min_version: 2,
        warn_version: 4,
        grace_deadline: None,
    };
    assert_eq!(policy.evaluate(2), AppUpdateStatus::UpdateAvailable);
}

#[test]
fn evaluate_update_required_with_future_grace_deadline() {
    // Grace deadline far in the future (year 2099)
    let future_deadline = 4_102_444_800_u64;
    let policy = VersionPolicy {
        min_version: 3,
        warn_version: 5,
        grace_deadline: Some(future_deadline),
    };
    assert_eq!(
        policy.evaluate(2),
        AppUpdateStatus::UpdateRequired {
            grace_deadline: Some(future_deadline),
        }
    );
}

#[test]
fn evaluate_update_required_no_deadline() {
    let policy = VersionPolicy {
        min_version: 3,
        warn_version: 5,
        grace_deadline: None,
    };
    assert_eq!(
        policy.evaluate(1),
        AppUpdateStatus::UpdateRequired {
            grace_deadline: None,
        }
    );
}

#[test]
fn evaluate_update_required_with_past_deadline() {
    // Deadline in the past (year 2020)
    let past_deadline = 1_577_836_800_u64;
    let policy = VersionPolicy {
        min_version: 3,
        warn_version: 5,
        grace_deadline: Some(past_deadline),
    };
    assert_eq!(
        policy.evaluate(1),
        AppUpdateStatus::UpdateRequired {
            grace_deadline: None,
        }
    );
}

// ---------------------------------------------------------------------------
// VersionPolicy::is_none_policy
// ---------------------------------------------------------------------------

#[test]
fn is_none_policy_when_both_zero() {
    let policy = VersionPolicy {
        min_version: 0,
        warn_version: 0,
        grace_deadline: None,
    };
    assert!(policy.is_none_policy());
}

#[test]
fn is_not_none_policy_when_min_nonzero() {
    let policy = VersionPolicy {
        min_version: 1,
        warn_version: 0,
        grace_deadline: None,
    };
    assert!(!policy.is_none_policy());
}

#[test]
fn is_not_none_policy_when_warn_nonzero() {
    let policy = VersionPolicy {
        min_version: 0,
        warn_version: 1,
        grace_deadline: None,
    };
    assert!(!policy.is_none_policy());
}

// ---------------------------------------------------------------------------
// VersionPolicy::from_headers
// ---------------------------------------------------------------------------

#[test]
fn from_headers_parses_all_values() {
    let policy = VersionPolicy::from_headers(Some("2"), Some("4"), Some("1700000000"));
    assert_eq!(policy.min_version, 2);
    assert_eq!(policy.warn_version, 4);
    assert_eq!(policy.grace_deadline, Some(1_700_000_000));
}

#[test]
fn from_headers_handles_missing_headers() {
    let policy = VersionPolicy::from_headers(None, None, None);
    assert_eq!(policy.min_version, 0);
    assert_eq!(policy.warn_version, 0);
    assert_eq!(policy.grace_deadline, None);
    assert!(policy.is_none_policy());
}

#[test]
fn from_headers_handles_partial_headers() {
    let policy = VersionPolicy::from_headers(Some("3"), None, None);
    assert_eq!(policy.min_version, 3);
    assert_eq!(policy.warn_version, 0);
    assert_eq!(policy.grace_deadline, None);
}

#[test]
fn from_headers_handles_invalid_numbers_as_zero() {
    let policy = VersionPolicy::from_headers(Some("abc"), Some("xyz"), Some("not-a-number"));
    assert_eq!(policy.min_version, 0);
    assert_eq!(policy.warn_version, 0);
    assert_eq!(policy.grace_deadline, None);
}

// ---------------------------------------------------------------------------
// VersionPolicy::from_cdn_json
// ---------------------------------------------------------------------------

#[test]
fn from_cdn_json_parses_complete_json_with_unix_timestamp() {
    let json = r#"{"min_version": 2, "warn_version": 4, "grace_deadline": 1700000000}"#;
    let policy = VersionPolicy::from_cdn_json(json).expect("valid JSON");
    assert_eq!(policy.min_version, 2);
    assert_eq!(policy.warn_version, 4);
    assert_eq!(policy.grace_deadline, Some(1_700_000_000));
}

#[test]
fn from_cdn_json_handles_null_deadline() {
    let json = r#"{"min_version": 1, "warn_version": 3, "grace_deadline": null}"#;
    let policy = VersionPolicy::from_cdn_json(json).expect("valid JSON");
    assert_eq!(policy.min_version, 1);
    assert_eq!(policy.warn_version, 3);
    assert_eq!(policy.grace_deadline, None);
}

#[test]
fn from_cdn_json_handles_missing_deadline_field() {
    let json = r#"{"min_version": 1, "warn_version": 3}"#;
    let policy = VersionPolicy::from_cdn_json(json).expect("valid JSON");
    assert_eq!(policy.min_version, 1);
    assert_eq!(policy.warn_version, 3);
    assert_eq!(policy.grace_deadline, None);
}

#[test]
fn from_cdn_json_handles_iso8601_deadline() {
    // 2024-01-15T00:00:00Z = 1705276800
    let json = r#"{"min_version": 2, "warn_version": 4, "grace_deadline": "2024-01-15T00:00:00Z"}"#;
    let policy = VersionPolicy::from_cdn_json(json).expect("valid JSON");
    assert_eq!(policy.min_version, 2);
    assert_eq!(policy.warn_version, 4);
    assert_eq!(policy.grace_deadline, Some(1_705_276_800));
}

#[test]
fn from_cdn_json_rejects_invalid_json() {
    let result = VersionPolicy::from_cdn_json("not json at all");
    assert!(result.is_err());
}

#[test]
fn from_cdn_json_rejects_missing_required_fields() {
    let json = r#"{"min_version": 1}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
}

#[test]
fn from_cdn_json_iso8601_epoch_parses_to_zero() {
    let json = r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "1970-01-01T00:00:00Z"}"#;
    let policy = VersionPolicy::from_cdn_json(json).expect("valid JSON");
    assert_eq!(policy.grace_deadline, Some(0));
}

#[test]
fn from_cdn_json_rejects_non_utc_iso8601() {
    let json =
        r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "2024-01-15T00:00:00+01:00"}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Derive traits
// ---------------------------------------------------------------------------

#[test]
fn version_policy_is_clone_debug_eq() {
    let policy = VersionPolicy {
        min_version: 1,
        warn_version: 2,
        grace_deadline: None,
    };
    let cloned = policy.clone();
    assert_eq!(policy, cloned);
    // Debug is derivable — just ensure it doesn't panic
    let _debug = format!("{:?}", policy);
}

#[test]
fn app_update_status_is_clone_debug_eq() {
    let status = AppUpdateStatus::UpToDate;
    let cloned = status.clone();
    assert_eq!(status, cloned);
    let _debug = format!("{:?}", status);
}

// ---------------------------------------------------------------------------
// Truncation and validation regression tests
// ---------------------------------------------------------------------------

#[test]
fn from_cdn_json_rejects_min_version_exceeding_u16() {
    let json = r#"{"min_version": 70000, "warn_version": 1}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("out of u16 range"),
        "error should mention u16 range"
    );
}

#[test]
fn from_cdn_json_rejects_warn_version_exceeding_u16() {
    let json = r#"{"min_version": 1, "warn_version": 100000}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("out of u16 range"),
        "error should mention u16 range"
    );
}

#[test]
fn from_cdn_json_accepts_u16_max() {
    let json = r#"{"min_version": 65535, "warn_version": 65535}"#;
    let policy = VersionPolicy::from_cdn_json(json).expect("u16::MAX should be valid");
    assert_eq!(policy.min_version, 65535);
    assert_eq!(policy.warn_version, 65535);
}

#[test]
fn from_cdn_json_rejects_pre_1970_iso8601() {
    let json = r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "1969-12-31T23:59:59Z"}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("pre-1970"),
        "error should mention pre-1970"
    );
}

#[test]
fn from_cdn_json_rejects_bad_separators() {
    // Spaces instead of dashes/colons
    let json = r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "2024 01 15T00 00 00Z"}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
}

#[test]
fn from_cdn_json_rejects_hour_24() {
    let json = r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "2024-01-15T24:00:00Z"}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("invalid time"),
        "error should mention invalid time"
    );
}

#[test]
fn from_cdn_json_rejects_minute_60() {
    let json = r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "2024-01-15T00:60:00Z"}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
}

#[test]
fn from_cdn_json_accepts_leap_second() {
    // Second 60 is valid for leap seconds
    let json = r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "2024-06-30T23:59:60Z"}"#;
    let policy = VersionPolicy::from_cdn_json(json).expect("leap second should be valid");
    assert!(policy.grace_deadline.is_some());
}

#[test]
fn from_cdn_json_rejects_second_61() {
    let json = r#"{"min_version": 1, "warn_version": 2, "grace_deadline": "2024-01-15T00:00:61Z"}"#;
    let result = VersionPolicy::from_cdn_json(json);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Proptest: evaluate monotonicity (CC-04)
// ---------------------------------------------------------------------------

/// Helper to rank AppUpdateStatus (higher = "better" / less urgent).
fn status_rank(s: &AppUpdateStatus) -> u8 {
    match s {
        AppUpdateStatus::UpdateRequired { .. } => 0,
        AppUpdateStatus::UpdateAvailable => 1,
        AppUpdateStatus::UpToDate => 2,
    }
}

proptest! {
    /// For a fixed policy, incrementing the app version must never produce a
    /// "worse" status (monotonicity: version N+1 >= version N).
    #[test]
    fn evaluate_monotonicity(
        min_ver in 0u16..=1000,
        warn_ver in 0u16..=1000,
        app_ver in 0u16..=999,
    ) {
        let policy = VersionPolicy {
            min_version: min_ver,
            warn_version: warn_ver,
            grace_deadline: None,
        };
        let rank_n = status_rank(&policy.evaluate(app_ver));
        let rank_n1 = status_rank(&policy.evaluate(app_ver + 1));
        prop_assert!(
            rank_n1 >= rank_n,
            "version {} rank {} but version {} rank {} — monotonicity violated",
            app_ver, rank_n, app_ver + 1, rank_n1
        );
    }

    /// Generate valid ISO 8601 date strings and verify from_cdn_json doesn't panic.
    #[test]
    fn from_cdn_json_valid_iso_no_panic(
        year in 1970u32..2100,
        month in 1u32..=12,
        day in 1u32..=28, // stay safe with day range
        hour in 0u32..=23,
        minute in 0u32..=59,
        second in 0u32..=59,
    ) {
        let iso = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            year, month, day, hour, minute, second
        );
        let json = format!(
            r#"{{"min_version": 1, "warn_version": 2, "grace_deadline": "{}"}}"#,
            iso
        );
        // Must not panic; result should be Ok.
        let result = VersionPolicy::from_cdn_json(&json);
        prop_assert!(result.is_ok(), "valid ISO date should parse: {} -> {:?}", iso, result);
    }

    /// Random version values in u16 range should always parse from CDN JSON.
    #[test]
    fn from_cdn_json_u16_versions(
        min_ver in 0u64..=65535,
        warn_ver in 0u64..=65535,
    ) {
        let json = format!(
            r#"{{"min_version": {}, "warn_version": {}}}"#,
            min_ver, warn_ver
        );
        let result = VersionPolicy::from_cdn_json(&json);
        prop_assert!(result.is_ok(), "u16-range values should parse: {}", json);
        let policy = result.unwrap();
        prop_assert_eq!(policy.min_version, min_ver as u16);
        prop_assert_eq!(policy.warn_version, warn_ver as u16);
    }

    /// Values above u16::MAX must be rejected.
    #[test]
    fn from_cdn_json_rejects_above_u16(
        big_val in 65536u64..=1_000_000,
    ) {
        let json_min = format!(
            r#"{{"min_version": {}, "warn_version": 1}}"#,
            big_val
        );
        prop_assert!(VersionPolicy::from_cdn_json(&json_min).is_err());

        let json_warn = format!(
            r#"{{"min_version": 1, "warn_version": {}}}"#,
            big_val
        );
        prop_assert!(VersionPolicy::from_cdn_json(&json_warn).is_err());
    }
}
