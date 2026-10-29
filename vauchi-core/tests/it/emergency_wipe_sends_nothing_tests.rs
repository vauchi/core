// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An emergency wipe concerns this device only and sends nothing
//! (vauchi/private#910, ADR-033 amendment 2026-10-29). The identity's other
//! devices keep working, so its contacts must not be told it is gone and
//! the relay must not purge it; a message at the moment of a coerced wipe
//! would also tell whoever watches the network that it happened.

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::network::OhttpClient;
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};

// @scenario: privacy_compliance :: Panic shred destroys this device's data and tells no one
// @scenario: emergency_shred :: Panic shred destroys this device's data and sends nothing
#[test]
fn an_emergency_wipe_with_contacts_sends_nothing_to_either_hop() {
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let application = MockRelay::start();
    let dir = tempfile::tempdir().expect("temp dir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    let mut vauchi = Vauchi::new(config).expect("create Vauchi");
    vauchi.create_identity("Me").expect("identity");
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).expect("key"));
    let bob = Identity::create("Bob", 0);
    vauchi
        .storage()
        .contacts()
        .save_contact(&Contact::from_exchange(
            *bob.signing_public_key(),
            ContactCard::new("Bob"),
            SymmetricKey::generate(),
            0,
        ))
        .expect("contact");
    let before = (outer.received().len(), application.received().len());

    vauchi.perform_emergency_wipe(true).expect("wipe");

    let sent_to_outer: Vec<String> = outer.received()[before.0..]
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect();
    let sent_to_application: Vec<String> = application.received()[before.1..]
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect();
    assert_eq!(sent_to_outer, Vec::<String>::new());
    assert_eq!(sent_to_application, Vec::<String>::new());
    assert!(
        vauchi.identity().is_none(),
        "the wipe still removes the identity"
    );
}
