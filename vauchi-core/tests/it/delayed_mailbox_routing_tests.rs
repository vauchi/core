// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Public sync over real OHTTP with constructed exchanged peers and an
//! in-memory token-filtered gateway. No native exchange or key bootstrap.

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::common::linked_peer::link_contacts;
use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{DAY, NOW, SignedGateway};
use vauchi_core::api::{Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::clock::FakeClock;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;
use vauchi_core::rng::OsSecureRng;
use vauchi_core::sleeper::FakeSleeper;
use vauchi_protocol::ohttp_key::window_of;

fn client(
    outer: &MockRelay,
    application: &MockRelay,
    clock: Arc<FakeClock>,
    name: &str,
) -> (Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    config.relay.ohttp_delayed = true;
    let mut client = Vauchi::new_with(config, clock, OsSecureRng::shared(), None)
        .unwrap()
        .with_sleeper(Arc::new(FakeSleeper::new()));
    client.create_identity(name).unwrap();
    client.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).unwrap());
    (client, dir)
}

fn sync(client: &mut Vauchi) -> VauchiSyncOutcome {
    client.set_next_sync_allowed(Instant::now());
    let result = client.sync().unwrap();
    assert!(
        matches!(&result, VauchiSyncOutcome::Ok { errors, .. } if errors.is_empty()),
        "{result:?}"
    );
    result
}

// @internal: Delayed fetch crosses midnight while its previous-day mailbox leaves the recent window.
#[test]
fn a_previous_day_delta_collected_after_midnight_still_applies() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW - DAY)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x61), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let (mut alice, _alice_dir) = client(&outer, &application, clock.clone(), "Alice");
    let (mut bob, _bob_dir) = client(&outer, &application, clock.clone(), "Bob");
    let linked = link_contacts(&alice, &bob, "Bob");
    let mut updated = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
    let field_id = field.id().to_string();
    updated.add_field(field).unwrap();
    bob.update_own_card(&updated).unwrap();
    bob.set_contact_visibility_override(&linked.host_contact_id, &field_id, true)
        .unwrap();
    assert!(
        bob.get_effective_field_visibility(&linked.host_contact_id, &field_id)
            .unwrap()
    );
    let sent = sync(&mut bob);
    assert!(
        matches!(&sent, VauchiSyncOutcome::Ok { sent, .. } if *sent > 0),
        "{sent:?}"
    );
    assert_eq!(
        bob.storage()
            .pending()
            .get_all_pending_updates()
            .unwrap()
            .len(),
        0
    );

    // The envelope is already a day old. This fetch and its collection are
    // only two minutes apart, inside the real delayed-transport window.
    clock.advance(Duration::from_secs(2 * DAY - 120));
    let posted = sync(&mut alice);
    eprintln!("posted: {posted:?}");
    assert_eq!(
        alice
            .get_contact(&linked.peer_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .len(),
        0
    );
    clock.advance(Duration::from_secs(120));
    let collected = sync(&mut alice);
    eprintln!("collected: {collected:?}");
    assert!(
        matches!(&collected, VauchiSyncOutcome::Ok { unresolved: 0, .. }),
        "{collected:?}"
    );
    let saved = alice.get_contact(&linked.peer_contact_id).unwrap().unwrap();
    assert_eq!(
        saved
            .card()
            .fields()
            .iter()
            .map(|field| field.value())
            .collect::<Vec<_>>(),
        vec!["bob@example.invalid"]
    );
}
