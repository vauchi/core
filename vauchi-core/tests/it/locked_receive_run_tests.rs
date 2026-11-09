// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A locked run receives over the relay with the Background Receive Key
//! alone: it registers the sets the last unlocked sync stored, fetches,
//! keeps what an existing session opens and acknowledges it, and sends
//! nothing else (ADR-033 amendment 2026-11-07 point 6, vauchi/private#1093
//! slice 6, step 4). It does not run past the one-day window or while the
//! relay offers delayed fetches (owner 2026-11-09).
//!
//! Traces to: features/storage_key_upgrade.feature

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::common::linked_peer::{LinkedPeer, link_contacts};
use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{DAY, NOW, SignedGateway};
use vauchi_core::api::locked_receive::{
    LockedInboxItem, LockedRunOutcome, receive_while_locked_with_ohttp,
};
use vauchi_core::api::{RelayConfig, Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;
use vauchi_core::network::mailbox_token::current_day_epoch;
use vauchi_core::rng::OsSecureRng;
use vauchi_core::sleeper::FakeSleeper;
use vauchi_core::storage::{LockedReceiveStorage, MemoryKeyStorage, SecureStorage};
use vauchi_protocol::ohttp_key::window_of;

struct World {
    outer: MockRelay,
    application: MockRelay,
    clock: Arc<FakeClock>,
    /// The receiver, whose phone locks.
    alice: Vauchi,
    alice_dir: tempfile::TempDir,
    keychain: Arc<MemoryKeyStorage>,
    bob: Vauchi,
    _bob_dir: tempfile::TempDir,
    linked: LinkedPeer,
}

fn config(world_relays: (&MockRelay, &MockRelay), dir: &tempfile::TempDir) -> VauchiConfig {
    let (outer, application) = world_relays;
    VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url())
}

fn client(
    outer: &MockRelay,
    application: &MockRelay,
    clock: Arc<FakeClock>,
    keychain: Option<Arc<MemoryKeyStorage>>,
    name: &str,
) -> (Vauchi, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config((outer, application), &dir);
    if keychain.is_none() {
        config = config.with_storage_key(SymmetricKey::generate());
    }
    let mut client = Vauchi::new_with(
        config,
        clock,
        OsSecureRng::shared(),
        keychain.map(|k| k as Arc<dyn SecureStorage>),
    )
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

/// Alice and Bob linked; Alice's phone synced unlocked once.
fn world() -> World {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x66), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let keychain = Arc::new(MemoryKeyStorage::new());
    let (mut alice, alice_dir) = client(
        &outer,
        &application,
        clock.clone(),
        Some(keychain.clone()),
        "Alice",
    );
    let (bob, bob_dir) = client(&outer, &application, clock.clone(), None, "Bob");
    let linked = link_contacts(&alice, &bob, "Bob");
    sync(&mut alice);
    World {
        outer,
        application,
        clock,
        alice,
        alice_dir,
        keychain,
        bob,
        _bob_dir: bob_dir,
        linked,
    }
}

impl World {
    /// Bob shares a new field with Alice and syncs it to the relay.
    fn bob_sends_an_update(&mut self) {
        let mut updated = self.bob.own_card().unwrap().unwrap().clone();
        let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
        let field_id = field.id().to_string();
        updated.add_field(field).unwrap();
        self.bob.update_own_card(&updated).unwrap();
        self.bob
            .set_contact_visibility_override(&self.linked.host_contact_id, &field_id, true)
            .unwrap();
        let sent = sync(&mut self.bob);
        assert!(
            matches!(&sent, VauchiSyncOutcome::Ok { sent, .. } if *sent > 0),
            "{sent:?}"
        );
    }

    fn relay(&self) -> RelayConfig {
        config((&self.outer, &self.application), &self.alice_dir).relay
    }

    fn locked_run(&self, relay: &RelayConfig) -> LockedRunOutcome {
        let brk: [u8; 32] = self
            .keychain
            .load_key("brk")
            .unwrap()
            .unwrap()
            .try_into()
            .unwrap();
        let storage = LockedReceiveStorage::open(
            self.alice_dir.path().join("vauchi.db"),
            SymmetricKey::from_bytes(brk),
        )
        .unwrap();
        receive_while_locked_with_ohttp(
            &storage,
            relay,
            self.clock.clone(),
            OsSecureRng::shared().as_ref(),
            OhttpClient::new(self.outer.ohttp_key_config()).unwrap(),
        )
        .unwrap()
    }

    fn inbox(&self) -> Vec<LockedInboxItem> {
        self.alice
            .storage()
            .receive_inbox()
            .items()
            .unwrap()
            .readable
            .iter()
            .map(|item| LockedInboxItem::decode(&item.update).unwrap())
            .collect()
    }

    fn today(&self) -> u64 {
        current_day_epoch(self.clock.unix_seconds())
    }
}

/// Every token these fetch requests asked for.
fn fetched_tokens(requests: &[Vec<u8>]) -> BTreeSet<String> {
    const MARKER: &[u8] = b"{\"action\":\"fetch\"";
    requests
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
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

// @internal
#[test]
fn a_locked_run_keeps_an_update_for_the_unlock_and_acknowledges_it() {
    let mut world = world();
    world.bob_sends_an_update();
    world.clock.advance(Duration::from_secs(3600));

    let first = world.locked_run(&world.relay());
    let second = world.locked_run(&world.relay());

    // Bob's sync also sent a blob on a session Alice does not hold yet;
    // starting one needs the relationship key, so it waits for the unlock.
    assert_eq!(first, LockedRunOutcome::Received { stored: 1, left: 1 });
    assert_eq!(
        second,
        LockedRunOutcome::Received { stored: 0, left: 1 },
        "the acknowledged update is gone from the relay; the other blob stays"
    );
    let inbox = world.inbox();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].contact_id(), world.linked.peer_contact_id);
    assert_eq!(
        world
            .alice
            .get_contact(&world.linked.peer_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .len(),
        0,
        "the card waits for the merge at the unlock"
    );
}

// @internal
#[test]
fn a_locked_run_registers_only_what_the_unlocked_sync_stored() {
    let world = world();
    let stored: BTreeSet<String> = [world.today() - 1, world.today()]
        .into_iter()
        .flat_map(|day| {
            world
                .alice
                .receive_registration_ahead(day)
                .unwrap()
                .unwrap()
                .batches()
                .iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect();
    let before = world.outer.ohttp_inner_requests().len();

    world.locked_run(&world.relay());

    let fetched = fetched_tokens(&world.outer.ohttp_inner_requests()[before..]);
    assert_eq!(
        fetched, stored,
        "a locked run fetches yesterday's and today's stored sets, no more and no less"
    );
}

// @internal
#[test]
fn a_locked_run_sends_nothing_but_registrations_fetches_and_acks() {
    let mut world = world();
    world.bob_sends_an_update();
    let before = world.outer.ohttp_actions().len();

    world.locked_run(&world.relay());

    let actions: BTreeSet<String> = world.outer.ohttp_actions()[before..]
        .iter()
        .cloned()
        .collect();
    assert!(
        actions.is_subset(&BTreeSet::from([
            "register".to_string(),
            "fetch".to_string(),
            "ack".to_string(),
        ])),
        "{actions:?}"
    );
    assert!(actions.contains("ack"), "{actions:?}");
}

// @internal
#[test]
fn a_locked_run_a_day_after_the_unlocked_sync_receives_nothing() {
    let mut world = world();
    world.bob_sends_an_update();
    world.clock.advance(Duration::from_secs(DAY));
    let before = world.outer.ohttp_actions().len();

    let outcome = world.locked_run(&world.relay());

    assert_eq!(outcome, LockedRunOutcome::WindowEnded);
    assert_eq!(world.outer.ohttp_actions().len(), before, "no request");
    assert!(world.inbox().is_empty());
}

// @internal
#[test]
fn a_locked_run_waits_for_the_unlock_while_delayed_fetches_are_offered() {
    let mut world = world();
    world.bob_sends_an_update();
    let mut relay = world.relay();
    relay.ohttp_delayed = true;
    let before = world.outer.ohttp_actions().len();

    let outcome = world.locked_run(&relay);

    assert_eq!(outcome, LockedRunOutcome::DelayedFetchesOffered);
    assert_eq!(world.outer.ohttp_actions().len(), before, "no request");
}

// @internal
#[test]
fn a_phone_that_never_synced_unlocked_has_nothing_to_register() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x67), window_of(NOW));
    let keychain = Arc::new(MemoryKeyStorage::new());
    let (_alice, dir) = client(
        &outer,
        &application,
        clock.clone(),
        Some(keychain.clone()),
        "Alice",
    );
    let brk: [u8; 32] = keychain
        .load_key("brk")
        .unwrap()
        .unwrap()
        .try_into()
        .unwrap();
    let storage =
        LockedReceiveStorage::open(dir.path().join("vauchi.db"), SymmetricKey::from_bytes(brk))
            .unwrap();

    let outcome = receive_while_locked_with_ohttp(
        &storage,
        &config((&outer, &application), &dir).relay,
        clock,
        OsSecureRng::shared().as_ref(),
        OhttpClient::new(outer.ohttp_key_config()).unwrap(),
    )
    .unwrap();

    assert_eq!(outcome, LockedRunOutcome::NothingStored);
    assert!(outer.ohttp_actions().is_empty());
}

// @internal
#[test]
fn a_relay_that_starts_delaying_after_the_unlock_gets_one_locked_round() {
    let mut world = world();
    world.bob_sends_an_update();
    // The last unlocked sync saw no window; the relay offers one now. A
    // locked run cannot know before its first answer, so it fetches once,
    // keeps the window, and the next locked run makes no request.
    world.outer.advertise_delay_window(Some("900"));

    let first = world.locked_run(&world.relay());
    let after_first = world.outer.ohttp_actions().len();
    let second = world.locked_run(&world.relay());

    assert!(
        matches!(first, LockedRunOutcome::Received { .. }),
        "{first:?}"
    );
    assert_eq!(second, LockedRunOutcome::DelayedFetchesOffered);
    assert_eq!(world.outer.ohttp_actions().len(), after_first, "no request");
}
