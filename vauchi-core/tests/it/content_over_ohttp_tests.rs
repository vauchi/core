// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content files travel through OHTTP: the relay serves them as the
//! `content` action, so the server that learns which language was asked for
//! cannot tie it to an IP address (vauchi/private#733 D8, D11).

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;

use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};

fn client(outer: &MockRelay, application: &MockRelay) -> (Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Content").unwrap();
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).unwrap());
    (vauchi, dir)
}

// @internal
#[test]
fn a_content_file_is_fetched_as_an_ohttp_action() {
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    outer.set_gateway_content(vec![("locales/pt.json", b"{\"pt\":1}".to_vec())]);
    let application = MockRelay::start();
    let (vauchi, _dir) = client(&outer, &application);

    let body = vauchi
        .build_action_relay_transport(&application.url(), 5_000)
        .fetch_content("locales/pt.json")
        .unwrap();

    assert_eq!(body, b"{\"pt\":1}");
    assert_eq!(outer.ohttp_actions(), ["content"]);
}

// @internal
#[test]
fn a_content_error_from_the_relay_is_an_error() {
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let application = MockRelay::start();
    let (vauchi, _dir) = client(&outer, &application);

    let result = vauchi
        .build_action_relay_transport(&application.url(), 5_000)
        .fetch_content("locales/pt.json");

    assert!(result.is_err(), "{result:?}");
}

/// A locale file is larger than the OHTTP relay passes back; asked for
/// compressed, it arrives (vauchi/private#733 D14).
// @internal
#[test]
fn a_locale_larger_than_the_ohttp_relay_passes_arrives_compressed() {
    let locale = br#"{"pt.greeting":"Bom dia, tudo bem?"},"#.repeat(10_000);
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    outer.set_gateway_content(vec![("locales/pt.json", locale.clone())]);
    outer.answer_content_compressed();
    outer.limit_content_answers(128 * 1024);
    let application = MockRelay::start();
    let (vauchi, _dir) = client(&outer, &application);

    let body = vauchi
        .build_action_relay_transport(&application.url(), 5_000)
        .fetch_content("locales/pt.json")
        .unwrap();

    assert_eq!(body, locale);
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// What a relay answers is untrusted: a small answer that inflates without
/// bound, bytes that are not gzip, or an encoding Core does not know are
/// errors, never content.
// @internal
#[test]
fn an_encoded_answer_core_cannot_trust_is_an_error() {
    let six_mebibytes_of_zeros = gzip(&vec![0_u8; 6 * 1024 * 1024]);
    for (encoding, bytes) in [
        ("gzip", six_mebibytes_of_zeros),
        ("gzip", b"not gzip at all".to_vec()),
        ("gzip", Vec::new()),
        ("zstd", gzip(b"{\"pt\":1}")),
    ] {
        let outer = MockRelay::start();
        outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
        outer.set_gateway_encoded_content("locales/pt.json", encoding, bytes.clone());
        let application = MockRelay::start();
        let (vauchi, _dir) = client(&outer, &application);

        let result = vauchi
            .build_action_relay_transport(&application.url(), 5_000)
            .fetch_content("locales/pt.json");

        assert!(
            result.is_err(),
            "{encoding}, {} bytes: accepted",
            bytes.len()
        );
    }
}

// @internal
#[test]
fn content_is_never_fetched_without_ohttp() {
    let application = MockRelay::start();
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url());
    let vauchi = Vauchi::new(config).unwrap();

    let result = vauchi
        .build_action_relay_transport(&application.url(), 5_000)
        .fetch_content("locales/pt.json");

    assert!(result.is_err(), "{result:?}");
}
