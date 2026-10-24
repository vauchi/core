// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A background tick draws the next sync from the background
//! schedule (vauchi/private#603 option 2): Android re-arms its one-time
//! worker at that gap instead of a fixed 15-min period.

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::time::Duration;

use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;
use vauchi_protocol::ohttp_key::window_of;

use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};

const BACKGROUND_MIN: Duration = Duration::from_secs(60);
const BACKGROUND_MAX: Duration = Duration::from_secs(3_600);
const FOREGROUND_MAX: Duration = Duration::from_secs(300);

fn connected_client(outer: &MockRelay, application: &MockRelay) -> (Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    let mut vauchi = Vauchi::new(config).expect("create Vauchi");
    vauchi.create_identity("Background").expect("identity");
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).expect("key"));
    (vauchi, dir)
}

// @internal
#[test]
fn a_background_tick_draws_the_next_sync_from_the_background_schedule() {
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let application = MockRelay::start();
    let (mut vauchi, _dir) = connected_client(&outer, &application);

    let mut gaps = Vec::new();
    for _ in 0..10 {
        vauchi.set_next_sync_allowed(std::time::Instant::now());
        vauchi.background_sync_tick(None).expect("tick");
        gaps.push(vauchi.time_until_sync_due().expect("identity"));
    }

    // A deadline read a moment after the draw has lost a few milliseconds.
    let slack = Duration::from_secs(1);
    for gap in &gaps {
        assert!(
            *gap + slack >= BACKGROUND_MIN && *gap <= BACKGROUND_MAX,
            "{gap:?} outside the background bounds: {gaps:?}"
        );
    }
    // All ten under the 5-min foreground ceiling happens with p = 0.28^10.
    assert!(
        gaps.iter().any(|gap| *gap > FOREGROUND_MAX),
        "no gap beyond the foreground schedule: {gaps:?}"
    );
}

// A tick while the app is open keeps the 60 s schedule: drawing 15 min there
// would stall the foreground's syncs.
// @internal
#[test]
fn a_periodic_tick_keeps_the_foreground_schedule() {
    let outer = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    let application = MockRelay::start();
    let (mut vauchi, _dir) = connected_client(&outer, &application);

    for _ in 0..10 {
        vauchi.set_next_sync_allowed(std::time::Instant::now());
        vauchi.periodic_sync_tick().expect("tick");
        let gap = vauchi.time_until_sync_due().expect("identity");
        assert!(
            gap <= FOREGROUND_MAX,
            "{gap:?} beyond the foreground schedule"
        );
    }
}
