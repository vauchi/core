// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The link-mode exchange domain API (private#670) against the in-process
//! relay's real escrow gate store: Alice shares, Bob accepts, both advance
//! until each holds the other.

#![cfg(feature = "network-http")]

use vauchi_core::api::VauchiConfig;
use vauchi_core::api::vauchi::{LinkExchange, LinkExchangeProgress, VauchiBuilder};

use crate::common::mock_relay::MockRelay;

fn user_at(mock: &MockRelay, name: &str) -> (vauchi_core::Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config =
        VauchiConfig::with_storage_path(dir.path().join("vauchi.db")).with_relay_url(mock.url());
    config.ohttp.allow_direct = true;
    let mut vauchi = VauchiBuilder::new().config(config).build().expect("build");
    vauchi.create_identity(name).expect("identity");
    (vauchi, dir)
}

fn advance_until_done(
    users: [(&vauchi_core::Vauchi, &mut LinkExchange); 2],
) -> [LinkExchangeProgress; 2] {
    let [(a, ex_a), (b, ex_b)] = users;
    let mut progress = [LinkExchangeProgress::Waiting, LinkExchangeProgress::Waiting];
    for _ in 0..20 {
        progress = [
            a.advance_link_exchange(ex_a).expect("advance initiator"),
            b.advance_link_exchange(ex_b).expect("advance responder"),
        ];
        if !progress.contains(&LinkExchangeProgress::Waiting) {
            break;
        }
    }
    progress
}

fn contact_id(vauchi: &vauchi_core::Vauchi) -> String {
    hex::encode(vauchi.identity().unwrap().signing_public_key())
}

// @scenario: contact_exchange :: two users exchange over a link without the app screens
// @internal
#[test]
fn a_shared_link_accepted_by_the_peer_completes_on_both_sides() {
    let mock = MockRelay::start();
    mock.enable_escrow_store();
    let (alice, _a) = user_at(&mock, "Alice");
    let (bob, _b) = user_at(&mock, "Bob");

    let (link, mut alice_exchange) = alice.start_link_exchange().expect("share");
    let mut bob_exchange = bob.accept_link_exchange(&link).expect("accept");
    let progress = advance_until_done([(&alice, &mut alice_exchange), (&bob, &mut bob_exchange)]);

    assert_eq!(
        progress,
        [
            LinkExchangeProgress::Completed {
                contact_id: contact_id(&bob)
            },
            LinkExchangeProgress::Completed {
                contact_id: contact_id(&alice)
            },
        ]
    );
    assert!(alice.get_contact(&contact_id(&bob)).unwrap().is_some());
    assert!(bob.get_contact(&contact_id(&alice)).unwrap().is_some());
}

// @internal
#[test]
fn a_shared_link_is_an_https_exchange_link() {
    let mock = MockRelay::start();
    let (alice, _a) = user_at(&mock, "Alice");

    let (link, _exchange) = alice.start_link_exchange().expect("share");

    assert!(
        vauchi_core::exchange::link_mode::parse_link_url(&link).is_some(),
        "{link}"
    );
    assert!(link.starts_with("https://vauchi.app/x#"), "{link}");
}

// CC-14: the link is untrusted input.
// @internal
#[test]
fn accepting_anything_but_an_exchange_link_is_refused() {
    let mock = MockRelay::start();
    let (bob, _b) = user_at(&mock, "Bob");

    for junk in [
        "",
        "hello",
        "https://example.com/x#pk=a&n=b",
        "https://vauchi.app/x#",
    ] {
        assert!(bob.accept_link_exchange(junk).is_err(), "{junk:?}");
    }
}

// @internal
#[test]
fn accepting_your_own_link_never_completes_an_exchange_with_yourself() {
    let mock = MockRelay::start();
    mock.enable_escrow_store();
    let (alice, _a) = user_at(&mock, "Alice");
    let (link, mut sharing) = alice.start_link_exchange().expect("share");
    let mut accepting = alice.accept_link_exchange(&link).expect("accept");

    let progress = advance_until_done([(&alice, &mut sharing), (&alice, &mut accepting)]);

    assert!(
        !progress
            .iter()
            .any(|p| matches!(p, LinkExchangeProgress::Completed { .. })),
        "{progress:?}"
    );
    assert!(alice.get_contact(&contact_id(&alice)).unwrap().is_none());
}
