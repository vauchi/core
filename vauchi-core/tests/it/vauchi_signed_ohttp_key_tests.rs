// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Connecting to a relay with an OHTTP anchor (#288 plan 6.2-6.4): the key
//! comes only from a signed record that chains to the anchor, one held key
//! per relay survives a restart, and the client fetches again only when the
//! window moves. A relay without an anchor keeps today's path until plan 6.7.

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::{SymmetricKey, Vauchi, VauchiConfig};
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::{CannedResponse, MockRelay};
use crate::common::signed_gateway::{DAY, NOW, SignedGateway};

struct Stack {
    application: MockRelay,
    outer: MockRelay,
    gateway: SignedGateway,
    clock: Arc<FakeClock>,
    dir: tempfile::TempDir,
    storage_key: SymmetricKey,
}

impl Stack {
    fn new() -> Self {
        Self {
            application: MockRelay::start(),
            outer: MockRelay::start(),
            gateway: SignedGateway::new(0x31),
            clock: Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW))),
            dir: tempfile::tempdir().expect("temp dir"),
            storage_key: SymmetricKey::generate(),
        }
    }

    /// A client of the application relay, reaching it through the outer
    /// relay, trusting `anchor`. Opening it again reuses the same storage.
    fn open(&self, anchor: [u8; 32]) -> Vauchi {
        let config = VauchiConfig::with_storage_path(db(self.dir.path()))
            .with_storage_key(self.storage_key.clone())
            .with_relay(self.application.url(), anchor)
            .with_ohttp_relay_url(self.outer.url());
        let clock: Arc<dyn Clock> = self.clock.clone();
        let mut vauchi =
            Vauchi::new_with(config, clock, vauchi_core::rng::OsSecureRng::shared(), None)
                .expect("open Vauchi");
        if vauchi.identity().is_none() {
            vauchi.create_identity("Alice").expect("identity");
        }
        vauchi
    }

    fn today(&self) -> u64 {
        window_of(self.clock.unix_seconds())
    }

    fn outer_paths(&self) -> Vec<String> {
        self.outer
            .received()
            .into_iter()
            .map(|request| request.path)
            .collect()
    }
}

fn db(dir: &Path) -> std::path::PathBuf {
    dir.join("vauchi.db")
}

// @scenario: release_privacy_multidevice_certification.feature:Neither relay can decrypt or identify application users
#[test]
fn connecting_takes_the_signed_key_and_never_the_unsigned_one() {
    let stack = Stack::new();
    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(stack.today()));
    let mut vauchi = stack.open(stack.gateway.anchor());

    vauchi.connect().expect("connect with the signed key");

    assert!(vauchi.has_ohttp_key());
    assert_eq!(stack.outer_paths(), vec!["/v2/ohttp-key-signed"]);
}

/// The fail-closed rule: a record that does not chain to the relay's
/// anchor leaves the client without a key — no bundled key, no unsigned
/// fetch in its place.
// @internal
#[test]
fn a_record_under_another_anchor_leaves_the_client_without_a_key() {
    let stack = Stack::new();
    let impostor = SignedGateway::new(0x77);
    stack
        .outer
        .queue("ohttp-key-signed", impostor.response(stack.today()));
    let mut vauchi = stack.open(stack.gateway.anchor());

    let result = vauchi.connect();

    assert!(result.is_err(), "connect must fail closed");
    assert!(!vauchi.has_ohttp_key());
    assert_eq!(stack.outer_paths(), vec!["/v2/ohttp-key-signed"]);
}

/// One held key per relay, kept in storage: a restart within the window
/// needs no fetch, so a client does not reveal itself to the outer relay
/// on every launch.
// @internal
#[test]
fn a_restart_within_the_window_uses_the_held_key_without_fetching() {
    let stack = Stack::new();
    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(stack.today()));
    stack
        .open(stack.gateway.anchor())
        .connect()
        .expect("first connect");
    stack.clock.advance(Duration::from_secs(3_600));

    let mut reopened = stack.open(stack.gateway.anchor());
    reopened.connect().expect("reconnect");

    assert!(reopened.has_ohttp_key());
    assert_eq!(stack.outer_paths(), vec!["/v2/ohttp-key-signed"]);
}

/// The window, not a TTL, decides when to fetch again.
// @internal
#[test]
fn a_new_window_fetches_the_next_record() {
    let stack = Stack::new();
    let today = stack.today();
    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(today));
    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(today + 1));
    stack
        .open(stack.gateway.anchor())
        .connect()
        .expect("first connect");
    stack.clock.advance(Duration::from_secs(DAY));

    let mut tomorrow = stack.open(stack.gateway.anchor());
    tomorrow.connect().expect("connect the next day");

    assert!(tomorrow.has_ohttp_key());
    assert_eq!(
        stack.outer_paths(),
        vec!["/v2/ohttp-key-signed", "/v2/ohttp-key-signed"]
    );
}

/// The gateway still holds yesterday's key, so a failed fetch the next day
/// need not cost the client its connection.
// @internal
#[test]
fn yesterdays_held_key_carries_a_failed_fetch() {
    let stack = Stack::new();
    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(stack.today()));
    stack
        .open(stack.gateway.anchor())
        .connect()
        .expect("first connect");
    stack.clock.advance(Duration::from_secs(DAY));
    stack.outer.set_default(CannedResponse::status(503));

    let mut tomorrow = stack.open(stack.gateway.anchor());
    tomorrow.connect().expect("yesterday's key still serves");

    assert!(tomorrow.has_ohttp_key());
}

/// Two windows on, the gateway no longer holds the key: without a new
/// signed record there is nothing to fall back to.
// @internal
#[test]
fn a_held_key_two_windows_old_does_not_carry_a_failed_fetch() {
    let stack = Stack::new();
    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(stack.today()));
    stack
        .open(stack.gateway.anchor())
        .connect()
        .expect("first connect");
    stack.clock.advance(Duration::from_secs(2 * DAY));
    stack.outer.set_default(CannedResponse::status(503));

    let mut later = stack.open(stack.gateway.anchor());

    assert!(later.connect().is_err());
    assert!(!later.has_ohttp_key());
    assert!(
        !stack.outer_paths().contains(&"/v2/ohttp-key".to_string()),
        "never the unsigned key"
    );
}

/// Building a transport performs no network I/O; for an anchored relay it
/// carries the held key or none — never the bundled one.
// @internal
#[test]
fn an_offline_transport_carries_the_held_key_or_none() {
    let stack = Stack::new();
    let fresh = stack.open(stack.gateway.anchor());
    let application = stack.application.url();

    assert!(
        !fresh.build_relay_transport(&application, 1_000).has_ohttp(),
        "no held key yet, and the bundled key is not for this relay"
    );

    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(stack.today()));
    let mut connected = stack.open(stack.gateway.anchor());
    connected.connect().expect("connect");
    let offline = stack.open(stack.gateway.anchor());

    assert!(
        offline
            .build_relay_transport(&application, 1_000)
            .has_ohttp()
    );
}

/// An exchange can be the first thing a client does on an anchored relay
/// (a TUI starting a Link right after creating its identity): with no
/// held key yet, it must fetch the signed record before it posts, not
/// set out without an OHTTP route.
// @internal
#[test]
fn a_first_exchange_fetches_the_signed_key_before_posting() {
    let stack = Stack::new();
    stack
        .outer
        .queue("ohttp-key-signed", stack.gateway.response(stack.today()));
    let vauchi = stack.open(stack.gateway.anchor());

    let _ = vauchi.start_relay_exchange(None);

    assert_eq!(
        stack.outer_paths().first().map(String::as_str),
        Some("/v2/ohttp-key-signed")
    );
    assert!(
        stack.outer_paths().contains(&"/v2/ohttp".to_string()),
        "the offer goes out through OHTTP: {:?}",
        stack.outer_paths()
    );
}
