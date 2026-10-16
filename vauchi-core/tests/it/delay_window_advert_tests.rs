// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The outer relay advertises its delayed path (vauchi/private#628, D-F6
//! amendment 2026-10-16): with no relay directory yet, every synchronous
//! OHTTP answer carries `Vauchi-Ohttp-Delay-Window`, and a client that has
//! seen it fetches through the delayed path from then on, across restarts.

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;
use vauchi_core::sleeper::FakeSleeper;
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};

const POLL: Duration = Duration::from_secs(30);
const MAX_WAIT: Duration = Duration::from_secs(960);

struct Client {
    vauchi: Vauchi,
    sleeper: Arc<FakeSleeper>,
}

fn outer_relay() -> MockRelay {
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    mock
}

/// A client of `outer` storing in `db`, with delayed mode left to the
/// advertisement (`RelayConfig::ohttp_delayed` off).
fn client(outer: &MockRelay, application: &MockRelay, db: &Path, key: &SymmetricKey) -> Client {
    let config = VauchiConfig::with_storage_path(db)
        .with_storage_key(key.clone())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    let sleeper = Arc::new(FakeSleeper::new());
    let mut vauchi = Vauchi::new(config)
        .expect("create Vauchi")
        .with_sleeper(sleeper.clone());
    if vauchi.identity().is_none() {
        vauchi.create_identity("Advertised").expect("identity");
    }
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).expect("key"));
    Client { vauchi, sleeper }
}

fn sync(client: &mut Client) {
    client
        .vauchi
        .set_next_sync_allowed(std::time::Instant::now());
    let _ = client.vauchi.sync();
}

fn delayed_posts(outer: &MockRelay) -> usize {
    outer
        .received()
        .iter()
        .filter(|r| r.method == "POST" && r.path == "/v2/ohttp-async")
        .count()
}

/// Fetches the gateway answered, less those that came through the delayed
/// path (no mail, so no acknowledgements are posted).
fn synchronous_fetches(outer: &MockRelay) -> usize {
    let fetches = outer
        .ohttp_actions()
        .iter()
        .filter(|action| action.as_str() == "fetch")
        .count();
    fetches - delayed_posts(outer)
}

struct World {
    outer: MockRelay,
    application: MockRelay,
    dir: tempfile::TempDir,
    key: SymmetricKey,
}

impl World {
    fn new() -> Self {
        Self {
            outer: outer_relay(),
            application: MockRelay::start(),
            dir: tempfile::tempdir().expect("temp dir"),
            key: SymmetricKey::generate(),
        }
    }

    fn client(&self) -> Client {
        client(
            &self.outer,
            &self.application,
            &self.dir.path().join("vauchi.db"),
            &self.key,
        )
    }
}

// @internal
#[test]
fn an_advertised_window_moves_the_next_sync_onto_the_delayed_path() {
    let world = World::new();
    world.outer.advertise_delay_window(Some("900"));
    let mut client = world.client();

    sync(&mut client);
    assert_eq!(delayed_posts(&world.outer), 0, "nothing was advertised yet");
    sync(&mut client);

    assert!(
        delayed_posts(&world.outer) > 0,
        "the second sync fetches through the delayed path"
    );
}

// @internal
#[test]
fn without_an_advertisement_fetches_stay_synchronous() {
    let world = World::new();
    let mut client = world.client();

    sync(&mut client);
    sync(&mut client);

    assert_eq!(delayed_posts(&world.outer), 0);
    assert!(synchronous_fetches(&world.outer) > 0);
}

// A process that runs one sync, like the CLI, must not make its first
// fetch synchronous each time it starts.
// @internal
#[test]
fn the_advertised_window_survives_a_restart() {
    let world = World::new();
    world.outer.advertise_delay_window(Some("900"));
    sync(&mut world.client());
    let learned_with = synchronous_fetches(&world.outer);

    let mut restarted = world.client();
    sync(&mut restarted);

    assert!(delayed_posts(&world.outer) > 0);
    assert_eq!(
        synchronous_fetches(&world.outer),
        learned_with,
        "the restarted client's first sync fetches through the delayed path"
    );
}

// @internal
#[test]
fn a_relay_that_drops_the_path_returns_its_clients_to_synchronous_fetches() {
    let world = World::new();
    world.outer.advertise_delay_window(Some("900"));
    let mut client = world.client();
    sync(&mut client);
    world.outer.advertise_delay_window(None);
    world.outer.refuse_async_path();

    sync(&mut client);
    let before = synchronous_fetches(&world.outer);
    sync(&mut client);

    assert!(delayed_posts(&world.outer) > 0, "refused posts");
    assert_eq!(
        synchronous_fetches(&world.outer),
        before + 1,
        "the sync after the refusal fetches synchronously"
    );
}

// CC-14 / DC-01: the header is unauthenticated input from the outer relay.
// @internal
#[test]
fn a_malformed_or_out_of_range_window_is_not_an_advertisement() {
    let huge = "9".repeat(40);
    for value in [
        "0", "3601", "-5", "+900", "9e2", "900s", "0x384", "٩٠٠", &huge,
    ] {
        let world = World::new();
        world.outer.advertise_delay_window(Some(value));
        let mut client = world.client();

        sync(&mut client);
        sync(&mut client);

        assert_eq!(delayed_posts(&world.outer), 0, "{value:?} is not a window");
    }
}

// @internal
#[test]
fn the_bounds_of_the_window_are_advertisements() {
    for value in ["1", "3600"] {
        let world = World::new();
        world.outer.advertise_delay_window(Some(value));
        let mut client = world.client();

        sync(&mut client);
        sync(&mut client);

        assert!(delayed_posts(&world.outer) > 0, "{value:?} is a window");
    }
}

// The wait must cover the window the relay actually uses (#628 step 2),
// not the 900 s default.
// @internal
#[test]
fn a_waiting_sync_waits_the_advertised_window() {
    let world = World::new();
    world.outer.advertise_delay_window(Some("120"));
    let mut client = world.client();
    sync(&mut client);
    client
        .vauchi
        .set_next_sync_allowed(std::time::Instant::now());

    client
        .vauchi
        .sync_collecting_delayed(POLL, MAX_WAIT)
        .expect("sync");

    assert_eq!(client.sleeper.calls(), vec![POLL; 4], "120 s in 30 s polls");
}
