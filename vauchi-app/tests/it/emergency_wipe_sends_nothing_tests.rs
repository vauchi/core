// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Emergency Wipe touches this device only and sends nothing: no relay
//! purge, no revocation, no notice to contacts (ADR-033, #910). A message
//! sent at the moment of a coerced wipe would tell whoever watches the
//! network that it happened; only Leave Vauchi tells contacts
//! (vauchi/private#1329).

#![cfg(all(feature = "network-http", feature = "testing"))]

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
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
    application: MockRelay,
    _dir: tempfile::TempDir,
}

/// Alice with one exchanged contact, so a revocation would have somewhere
/// to go, her requests passing through a gateway that records them.
fn alice_with_an_exchanged_contact() -> World {
    let gateway = MockRelay::start();
    gateway.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let application = MockRelay::start();
    let dir = tempfile::tempdir().expect("tempdir");
    let config = VauchiConfig::with_profile_directory(dir.path())
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(gateway.url());
    let mut vauchi = Vauchi::new(config).expect("vauchi");
    vauchi.create_identity("Alice").expect("identity");
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(gateway.ohttp_key_config()).expect("key"));
    let bob = Contact::from_exchange(
        [0xBB; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    vauchi.update_contact(&bob).expect("contact");
    World {
        engine: AppEngine::new(vauchi),
        gateway,
        application,
        _dir: dir,
    }
}

fn confirm_the_emergency_wipe(engine: &mut AppEngine) -> ActionResult {
    engine.navigate_to(AppScreen::Settings);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "WIPE".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_shred".into(),
    })
}

// @internal
#[test]
fn the_emergency_wipe_destroys_the_identity_and_sends_nothing() {
    let mut world = alice_with_an_exchanged_contact();

    let result = confirm_the_emergency_wipe(&mut world.engine);

    assert!(
        matches!(result, ActionResult::WipeComplete),
        "expected the wipe to complete, got {result:?}"
    );
    assert!(
        world.engine.vauchi().identity().is_none(),
        "identity survived"
    );
    assert_eq!(world.gateway.ohttp_actions(), Vec::<String>::new());
    assert_eq!(
        world.gateway.received().len(),
        0,
        "a request reached the OHTTP gateway"
    );
    assert_eq!(
        world.application.received().len(),
        0,
        "a request reached the relay"
    );
}
