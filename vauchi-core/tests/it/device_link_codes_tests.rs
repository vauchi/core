// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link codes and QR lifetime: the link QR lives exactly 5 minutes
//! (ADR-035), numeric and confirmation codes are `ddd-ddd` drawn across the
//! full range, and request bytes from the new device are length-checked
//! before use (vauchi/private#522).

use std::collections::HashSet;
use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkQR, DeviceLinkRequest, DeviceLinkResponder, DeviceLinkResponse,
    ExchangeError, generate_numeric_code,
};
use vauchi_core::identity::{DeviceRegistry, Identity};

const T0: u64 = 1_700_000_000;

fn is_ddd_ddd(code: &str) -> bool {
    code.len() == 7
        && code.as_bytes()[3] == b'-'
        && code
            .chars()
            .enumerate()
            .all(|(i, c)| i == 3 || c.is_ascii_digit())
}

/// Codes drawn uniformly from 000-000..=999-999 almost surely leave the
/// 000-xxx..004-xxx corner within a few dozen draws.
fn spread_across_the_range(codes: &[String]) -> bool {
    codes.iter().any(|c| &c[..3] > "004")
}

// @internal
#[test]
fn a_link_qr_expires_exactly_five_minutes_after_it_was_made() {
    let qr = DeviceLinkQR::generate_with_timestamp(&Identity::create("Alice", 0), T0);

    assert_eq!(qr.timestamp(), T0);
    assert_eq!(qr.expires_at(), T0 + 300);
    assert!(!qr.is_expired(T0 + 300));
    assert!(qr.is_expired(T0 + 301));
}

// @internal
#[test]
fn numeric_codes_are_ddd_ddd_across_the_full_range() {
    let codes: Vec<String> = (0..200).map(|_| generate_numeric_code()).collect();

    assert!(codes.iter().all(|c| is_ddd_ddd(c)), "{codes:?}");
    assert!(spread_across_the_range(&codes), "{codes:?}");
    assert!(codes.iter().collect::<HashSet<_>>().len() > 150);
}

// @internal
#[test]
fn confirmation_codes_are_ddd_ddd_across_the_full_range() {
    let identity = Identity::create("Alice", 0);
    let master_seed = [0x42u8; 32];
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&master_seed),
        identity.signing_keypair(),
    );
    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry, T0);
    let codes: Vec<String> = (0..40)
        .map(|_| {
            let qr = DeviceLinkQR::from_data_string(&initiator.qr().to_data_string()).unwrap();
            let mut responder = DeviceLinkResponder::from_qr(qr, "Phone".into(), T0).unwrap();
            let request = responder.create_request(T0).unwrap();
            initiator
                .prepare_confirmation(&request)
                .unwrap()
                .0
                .confirmation_code
        })
        .collect();

    assert!(codes.iter().all(|c| is_ddd_ddd(c)), "{codes:?}");
    assert!(spread_across_the_range(&codes), "{codes:?}");
}

// @internal
#[test]
fn a_link_request_needs_its_full_declared_length() {
    let empty_name = DeviceLinkRequest::new(String::new(), T0).to_bytes();
    assert_eq!(empty_name.len(), 4 + 32 + 8);
    assert!(DeviceLinkRequest::from_bytes(&empty_name).is_ok());

    let named = DeviceLinkRequest::new("Phone".into(), T0).to_bytes();
    assert_eq!(
        DeviceLinkRequest::from_bytes(&named).unwrap().device_name,
        "Phone"
    );
    for len in [0, 30, 43, named.len() - 1] {
        assert!(
            matches!(
                DeviceLinkRequest::from_bytes(&named[..len]),
                Err(ExchangeError::InvalidQRFormat)
            ),
            "{len} bytes"
        );
    }
}

// @internal
#[test]
fn a_link_response_keeps_the_device_index_it_was_given() {
    let identity = Identity::create("Alice", 0);
    let registry = DeviceRegistry::new(
        identity.device_info().to_registered(&[1u8; 32]),
        identity.signing_keypair(),
    );

    let response = DeviceLinkResponse::new([1u8; 32], "Alice".into(), 3, registry);

    assert_eq!(response.device_index(), 3);
}
