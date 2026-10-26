// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Catch-up through the public sync API against the faithful mailbox
//! fixture (vauchi/private#691): a client offline for 100 days receives
//! what the relay kept, in bounded steps, and its receive checkpoint
//! reaches today.

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
use vauchi_core::network::mailbox_token::{
    CATCH_UP_DAYS_PER_CYCLE, RETAINED_DAYS, compute_mailbox_token, current_day_epoch, token_hex,
};
use vauchi_core::network::{HttpTransport, HttpTransportConfig};
use vauchi_core::rng::OsSecureRng;
use vauchi_core::sleeper::FakeSleeper;
use vauchi_core::storage::ReceiveScope;
use vauchi_protocol::ohttp_key::window_of;

const OFFLINE_DAYS: u64 = 100;

fn client(
    outer: &MockRelay,
    application: &MockRelay,
    clock: Arc<FakeClock>,
    name: &str,
) -> (Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
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

fn checkpoint(client: &Vauchi, application: &MockRelay) -> Option<u64> {
    let identity = client.identity().unwrap();
    let scope = ReceiveScope::new(
        &application.url(),
        identity.signing_public_key(),
        identity.device_id(),
    );
    client
        .storage()
        .sync()
        .load_receive_checkpoint(&scope)
        .unwrap()
}

// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_client_offline_100_days_catches_up_in_bounded_steps() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x63), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let (mut alice, _alice_dir) = client(&outer, &application, clock.clone(), "Alice");
    let (mut bob, _bob_dir) = client(&outer, &application, clock.clone(), "Bob");
    let linked = link_contacts(&alice, &bob, "Bob");
    let send_day = current_day_epoch(NOW);

    // Bob shares a field with Alice while she is away.
    let mut updated = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
    let field_id = field.id().to_string();
    updated.add_field(field).unwrap();
    bob.update_own_card(&updated).unwrap();
    bob.set_contact_visibility_override(&linked.host_contact_id, &field_id, true)
        .unwrap();
    let sent = sync(&mut bob);
    assert!(
        matches!(&sent, VauchiSyncOutcome::Ok { sent, .. } if *sent > 0),
        "{sent:?}"
    );

    clock.advance(Duration::from_secs(OFFLINE_DAYS * DAY));
    let today = send_day + OFFLINE_DAYS;
    assert_eq!(checkpoint(&alice, &application), None);

    // Each sync takes a bounded chunk of the retained interval, oldest
    // first, and records how far it read; the update arrives in the cycle
    // whose chunk covers the day it was sent.
    let horizon = today - RETAINED_DAYS;
    // The first chunk covers CATCH_UP_DAYS_PER_CYCLE days from the horizon;
    // every later chunk re-reads one day for rotation skew, so it advances
    // the checkpoint by two days less than it covers.
    let first_end = horizon + CATCH_UP_DAYS_PER_CYCLE - 1;
    let step = CATCH_UP_DAYS_PER_CYCLE - 2;
    let cycles_until_sent = if send_day <= first_end {
        1
    } else {
        1 + (send_day - first_end).div_ceil(step)
    };
    let mut cycles = 0;
    let fields = |alice: &Vauchi| {
        alice
            .get_contact(&linked.peer_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .iter()
            .map(|field| field.value().to_string())
            .collect::<Vec<_>>()
    };
    while fields(&alice).is_empty() {
        cycles += 1;
        assert!(
            cycles <= cycles_until_sent,
            "no update after {cycles} syncs"
        );
        sync(&mut alice);
        let expected_checkpoint = (first_end + (cycles - 1) * step).min(today);
        assert_eq!(
            checkpoint(&alice, &application),
            Some(expected_checkpoint),
            "after sync {cycles}"
        );
    }
    assert_eq!(cycles, cycles_until_sent);
    assert_eq!(fields(&alice), vec!["bob@example.invalid"]);

    // The remaining chunks bring the checkpoint to today, then it stays.
    while checkpoint(&alice, &application) != Some(today) {
        cycles += 1;
        assert!(cycles <= RETAINED_DAYS / CATCH_UP_DAYS_PER_CYCLE + 2);
        sync(&mut alice);
    }
    sync(&mut alice);
    assert_eq!(checkpoint(&alice, &application), Some(today));

    // The relay no longer holds the envelope: Alice acknowledged it.
    let mut probe = HttpTransport::new(HttpTransportConfig::for_testing(outer.url(), 2_000));
    probe.set_ohttp(OhttpClient::new(outer.ohttp_key_config()).unwrap());
    let old_token = token_hex(&compute_mailbox_token(
        linked.shared_secret.as_bytes(),
        alice.identity().unwrap().signing_public_key(),
        send_day,
    ));
    assert!(probe.fetch(&[old_token]).unwrap().is_empty());
}
