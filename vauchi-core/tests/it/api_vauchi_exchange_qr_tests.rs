// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `Vauchi::generate_exchange_qr` and the `ExchangeQrData`
//! expiration helpers in `vauchi-core/src/api/vauchi/exchange.rs`.

use std::time::{SystemTime, UNIX_EPOCH};

use vauchi_core::Vauchi;
use vauchi_core::VauchiError;
use vauchi_core::api::ExchangeQrData;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ============================================================
// generate_exchange_qr
// ============================================================

// @internal
#[test]
fn generate_exchange_qr_returns_non_empty_qr() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let qr = wb.generate_exchange_qr().unwrap();

    assert!(!qr.data.is_empty(), "QR data must be non-empty");
    assert_eq!(qr.expires_in_secs, 300, "5-minute QR expiry per protocol");
    let now = now_secs();
    assert!(
        qr.generated_at <= now && qr.generated_at + 5 >= now,
        "generated_at must be near current Unix time (got {}, now {})",
        qr.generated_at,
        now
    );
}

// @internal
#[test]
fn generate_exchange_qr_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();
    let result = wb.generate_exchange_qr();
    assert!(matches!(result, Err(VauchiError::IdentityNotInitialized)));
}

// @internal
#[test]
fn generate_exchange_qr_produces_distinct_payloads_across_calls() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let q1 = wb.generate_exchange_qr().unwrap();
    let q2 = wb.generate_exchange_qr().unwrap();

    // Each call mints a new ephemeral session — distinct QR strings.
    assert_ne!(
        q1.data, q2.data,
        "consecutive QRs must differ (fresh ephemeral material)"
    );
}

// ============================================================
// ExchangeQrData::remaining_secs / is_expired
// ============================================================

// @internal
#[test]
fn remaining_secs_decreases_as_time_passes() {
    let qr = ExchangeQrData {
        data: "DUMMY".into(),
        generated_at: now_secs(),
        expires_in_secs: 300,
    };
    let remaining = qr.remaining_secs();
    assert!(
        remaining > 0 && remaining <= 300,
        "fresh QR remaining must be in (0, 300]; got {remaining}"
    );
}

// @internal
#[test]
fn remaining_secs_is_zero_after_expiry() {
    let qr = ExchangeQrData {
        data: "DUMMY".into(),
        generated_at: now_secs().saturating_sub(400),
        expires_in_secs: 300,
    };
    assert_eq!(
        qr.remaining_secs(),
        0,
        "QR expired 100s ago: remaining must saturate to 0"
    );
}

// @internal
#[test]
fn remaining_secs_is_zero_at_exact_expiry_boundary() {
    let qr = ExchangeQrData {
        data: "DUMMY".into(),
        generated_at: now_secs().saturating_sub(300),
        expires_in_secs: 300,
    };
    assert_eq!(
        qr.remaining_secs(),
        0,
        "at expiry boundary remaining must be 0"
    );
}

// @internal
#[test]
fn is_expired_true_after_window() {
    let qr = ExchangeQrData {
        data: "DUMMY".into(),
        generated_at: now_secs().saturating_sub(301),
        expires_in_secs: 300,
    };
    assert!(qr.is_expired());
}

// @internal
#[test]
fn is_expired_false_within_window() {
    let qr = ExchangeQrData {
        data: "DUMMY".into(),
        generated_at: now_secs(),
        expires_in_secs: 300,
    };
    assert!(!qr.is_expired());
}
