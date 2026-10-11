// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! An exchange QR carries `https://vauchi.app/x#q=<payload>`, so a phone
//! camera without the app lands on the vauchi.app/x page instead of
//! showing raw base64 (vauchi/private#277). The payload rides in the URL
//! fragment, which browsers never send to the server. Scanners accept
//! both that form and the raw payload older apps display.

use proptest::prelude::*;
use vauchi_core::exchange::oob_bootstrap::OobBootstrapQr;
use vauchi_core::exchange::{ExchangeError, ExchangeQR, X3DHKeyPair};
use vauchi_core::identity::Identity;

const QR_URL_PREFIX: &str = "https://vauchi.app/x#q=";
const NOW: u64 = 1_000;

fn glance_qr() -> (Identity, OobBootstrapQr) {
    let identity = Identity::create("Displayer", 0);
    let qr = OobBootstrapQr::generate(&identity, &X3DHKeyPair::generate(), NOW);
    (identity, qr)
}

fn exchange_qr() -> (Identity, ExchangeQR) {
    let identity = Identity::create("Zoë", 0);
    let qr = ExchangeQR::generate(&identity, &X3DHKeyPair::generate(), NOW);
    (identity, qr)
}

fn url_payload(url: &str) -> &str {
    url.strip_prefix(QR_URL_PREFIX)
        .unwrap_or_else(|| panic!("QR URL must start with {QR_URL_PREFIX}, got {url}"))
}

// @internal
#[test]
fn glance_qr_url_lands_on_vauchi_app_and_scans_back() {
    let (identity, qr) = glance_qr();

    let url = qr.to_qr_url();
    let payload = url_payload(&url);

    assert!(
        payload
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "the fragment payload is unpadded base64url, got {payload}"
    );
    let scanned =
        OobBootstrapQr::verified_from_data_string(&url, NOW).expect("the URL form must scan");
    assert_eq!(scanned.identity_key(), identity.signing_public_key());
    assert_eq!(scanned.oob_nonce(), qr.oob_nonce());
}

// @internal
#[test]
fn glance_qr_raw_form_from_older_apps_still_scans() {
    let (identity, qr) = glance_qr();

    let scanned = OobBootstrapQr::verified_from_data_string(&qr.to_data_string(), NOW)
        .expect("the raw base64 form must keep scanning");

    assert_eq!(scanned.identity_key(), identity.signing_public_key());
}

// @internal
#[test]
fn exchange_qr_url_lands_on_vauchi_app_and_scans_back() {
    let (identity, qr) = exchange_qr();

    let url = qr.to_qr_url();
    url_payload(&url);

    let scanned = ExchangeQR::from_data_string(&url).expect("the URL form must scan");
    assert_eq!(scanned.public_key(), identity.signing_public_key());
    assert_eq!(scanned.display_name(), "Zoë");
    assert!(scanned.verify_signature(), "the signature must survive");
}

// @internal
#[test]
fn exchange_qr_raw_form_from_older_apps_still_scans() {
    let (_, qr) = exchange_qr();

    let scanned = ExchangeQR::from_data_string(&qr.to_data_string())
        .expect("the raw base64 form must keep scanning");

    assert_eq!(scanned.display_name(), "Zoë");
}

// @internal
#[test]
fn qr_urls_off_the_landing_page_are_rejected() {
    let (_, glance) = glance_qr();
    let (_, exchange) = exchange_qr();
    let glance_payload = url_payload(&glance.to_qr_url()).to_owned();
    let exchange_payload = url_payload(&exchange.to_qr_url()).to_owned();

    for (payload, is_glance) in [(glance_payload, true), (exchange_payload, false)] {
        let hostile = [
            format!("https://evil.example/x#q={payload}"),
            format!("http://vauchi.app/x#q={payload}"),
            format!("https://vauchi.app/y#q={payload}"),
            format!("https://vauchi.app/x?q={payload}"),
            format!("https://vauchi.app/x#pk={payload}"),
            format!("https://vauchi.app/x#q={payload}&q={payload}"),
            format!("https://vauchi.app/x#q={payload}#"),
            format!("https://vauchi.app/x#q={}", payload.replace('-', "+")),
            "https://vauchi.app/x#q=".to_owned(),
            format!("https://vauchi.app/x#q={}", "A".repeat(4096)),
        ];
        for url in hostile {
            let result = if is_glance {
                OobBootstrapQr::verified_from_data_string(&url, NOW).map(|_| ())
            } else {
                ExchangeQR::from_data_string(&url).map(|_| ())
            };
            assert!(
                matches!(result, Err(ExchangeError::InvalidQRFormat)),
                "{url} must be rejected as an invalid QR, got {result:?}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // @internal
    #[test]
    fn both_qr_forms_scan_to_the_same_glance_payload(timestamp in 0u64..4_000_000_000) {
        let identity = Identity::create("Displayer", 0);
        let qr = OobBootstrapQr::generate(&identity, &X3DHKeyPair::generate(), timestamp);

        let from_url = OobBootstrapQr::from_data_string(&qr.to_qr_url()).unwrap();
        let from_raw = OobBootstrapQr::from_data_string(&qr.to_data_string()).unwrap();

        prop_assert_eq!(from_url.to_bytes(), qr.to_bytes());
        prop_assert_eq!(from_raw.to_bytes(), qr.to_bytes());
    }

    // @internal
    #[test]
    fn arbitrary_fragments_never_panic_the_scanner(fragment in ".{0,400}") {
        let url = format!("{QR_URL_PREFIX}{fragment}");
        let _ = OobBootstrapQr::from_data_string(&url);
        let _ = ExchangeQR::from_data_string(&url);
    }
}
