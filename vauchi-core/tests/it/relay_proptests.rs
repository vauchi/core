// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Property-based tests for relay discovery (T1.5, T2.3).
//!
//! Tests:
//! - QR v3 roundtrip with arbitrary relay URLs
//! - Contact relay field roundtrip through storage
//! - Relay URL validation properties

use proptest::prelude::*;

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::multistage::qr_codec::{StageQr, format_ini2_qr_with_relay, parse_qr};
use vauchi_core::exchange::{ExchangeQR, X3DHKeyPair};
use vauchi_core::identity::Identity;
use vauchi_core::network::relay_url::validate_relay_url;
use vauchi_core::storage::Storage;

/// Strategy: generate valid https:// relay URLs with random subdomains.
fn valid_relay_url_strategy() -> impl Strategy<Value = String> {
    // Random subdomain (1-50 alphanum chars) + fixed public domain
    "[a-z0-9]{1,50}".prop_map(|sub| format!("https://{sub}.relay.example.com"))
}

// ── QR v3 roundtrip properties ───────────────────────────────────

proptest! {
// @internal
    #[test]
    fn qr_v3_roundtrip_preserves_relay_url(
        relay_url in valid_relay_url_strategy()
    ) {
        let identity = Identity::create("PropTest", 0);
        let ephemeral = X3DHKeyPair::generate();

        let qr = ExchangeQR::generate_with_relay(
            &identity,
            &ephemeral,
            Some(relay_url.clone()),
            0u64,
        );

        let data = qr.to_data_string();
        let parsed = ExchangeQR::from_data_string(&data).unwrap();

        prop_assert_eq!(parsed.relay_url().unwrap(), &relay_url);
        prop_assert!(parsed.verify_signature());
    }
}

// ── Contact relay storage roundtrip properties ───────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

// @internal
    #[test]
    fn contact_relay_url_roundtrips_through_storage(
        relay_url in valid_relay_url_strategy()
    ) {
        let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

        let public_key = [42u8; 32];
        let card = ContactCard::new("PropTest");
        let shared_key = SymmetricKey::generate();
        let mut contact = Contact::from_exchange(public_key, card, shared_key, 0);
        contact.set_relay_url(Some(relay_url.clone()));

        storage.contacts().save_contact(&contact).unwrap();
        let loaded = storage.contacts().load_contact(contact.id()).unwrap().unwrap();

        prop_assert_eq!(loaded.relay_url().unwrap(), &relay_url);
    }
}

// ── Multi-stage INIT QR roundtrip properties ────────────────────

proptest! {
// @internal
    #[test]
    fn multistage_init_qr_roundtrip_with_relay(
        relay_url in valid_relay_url_strategy(),
        display_name in "[A-Za-z0-9 ]{1,20}"
    ) {
        let session_id = [42u8; 16];
        let _pk = [1u8; 32];
        let eph = [2u8; 32];
        let ch = [3u8; 32];

        let qr = format_ini2_qr_with_relay(
            &session_id, &eph, &ch,
            &display_name,
            Some(&relay_url),
        );

        let parsed = parse_qr(&qr).unwrap();
        match parsed {
            StageQr::Init {
                session_id: sid,
                ephemeral: e,
                commitment_hash: c,
                display_name: name,
                relay_url: url,
            } => {
                prop_assert_eq!(sid, session_id);
                prop_assert_eq!(e, eph);
                prop_assert_eq!(c, ch);
                prop_assert_eq!(name, display_name);
                prop_assert_eq!(url.as_deref(), Some(relay_url.as_str()));
            }
            _ => prop_assert!(false, "expected Init variant"),
        }
    }

// @internal
    #[test]
    fn multistage_init_qr_roundtrip_without_relay(
        display_name in "[A-Za-z0-9 ]{1,20}"
    ) {
        let session_id = [43u8; 16];
        let _pk = [4u8; 32];
        let eph = [5u8; 32];
        let ch = [6u8; 32];

        let qr = format_ini2_qr_with_relay(
            &session_id, &eph, &ch,
            &display_name,
            None,
        );

        let parsed = parse_qr(&qr).unwrap();
        match parsed {
            StageQr::Init { relay_url, display_name: name, .. } => {
                prop_assert_eq!(name, display_name);
                prop_assert!(relay_url.is_none());
            }
            _ => prop_assert!(false, "expected Init variant"),
        }
    }
}

// ── Relay URL validation properties ──────────────────────────────

proptest! {
// @internal
    #[test]
    fn valid_wss_urls_pass_validation(
        relay_url in valid_relay_url_strategy()
    ) {
        prop_assert!(validate_relay_url(&relay_url).is_ok(), "valid URL rejected: {}", relay_url);
    }

// @internal
    #[test]
    fn non_https_schemes_fail_validation(
        scheme in "(http|ws|wss|ftp|file)",
        host in "[a-z]{3,10}\\.[a-z]{2,4}"
    ) {
        let url = format!("{scheme}://{host}");
        prop_assert!(validate_relay_url(&url).is_err(), "non-https URL should be rejected: {}", url);
    }
}
