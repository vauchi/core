// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! adr-constraints "Public keys are never routing identifiers"
//! (ADR-029/ADR-037): a send and a receive between two contacts put no
//! identity or exchange public key on the wire, in any encoding, in the
//! outer OHTTP request or the request the gateway decapsulates
//! (private#972, SC-64).

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use base64::Engine as _;

use crate::common::linked_peer::link_contacts;
use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};
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
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    let mut client = Vauchi::new_with(config, clock, OsSecureRng::shared(), None)
        .unwrap()
        .with_sleeper(Arc::new(FakeSleeper::new()));
    client.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).unwrap());
    client.create_identity(name).unwrap();
    (client, dir)
}

fn sync(client: &mut Vauchi) {
    client.set_next_sync_allowed(Instant::now());
    let result = client.sync().unwrap();
    assert!(
        matches!(&result, VauchiSyncOutcome::Ok { errors, .. } if errors.is_empty()),
        "{result:?}"
    );
}

/// Every spelling of `key` a request could carry: raw, hex either case,
/// and base64 with and without padding, standard and URL-safe.
fn encodings_of(key: &[u8]) -> Vec<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    vec![
        key.to_vec(),
        hex::encode(key).into_bytes(),
        hex::encode_upper(key).into_bytes(),
        STANDARD.encode(key).into_bytes(),
        STANDARD_NO_PAD.encode(key).into_bytes(),
        URL_SAFE.encode(key).into_bytes(),
        URL_SAFE_NO_PAD.encode(key).into_bytes(),
    ]
}

fn carries(haystack: &[u8], key: &[u8]) -> bool {
    encodings_of(key).iter().any(|needle| {
        haystack
            .windows(needle.len())
            .any(|w| w == needle.as_slice())
    })
}

/// Path, headers and body of every request either relay saw, and every
/// request body the gateway decapsulated.
fn wire(outer: &MockRelay, application: &MockRelay) -> Vec<Vec<u8>> {
    let mut wire: Vec<Vec<u8>> = outer
        .received()
        .into_iter()
        .chain(application.received())
        .map(|request| {
            let mut bytes = request.path.into_bytes();
            for (name, value) in request.headers {
                bytes.extend(name.into_bytes());
                bytes.extend(value.into_bytes());
            }
            bytes.extend(request.body);
            bytes
        })
        .collect();
    wire.extend(outer.ohttp_inner_requests());
    wire
}

const BOB_NAME: &str = "Bartholomew Quince-Arbuthnot";
const BOB_EMAIL: &str = "bartholomew.8c2e@example.invalid";

/// Alice and Bob after Bob sent Alice a new email field through the
/// relay and Alice received it.
struct Delivered {
    outer: MockRelay,
    application: MockRelay,
    alice: Vauchi,
    bob: Vauchi,
    _dirs: [tempfile::TempDir; 2],
}

fn bob_sends_alice_an_email() -> Delivered {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x63), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 8);
    let (mut alice, alice_dir) = client(&outer, &application, clock.clone(), "Alice");
    let (mut bob, bob_dir) = client(&outer, &application, clock.clone(), BOB_NAME);
    let linked = link_contacts(&alice, &bob, BOB_NAME);

    let mut updated = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", BOB_EMAIL, 0);
    let field_id = field.id().to_string();
    updated.add_field(field).unwrap();
    bob.update_own_card(&updated).unwrap();
    bob.set_contact_visibility_override(&linked.host_contact_id, &field_id, true)
        .unwrap();
    sync(&mut bob);
    sync(&mut alice);

    let received = alice
        .get_contact(&linked.peer_contact_id)
        .unwrap()
        .unwrap()
        .card()
        .fields()
        .iter()
        .any(|f| f.value() == BOB_EMAIL);
    assert!(
        received,
        "the update must cross the relay, or the wire proves nothing"
    );
    assert!(
        !outer.ohttp_inner_requests().is_empty(),
        "the gateway decapsulated no request"
    );
    Delivered {
        outer,
        application,
        alice,
        bob,
        _dirs: [alice_dir, bob_dir],
    }
}

// @internal
#[test]
fn a_send_and_a_receive_put_no_public_key_on_the_wire() {
    let delivered = bob_sends_alice_an_email();

    let keys: Vec<(&str, Vec<u8>)> = [("Alice", &delivered.alice), ("Bob", &delivered.bob)]
        .into_iter()
        .flat_map(|(name, client)| {
            let identity = client.identity().unwrap();
            [
                (name, identity.signing_public_key().to_vec()),
                (name, identity.exchange_public_key().to_vec()),
            ]
        })
        .collect();
    for request in wire(&delivered.outer, &delivered.application) {
        for (name, key) in &keys {
            assert!(
                !carries(&request, key),
                "a request carries {name}'s public key: {}",
                String::from_utf8_lossy(&request)
            );
        }
    }
}

// adr-constraints "Never expose plaintext to the relay" (ADR-004): the
// relay sees the update only as an encrypted blob.
// @internal
#[test]
fn the_relay_never_sees_the_card_update_in_plaintext() {
    let delivered = bob_sends_alice_an_email();

    for request in wire(&delivered.outer, &delivered.application) {
        for (what, secret) in [("email", BOB_EMAIL), ("name", BOB_NAME)] {
            assert!(
                !carries(&request, secret.as_bytes()),
                "a request carries Bob's {what}: {}",
                String::from_utf8_lossy(&request)
            );
        }
    }
}

// @internal
#[test]
fn the_scan_finds_a_key_in_every_encoding_it_checks() {
    let key = [0xa5u8; 32];
    for encoding in encodings_of(&key) {
        let mut request = b"{\"action\":\"send\",\"to\":\"".to_vec();
        request.extend(&encoding);
        request.extend(b"\"}");
        assert!(
            carries(&request, &key),
            "{}",
            String::from_utf8_lossy(&encoding)
        );
    }
    assert!(!carries(b"{\"action\":\"send\",\"to\":\"token\"}", &key));
}
