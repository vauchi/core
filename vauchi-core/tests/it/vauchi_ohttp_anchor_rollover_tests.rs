// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A client follows its relay's anchor rollover (#288 plan 2.10, decision
//! 0.13): when the signed record no longer chains to the anchor it holds,
//! it fetches the relay's rollover chain, follows it only to the backup it
//! committed to, and keeps the anchor it reached for the next launch.

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::{SymmetricKey, Vauchi, VauchiConfig};
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{DAY, NOW, SignedGateway, rollover_response};

const SIGNED: &str = "/v2/ohttp-key-signed";
const ROLLOVER: &str = "/v2/ohttp-anchor-rollover";

struct Stack {
    application: MockRelay,
    outer: MockRelay,
    /// The anchor the client is configured with.
    original: SignedGateway,
    /// The backup `original` committed to; it takes over in the rollover.
    backup: SignedGateway,
    /// The backup `backup` commits to when it takes over.
    next: SignedGateway,
    clock: Arc<FakeClock>,
    dir: tempfile::TempDir,
    storage_key: SymmetricKey,
}

impl Stack {
    fn new() -> Self {
        Self {
            application: MockRelay::start(),
            outer: MockRelay::start(),
            original: SignedGateway::new(0x31),
            backup: SignedGateway::new(0x51),
            next: SignedGateway::new(0x71),
            clock: Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW))),
            dir: tempfile::tempdir().expect("temp dir"),
            storage_key: SymmetricKey::generate(),
        }
    }

    fn open_with(&self, config: VauchiConfig) -> Vauchi {
        let config = config
            .with_storage_key(self.storage_key.clone())
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

    /// A client trusting `anchor` with `backup_commitment` as its backup.
    fn open(&self, anchor: [u8; 32], backup_commitment: [u8; 32]) -> Vauchi {
        self.open_with(
            self.config()
                .with_relay(self.application.url(), anchor)
                .with_ohttp_backup_commitment(backup_commitment),
        )
    }

    /// A client configured with the original anchor and its backup.
    fn open_original(&self) -> Vauchi {
        self.open(self.original.anchor(), self.backup.commitment())
    }

    fn config(&self) -> VauchiConfig {
        VauchiConfig::with_storage_path(self.dir.path().join("vauchi.db"))
    }

    fn today(&self) -> u64 {
        window_of(self.clock.unix_seconds())
    }

    /// The relay after the ceremony's rollover: `backup` signs, and the
    /// chain records its takeover.
    fn rolled_over(&self, window: u64) {
        self.outer
            .queue("ohttp-key-signed", self.backup.response(window));
        self.outer.queue(
            "ohttp-anchor-rollover",
            rollover_response(&[self.backup.takeover(self.next.commitment())]),
        );
    }

    fn outer_paths(&self) -> Vec<String> {
        self.outer
            .received()
            .into_iter()
            .map(|request| request.path)
            .collect()
    }
}

// @scenario: release_privacy_multidevice_certification.feature:Neither relay can decrypt or identify application users
#[test]
fn a_rolled_over_relay_is_followed_to_the_committed_backup() {
    let stack = Stack::new();
    stack.rolled_over(stack.today());
    let mut vauchi = stack.open_original();

    vauchi.connect().expect("connect under the backup anchor");

    assert!(vauchi.has_ohttp_key());
    assert_eq!(stack.outer_paths(), vec![SIGNED, ROLLOVER]);
}

/// The anchor reached is kept: the next launch accepts the backup's
/// records without walking the chain again.
// @internal
#[test]
fn the_anchor_reached_survives_a_restart() {
    let stack = Stack::new();
    let today = stack.today();
    stack.rolled_over(today);
    stack
        .open_original()
        .connect()
        .expect("follow the rollover");
    stack.clock.advance(Duration::from_secs(DAY));
    stack
        .outer
        .queue("ohttp-key-signed", stack.backup.response(today + 1));

    let mut tomorrow = stack.open_original();
    tomorrow.connect().expect("connect under the kept anchor");

    assert!(tomorrow.has_ohttp_key());
    assert_eq!(stack.outer_paths(), vec![SIGNED, ROLLOVER, SIGNED]);
}

/// Whoever stole the original anchor cannot choose its successor: a chain
/// to a key the client never committed to leaves it without a key.
// @internal
#[test]
fn a_takeover_by_a_key_never_committed_to_is_refused() {
    let stack = Stack::new();
    let thief = SignedGateway::new(0x99);
    stack
        .outer
        .queue("ohttp-key-signed", thief.response(stack.today()));
    stack.outer.queue(
        "ohttp-anchor-rollover",
        rollover_response(&[thief.takeover(stack.next.commitment())]),
    );
    let mut vauchi = stack.open_original();

    assert!(vauchi.connect().is_err());
    assert!(!vauchi.has_ohttp_key());
}

/// Without a committed backup there is nothing a chain could lead to, so
/// the client does not ask for one.
// @internal
#[test]
fn without_a_backup_commitment_no_chain_is_fetched() {
    let stack = Stack::new();
    stack.rolled_over(stack.today());
    let mut vauchi = stack.open_with(
        stack
            .config()
            .with_relay(stack.application.url(), stack.original.anchor()),
    );

    assert!(vauchi.connect().is_err());
    assert!(!vauchi.has_ohttp_key());
    assert_eq!(stack.outer_paths(), vec![SIGNED]);
}

/// The anchor reached belongs to the configured one it was reached from;
/// configuring another anchor starts from that anchor, not from the kept
/// rollover.
// @internal
#[test]
fn a_different_configured_anchor_does_not_inherit_the_rollover() {
    let stack = Stack::new();
    let today = stack.today();
    stack.rolled_over(today);
    stack
        .open_original()
        .connect()
        .expect("follow the rollover");
    let other = SignedGateway::new(0x11);
    stack
        .outer
        .queue("ohttp-key-signed", stack.backup.response(today));
    stack.outer.queue(
        "ohttp-anchor-rollover",
        rollover_response(&[stack.backup.takeover(stack.next.commitment())]),
    );

    let mut reconfigured = stack.open(other.anchor(), stack.next.commitment());

    assert!(reconfigured.connect().is_err());
    assert!(!reconfigured.has_ohttp_key());
}
