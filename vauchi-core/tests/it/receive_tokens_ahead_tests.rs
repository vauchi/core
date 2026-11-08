// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An unlocked sync stores, under the Background Receive Key, the next 14
//! days of receive registrations, so a locked phone can fetch without a
//! shared key or the master seed (owner 2026-11-08, vauchi/private#1093
//! slice 5). A stored day is the whole set the day registers, padding
//! included, because the padding is fixed per day and a locked run that
//! registered less would mark itself to the relay (private#946).
//!
//! Traces to: features/sync_updates.feature

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::common::linked_peer::link_contacts;
use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{DAY, NOW, SignedGateway};
use vauchi_core::api::{Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::crypto::{ShreddingMasterKey, SymmetricKey};
use vauchi_core::network::OhttpClient;
use vauchi_core::network::mailbox_token::current_day_epoch;
use vauchi_core::rng::OsSecureRng;
use vauchi_core::sleeper::FakeSleeper;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, Storage, StorageError};
use vauchi_protocol::ohttp_key::window_of;

const DAYS_AHEAD: u64 = 14;

struct Relays {
    outer: MockRelay,
    application: MockRelay,
    clock: Arc<FakeClock>,
}

fn relays() -> Relays {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x65), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    Relays {
        outer,
        application,
        clock,
    }
}

fn client(
    relays: &Relays,
    dir: &tempfile::TempDir,
    keychain: Option<Arc<MemoryKeyStorage>>,
    name: &str,
) -> Vauchi {
    let mut config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_relay_url(relays.application.url())
        .with_ohttp_relay_url(relays.outer.url());
    if keychain.is_none() {
        config = config.with_storage_key(SymmetricKey::generate());
    }
    let mut client = Vauchi::new_with(
        config,
        relays.clock.clone(),
        OsSecureRng::shared(),
        keychain.map(|k| k as Arc<dyn SecureStorage>),
    )
    .unwrap()
    .with_sleeper(Arc::new(FakeSleeper::new()));
    client.set_ohttp_key_for_testing(OhttpClient::new(relays.outer.ohttp_key_config()).unwrap());
    client.create_identity(name).unwrap();
    client
}

fn sync(client: &mut Vauchi) {
    client.set_next_sync_allowed(Instant::now());
    let result = client.sync().unwrap();
    assert!(
        matches!(&result, VauchiSyncOutcome::Ok { errors, .. } if errors.is_empty()),
        "{result:?}"
    );
}

fn today(relays: &Relays) -> u64 {
    current_day_epoch(relays.clock.unix_seconds())
}

/// Every fetch request the relay received, each as a set of tokens. A
/// registered batch of 256 is fetched in chunks, so one batch spans several.
fn fetched_chunks(outer: &MockRelay) -> Vec<BTreeSet<String>> {
    const MARKER: &[u8] = b"{\"action\":\"fetch\"";
    outer
        .ohttp_inner_requests()
        .iter()
        .filter_map(|request| {
            let start = request
                .windows(MARKER.len())
                .position(|window| window == MARKER)?;
            let mut stream = serde_json::Deserializer::from_slice(&request[start..])
                .into_iter::<serde_json::Value>();
            let body = stream.next()?.ok()?;
            Some(
                body["mailbox_tokens"]
                    .as_array()?
                    .iter()
                    .filter_map(|token| token.as_str().map(str::to_owned))
                    .collect(),
            )
        })
        .collect()
}

fn as_sets(batches: &[Vec<String>]) -> Vec<BTreeSet<String>> {
    batches
        .iter()
        .map(|batch| batch.iter().cloned().collect())
        .collect()
}

// @internal
#[test]
fn a_sync_stores_today_exactly_the_batches_it_registered() {
    let relays = relays();
    let dir = tempfile::tempdir().unwrap();
    let mut alice = client(
        &relays,
        &dir,
        Some(Arc::new(MemoryKeyStorage::new())),
        "Alice",
    );
    let bob_dir = tempfile::tempdir().unwrap();
    let bob = client(&relays, &bob_dir, None, "Bob");
    link_contacts(&alice, &bob, "Bob");

    sync(&mut alice);

    let stored = alice
        .receive_registration_ahead(today(&relays))
        .unwrap()
        .expect("today is stored");
    let chunks = fetched_chunks(&relays.outer);
    assert!(!stored.batches().is_empty());
    // A locked run must register the very set an unlocked one did: the
    // chunks that touch a stored batch lie wholly inside it and cover it.
    for batch in as_sets(stored.batches()) {
        assert_eq!(batch.len(), 256, "a stored batch is padded like a sent one");
        let touching: Vec<&BTreeSet<String>> = chunks
            .iter()
            .filter(|chunk| !chunk.is_disjoint(&batch))
            .collect();
        assert!(
            touching.iter().all(|chunk| chunk.is_subset(&batch)),
            "a fetch mixed a stored batch with tokens outside it"
        );
        let covered: BTreeSet<String> = touching.into_iter().flatten().cloned().collect();
        assert_eq!(covered, batch, "the stored batch is what was fetched");
    }
}

// @internal
#[test]
fn the_next_fourteen_days_are_stored_and_no_more() {
    let relays = relays();
    let dir = tempfile::tempdir().unwrap();
    let mut alice = client(
        &relays,
        &dir,
        Some(Arc::new(MemoryKeyStorage::new())),
        "Alice",
    );

    sync(&mut alice);

    let first = today(&relays);
    let last = first + DAYS_AHEAD - 1;
    assert!(alice.receive_registration_ahead(first).unwrap().is_some());
    assert!(alice.receive_registration_ahead(last).unwrap().is_some());
    assert!(
        alice
            .receive_registration_ahead(last + 1)
            .unwrap()
            .is_none()
    );
}

// @internal
#[test]
fn a_new_contact_is_in_the_stored_days_after_the_next_sync() {
    let relays = relays();
    let dir = tempfile::tempdir().unwrap();
    let mut alice = client(
        &relays,
        &dir,
        Some(Arc::new(MemoryKeyStorage::new())),
        "Alice",
    );
    sync(&mut alice);
    let bob_dir = tempfile::tempdir().unwrap();
    let bob = client(&relays, &bob_dir, None, "Bob");
    let linked = link_contacts(&alice, &bob, "Bob");

    sync(&mut alice);

    let last = today(&relays) + DAYS_AHEAD - 1;
    let stored = alice.receive_registration_ahead(last).unwrap().unwrap();
    assert_eq!(
        stored.contact_ids(),
        BTreeSet::from([linked.peer_contact_id.as_str()])
    );
}

// @internal
#[test]
fn days_before_today_are_dropped() {
    let relays = relays();
    let dir = tempfile::tempdir().unwrap();
    let mut alice = client(
        &relays,
        &dir,
        Some(Arc::new(MemoryKeyStorage::new())),
        "Alice",
    );
    sync(&mut alice);
    let first = today(&relays);

    relays.clock.advance(Duration::from_secs(2 * DAY));
    sync(&mut alice);

    let store = alice.storage().receive_tokens();
    assert!(store.load_day(first).unwrap().is_none());
    assert!(
        store
            .load_day(first + 2 + DAYS_AHEAD - 1)
            .unwrap()
            .is_some()
    );
}

// @internal
#[test]
fn stored_days_need_the_receive_key() {
    let relays = relays();
    let dir = tempfile::tempdir().unwrap();
    let keychain = Arc::new(MemoryKeyStorage::new());
    {
        let mut alice = client(&relays, &dir, Some(keychain.clone()), "Alice");
        sync(&mut alice);
    }
    let smk: [u8; 32] = keychain
        .load_key("smk")
        .unwrap()
        .unwrap()
        .try_into()
        .unwrap();

    let storage = Storage::open(
        dir.path().join("vauchi.db"),
        ShreddingMasterKey::from_bytes(smk).derive_sek(),
    )
    .unwrap();

    assert!(matches!(
        storage.receive_tokens().load_day(today(&relays)),
        Err(StorageError::ReceiveKeyUnavailable)
    ));
}

// @internal
#[test]
fn without_a_receive_key_nothing_is_stored_ahead() {
    let relays = relays();
    let dir = tempfile::tempdir().unwrap();
    let mut alice = client(&relays, &dir, None, "Alice");

    sync(&mut alice);

    assert!(
        alice
            .receive_registration_ahead(today(&relays))
            .unwrap()
            .is_none()
    );
}

// @claim: S12
// @internal
#[test]
fn no_stored_token_is_readable_on_disk() {
    let relays = relays();
    let dir = tempfile::tempdir().unwrap();
    let token = {
        let mut alice = client(
            &relays,
            &dir,
            Some(Arc::new(MemoryKeyStorage::new())),
            "Alice",
        );
        sync(&mut alice);
        alice
            .receive_registration_ahead(today(&relays))
            .unwrap()
            .unwrap()
            .batches()[0][0]
            .clone()
    };

    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes
                .windows(token.len())
                .any(|window| window == token.as_bytes()),
            "a mailbox token links its owner's fetches; it is sealed at rest"
        );
    }
}
