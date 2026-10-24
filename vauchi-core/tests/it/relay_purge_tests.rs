// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The purge an identity deletion sends: signed at send time by the
//! identity being deleted, and carrying the user's choice about the
//! guardian set inside the signature (#711 D2).

use vauchi_core::crypto::{PublicKey, Signature};
use vauchi_core::{Vauchi, VauchiConfig, VauchiError};

use crate::common::mock_relay::{CannedResponse, MockRelay};

fn identity_on(relay: &MockRelay) -> (Vauchi, tempfile::TempDir) {
    relay.queue(
        "purge",
        CannedResponse::ok_json(br#"{"status":"ok","blobs_deleted":0}"#.to_vec()),
    );
    let mut config = VauchiConfig::default().with_relay_url(relay.url());
    config.ohttp.allow_direct = true;
    config.relay.connect_timeout_ms = 2_000;
    let dir = tempfile::tempdir().unwrap();
    config.storage_path = dir.path().join("deleting.db");
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Alice").unwrap();
    (vauchi, dir)
}

fn sent_purge(relay: &MockRelay) -> serde_json::Value {
    let requests = relay.received();
    assert_eq!(requests.len(), 1, "exactly one purge is sent");
    serde_json::from_slice(&requests[0].body).unwrap()
}

fn signed_message(request: &serde_json::Value, suffix: &[u8]) -> Vec<u8> {
    let mut message = hex::decode(request["public_key"].as_str().unwrap()).unwrap();
    message.extend(hex::decode(request["purge_token"].as_str().unwrap()).unwrap());
    message.extend(request["timestamp"].as_u64().unwrap().to_be_bytes());
    message.extend_from_slice(suffix);
    message
}

fn verifies(request: &serde_json::Value, owner: [u8; 32], message: &[u8]) -> bool {
    let signature: [u8; 64] = hex::decode(request["signature"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    PublicKey::from_bytes(owner).verify(message, &Signature::from_bytes(signature))
}

// @scenario: privacy_compliance :: The relay purge carries my recovery choice
#[test]
fn erasing_everything_signs_the_guardian_set_choice_into_the_purge() {
    let relay = MockRelay::start();
    let (vauchi, _dir) = identity_on(&relay);
    let owner = *vauchi.identity().unwrap().signing_public_key();
    let before = vauchi.clock().unix_seconds();

    vauchi.purge_relay_data(true).unwrap();

    let request = sent_purge(&relay);
    assert_eq!(request["public_key"], hex::encode(owner));
    assert_eq!(request["erase_guardian_set"], true);
    let timestamp = request["timestamp"].as_u64().unwrap();
    assert!(timestamp >= before && timestamp <= vauchi.clock().unix_seconds());
    assert!(verifies(
        &request,
        owner,
        &signed_message(&request, b"erase-guardian-set")
    ));
    assert!(
        !verifies(&request, owner, &signed_message(&request, b"")),
        "dropping the flag from the request must break the signature"
    );
}

// A relay that predates the flag verifies exactly the bytes it always did.
// @scenario: privacy_compliance :: The relay purge carries my recovery choice
#[test]
fn keeping_recovery_sends_the_purge_old_relays_accept() {
    let relay = MockRelay::start();
    let (vauchi, _dir) = identity_on(&relay);
    let owner = *vauchi.identity().unwrap().signing_public_key();

    vauchi.purge_relay_data(false).unwrap();

    let request = sent_purge(&relay);
    assert!(request.get("erase_guardian_set").is_none(), "{request}");
    assert!(verifies(&request, owner, &signed_message(&request, b"")));
    assert!(!verifies(
        &request,
        owner,
        &signed_message(&request, b"erase-guardian-set")
    ));
}

// @internal
#[test]
fn each_purge_carries_a_fresh_token() {
    let relay = MockRelay::start();
    let (vauchi, _dir) = identity_on(&relay);
    relay.queue(
        "purge",
        CannedResponse::ok_json(br#"{"status":"ok","blobs_deleted":0}"#.to_vec()),
    );

    vauchi.purge_relay_data(false).unwrap();
    vauchi.purge_relay_data(false).unwrap();

    let requests = relay.received();
    let token = |i: usize| {
        serde_json::from_slice::<serde_json::Value>(&requests[i].body).unwrap()["purge_token"]
            .clone()
    };
    assert_ne!(token(0), token(1));
}

// @internal
#[test]
fn without_an_identity_nothing_is_sent() {
    let relay = MockRelay::start();
    let mut config = VauchiConfig::default().with_relay_url(relay.url());
    config.ohttp.allow_direct = true;
    let dir = tempfile::tempdir().unwrap();
    config.storage_path = dir.path().join("empty.db");
    let vauchi = Vauchi::new(config).unwrap();

    let err = vauchi.purge_relay_data(true).unwrap_err();

    assert!(matches!(err, VauchiError::IdentityNotInitialized), "{err}");
    assert!(relay.received().is_empty());
}
