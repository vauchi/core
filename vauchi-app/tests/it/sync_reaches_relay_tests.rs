// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every way the engine is asked to sync puts a request on the relay
//! (vauchi/private#1297, ADR-082 D-4). With the sync call removed from the
//! engine's sync handler, 3,271 app tests and e2e's 279 CLI tests passed:
//! they asserted the screen and the commands, never the request.

#![cfg(all(feature = "network-http", feature = "testing"))]

use vauchi_app::ui::{AppEngine, UserAction, WorkflowEngine};
use vauchi_core::ImportSource;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};

struct World {
    engine: AppEngine,
    gateway: MockRelay,
    _application: MockRelay,
    _dir: tempfile::TempDir,
}

/// Alice with one contact and a due sync, her requests going through an
/// OHTTP gateway that records what it decapsulates.
fn alice_with_a_contact() -> World {
    let gateway = MockRelay::start();
    gateway.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let application = MockRelay::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(gateway.url());
    let mut vauchi = Vauchi::new(config).expect("vauchi");
    vauchi.create_identity("Alice").expect("identity");
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(gateway.ohttp_key_config()).expect("key"));
    vauchi
        .add_contact(Contact::from_import(
            "contact-Bob".into(),
            ContactCard::new("Bob"),
            ImportSource::VcardFile,
            None,
            0,
        ))
        .expect("contact");
    World {
        engine: AppEngine::new(vauchi),
        gateway,
        _application: application,
        _dir: dir,
    }
}

/// How many day tokens one sync fetches is Core's catch-up plan, not the
/// engine's contract; that it fetches at all is.
fn assert_fetched(gateway: &MockRelay) {
    let actions = gateway.ohttp_actions();
    assert!(
        actions.iter().any(|action| action == "fetch"),
        "no fetch reached the relay: {actions:?}"
    );
}

// @internal
#[test]
fn the_sync_chip_puts_a_request_on_the_relay() {
    let mut world = alice_with_a_contact();
    let before = world.gateway.ohttp_actions();

    let _ = world.engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });

    assert_eq!(before, Vec::<String>::new());
    assert_fetched(&world.gateway);
}

// @internal
#[test]
fn coming_to_the_foreground_puts_a_request_on_the_relay() {
    let mut world = alice_with_a_contact();

    let _ = world.engine.handle_action(UserAction::AppForegrounded);

    assert_fetched(&world.gateway);
}

// @internal
#[test]
fn a_wakeup_with_a_due_sync_puts_a_request_on_the_relay() {
    let mut world = alice_with_a_contact();

    world.engine.on_wakeup();

    assert_fetched(&world.gateway);
}

// @internal
#[test]
fn a_wakeup_inside_the_sync_gate_puts_nothing_on_the_relay() {
    use std::time::{Duration, Instant};
    let mut world = alice_with_a_contact();
    world
        .engine
        .vauchi_mut()
        .set_next_sync_allowed(Instant::now() + Duration::from_secs(3600));

    world.engine.on_wakeup();

    assert_eq!(world.gateway.ohttp_actions(), Vec::<String>::new());
}
