// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local OHTTP mailbox fixture contracts for private#691 catch-up journeys.

#![cfg(feature = "network-http")]

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use vauchi_core::clock::FakeClock;
use vauchi_core::network::{HttpTransport, HttpTransportConfig, OhttpClient};
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{DAY, NOW, SignedGateway};

fn mailbox_stack() -> (MockRelay, HttpTransport, Arc<FakeClock>) {
    let relay = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    relay.enable_ohttp_gateway(&SignedGateway::new(0x51), window_of(NOW));
    relay.enable_gateway_mailboxes(clock.clone(), 2);
    let mut http = HttpTransport::new(HttpTransportConfig::for_testing(relay.url(), 2_000));
    http.set_ohttp(OhttpClient::new(relay.ohttp_key_config()).unwrap());
    (relay, http, clock)
}

// @internal
#[test]
fn historical_mailbox_store_filters_fetches_and_requires_the_matching_ack() {
    let (_relay, http, clock) = mailbox_stack();
    let alice = "11".repeat(32);
    let bob = "22".repeat(32);
    let first = http.send_update(&alice, "YWxwaGE=", None).unwrap();
    let second = http.send_update(&bob, "YmV0YQ==", None).unwrap();

    assert!(http.fetch(&["33".repeat(32)]).unwrap().is_empty());
    clock.advance(Duration::from_secs(100 * DAY));
    let fetched = http.fetch(std::slice::from_ref(&alice)).unwrap();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].blob_id, first);
    assert_eq!(fetched[0].ciphertext, "YWxwaGE=");
    assert_eq!(fetched[0].mailbox_token.as_deref(), Some(alice.as_str()));
    assert_eq!(fetched[0].created_at, NOW);

    assert!(!http.acknowledge(&bob, &first).unwrap());
    assert_eq!(
        http.fetch(std::slice::from_ref(&alice)).unwrap()[0].blob_id,
        first
    );
    assert!(http.acknowledge(&alice, &first).unwrap());
    assert!(http.fetch(&[alice]).unwrap().is_empty());
    assert_eq!(http.fetch(&[bob]).unwrap()[0].blob_id, second);
}

// @internal
#[test]
fn historical_mailbox_store_pages_without_deleting_unacknowledged_blobs() {
    let (_relay, http, clock) = mailbox_stack();
    let token = "44".repeat(32);
    let first = http.send_update(&token, "YQ==", None).unwrap();
    clock.advance(Duration::from_secs(1));
    let second = http.send_update(&token, "Yg==", None).unwrap();
    clock.advance(Duration::from_secs(1));
    let third = http.send_update(&token, "Yw==", None).unwrap();

    let (page, truncated) = http.fetch_page(std::slice::from_ref(&token)).unwrap();
    assert!(truncated);
    assert_eq!(
        page.iter()
            .map(|blob| blob.blob_id.as_str())
            .collect::<Vec<_>>(),
        vec![first.as_str(), second.as_str()]
    );
    let (repeated, still_truncated) = http.fetch_page(std::slice::from_ref(&token)).unwrap();
    assert!(still_truncated);
    assert_eq!(
        repeated
            .iter()
            .map(|blob| blob.blob_id.as_str())
            .collect::<Vec<_>>(),
        vec![first.as_str(), second.as_str()]
    );
    assert!(http.acknowledge(&token, &first).unwrap());
    assert!(http.acknowledge(&token, &second).unwrap());

    let (last, truncated) = http.fetch_page(&[token]).unwrap();
    assert!(!truncated);
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].blob_id, third);
    assert_eq!(last[0].created_at, NOW + 2);
}
