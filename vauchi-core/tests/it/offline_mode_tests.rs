// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(all(feature = "network-http", feature = "testing"))]

use vauchi_core::api::{Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::{
    HttpTransportAdapter, MessageEnvelope, MessagePayload, PROTOCOL_VERSION, RegisterMailbox,
    Transport,
};

use crate::common::mock_relay::MockRelay;

// @scenario: sync_updates :: Offline mode blocks every relay path
#[test]
fn offline_mode_blocks_bootstrap_sync_and_action_requests() {
    let application = MockRelay::start();
    let outer = MockRelay::start();
    let dir = tempfile::tempdir().expect("temp dir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    let mut vauchi = Vauchi::new(config).expect("create Vauchi");
    vauchi.create_identity("Offline").expect("identity");
    vauchi.config_mut().offline = true;

    let error = vauchi.connect().expect_err("offline connect must fail");
    assert!(error.to_string().contains("offline mode"), "{error}");
    assert!(matches!(
        vauchi.sync().expect("offline sync"),
        VauchiSyncOutcome::NotConnected
    ));
    assert!(matches!(
        vauchi
            .periodic_sync_tick()
            .expect("offline background sync"),
        VauchiSyncOutcome::NotConnected
    ));

    let application_url = vauchi.config().relay.server_url.clone();
    let timeout = vauchi.config().relay.connect_timeout_ms;
    let mut adapter =
        HttpTransportAdapter::new(vauchi.build_action_relay_transport(&application_url, timeout));
    adapter
        .send(&MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "register".to_string().into(),
            timestamp: 0,
            payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                tokens: vec!["t".repeat(64)],
            }),
        })
        .expect("mailbox registration is local-only");
    adapter
        .receive()
        .expect_err("offline action transport must fail closed");
    assert!(
        application.received().is_empty(),
        "{:?}",
        application.received()
    );
    assert!(outer.received().is_empty(), "{:?}", outer.received());
}

// @internal
#[test]
fn offline_mode_persists_and_seeds_config_on_reopen() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = SymmetricKey::generate();
    let path = dir.path().join("vauchi.db");
    {
        let config = VauchiConfig::with_storage_path(&path).with_storage_key(key.clone());
        let vauchi = Vauchi::new(config).expect("create Vauchi");
        assert!(!vauchi.config().offline);
        let mut flags = vauchi.load_settings_flags().expect("flags");
        flags.offline = true;
        vauchi.save_settings_flags(&flags).expect("save flags");
    }
    {
        let config = VauchiConfig::with_storage_path(&path).with_storage_key(key);
        let vauchi = Vauchi::new(config).expect("reopen Vauchi");
        assert!(vauchi.config().offline);
        assert!(vauchi.load_settings_flags().expect("flags").offline);
    }
}
