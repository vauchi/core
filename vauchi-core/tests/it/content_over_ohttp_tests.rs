// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
