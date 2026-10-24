// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A QR's timestamp is chosen by whoever signs it, so it may sit at the
//! top of `u64`; the expiry and drift checks must refuse it, not overflow.
//! Release builds keep overflow checks and abort on panic
//! (vauchi/private#795).

use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::{
    DeviceLinkQR, DeviceLinkResponder, ExchangeError, ExchangeQR, check_clock_drift,
};
use vauchi_core::identity::Identity;

const NOW: u64 = 1_700_000_000;
const FAR_FUTURE: u64 = u64::MAX - 10;

fn alice() -> Identity {
    Identity::from_device_link([0x11; 32], "Alice".into(), 0, "Primary Device".into(), 0)
}

fn scanned_link_qr(timestamp: u64) -> DeviceLinkQR {
    let qr = DeviceLinkQR::generate_with_timestamp(&alice(), timestamp);
    DeviceLinkQR::from_data_string(&qr.to_data_string()).unwrap()
}

// @internal
#[test]
fn a_link_qr_timestamped_at_the_top_of_u64_counts_as_expired() {
    let qr = scanned_link_qr(FAR_FUTURE);

    assert!(qr.is_expired(NOW));
    assert_eq!(qr.expires_at(), u64::MAX);
}

// @internal
#[test]
fn joining_from_a_link_qr_timestamped_at_the_top_of_u64_is_refused() {
    let result = DeviceLinkResponder::from_qr(scanned_link_qr(FAR_FUTURE), "Phone".into(), NOW);

    assert!(matches!(result, Err(ExchangeError::TokenExpired)));
}

// @internal
#[test]
fn a_fresh_link_qr_is_still_joinable() {
    let qr = scanned_link_qr(NOW);

    assert!(!qr.is_expired(NOW + 300));
    assert!(qr.is_expired(NOW + 301));
    assert_eq!(qr.expires_at(), NOW + 300);
}

// @internal
#[test]
fn an_exchange_qr_timestamped_at_the_top_of_u64_counts_as_expired() {
    let qr = ExchangeQR::generate_with_timestamp(&alice(), &X3DHKeyPair::generate(), FAR_FUTURE);

    assert!(qr.is_expired(NOW));
}

// @internal
#[test]
fn clock_drift_against_a_timestamp_past_i64_is_reported_not_overflowed() {
    assert!(matches!(
        check_clock_drift(FAR_FUTURE, NOW),
        Err(ExchangeError::ClockDrift(i64::MIN))
    ));
    assert!(matches!(
        check_clock_drift(1 << 63, NOW),
        Err(ExchangeError::ClockDrift(drift)) if drift == i64::MIN + 1_700_000_000
    ));
    assert!(matches!(check_clock_drift(NOW - 30, NOW), Ok(())));
    assert!(matches!(
        check_clock_drift(NOW - 31, NOW),
        Err(ExchangeError::ClockDrift(31))
    ));
}
