// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Under duress the real profile keeps syncing as a background worker that
//! exposes nothing to the UI (ADR-080 points 2 and 5, vauchi/private#1117).

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::common::linked_peer::link_contacts;
use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{NOW, SignedGateway};
use vauchi_core::activity_log::RETENTION_SECS;
use vauchi_core::api::{AuthMode, Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::clock::FakeClock;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;
use vauchi_core::rng::OsSecureRng;
use vauchi_core::sleeper::FakeSleeper;
use vauchi_protocol::ohttp_key::window_of;

const APP_PIN: &str = "real-pin-2468";
const DURESS_PIN: &str = "135791";

struct Relays {
    outer: MockRelay,
    application: MockRelay,
    clock: Arc<FakeClock>,
}

impl Relays {
    fn start() -> Self {
        let outer = MockRelay::start();
        let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
        outer.enable_ohttp_gateway(&SignedGateway::new(0x71), window_of(NOW));
        outer.enable_gateway_mailboxes(clock.clone(), 4);
        Relays {
            outer,
            application: MockRelay::start(),
            clock,
        }
    }
}

struct Install {
    dir: tempfile::TempDir,
    key: SymmetricKey,
}

impl Install {
    fn new() -> Self {
        Install {
            dir: tempfile::tempdir().unwrap(),
            key: SymmetricKey::generate(),
        }
    }

    fn open(&self, relays: &Relays) -> Vauchi {
        let config = VauchiConfig::with_storage_path(self.dir.path().join("vauchi.db"))
            .with_storage_key(self.key.clone())
            .with_relay_url(relays.application.url())
            .with_ohttp_relay_url(relays.outer.url());
        let mut client =
            Vauchi::new_with(config, relays.clock.clone(), OsSecureRng::shared(), None)
                .unwrap()
                .with_sleeper(Arc::new(FakeSleeper::new()));
        client
            .set_ohttp_key_for_testing(OhttpClient::new(relays.outer.ohttp_key_config()).unwrap());
        client
    }
}

fn sync(client: &mut Vauchi) -> VauchiSyncOutcome {
    client.set_next_sync_allowed(Instant::now());
    let outcome = client.sync().unwrap();
    assert!(
        matches!(&outcome, VauchiSyncOutcome::Ok { errors, .. } if errors.is_empty()),
        "{outcome:?}"
    );
    outcome
}

/// Bob shares an email with Alice and sends it.
fn bob_shares_an_email(bob: &mut Vauchi, alice_at_bob: &str) {
    let mut card = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    bob.update_own_card(&card).unwrap();
    bob.set_contact_visibility_override(alice_at_bob, &field_id, true)
        .unwrap();
    sync(bob);
}

fn card_values(alice: &Vauchi, bob_at_alice: &str) -> Vec<String> {
    alice
        .get_contact(bob_at_alice)
        .unwrap()
        .unwrap()
        .card()
        .fields()
        .iter()
        .map(|field| field.value().to_string())
        .collect()
}

/// Alice, with an app password and a duress PIN, linked to Bob; both on
/// file-backed installs, Alice unlocked with `credential`.
struct World {
    relays: Relays,
    alice_install: Install,
    alice: Vauchi,
    _bob_install: Install,
    bob: Vauchi,
    linked: crate::common::linked_peer::LinkedPeer,
}

fn world(credential: &str, expected_mode: AuthMode) -> World {
    let relays = Relays::start();
    let alice_install = Install::new();
    let mut alice = alice_install.open(&relays);
    alice.create_identity("Alice").unwrap();
    let bob_install = Install::new();
    let mut bob = bob_install.open(&relays);
    bob.create_identity("Bob").unwrap();
    let linked = link_contacts(&alice, &bob, "Bob");
    alice.setup_app_password(APP_PIN).unwrap();
    alice.setup_duress_password(DURESS_PIN).unwrap();
    assert_eq!(alice.authenticate(credential).unwrap(), expected_mode);
    World {
        relays,
        alice_install,
        alice,
        _bob_install: bob_install,
        bob,
        linked,
    }
}

// @scenario: duress_mode :: Duress mode with notification
#[test]
fn a_real_update_received_under_duress_is_applied_and_reaches_no_foreground_event() {
    let mut world = world(DURESS_PIN, AuthMode::Duress);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    world.alice.events().add_handler(Arc::new(move |event| {
        sink.lock().unwrap().push(format!("{event:?}"))
    }));

    bob_shares_an_email(&mut world.bob, &world.linked.host_contact_id);
    sync(&mut world.alice);
    drop(world.alice);

    assert_eq!(*seen.lock().unwrap(), Vec::<String>::new());
    let mut real = world.alice_install.open(&world.relays);
    assert_eq!(real.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    assert_eq!(
        card_values(&real, &world.linked.peer_contact_id),
        vec!["bob@example.invalid".to_string()]
    );
}

/// How many messages Alice's next sync sends after she adds a public field,
/// unlocked with `credential`.
fn sent_by_an_edit_and_sync(credential: &str, expected_mode: AuthMode) -> usize {
    let mut world = world(credential, expected_mode);
    // Alice holds the responder's ratchet, which sends once Bob has.
    bob_shares_an_email(&mut world.bob, &world.linked.host_contact_id);
    sync(&mut world.alice);
    let field = ContactField::new(FieldType::Email, "Work", "alice@work.invalid", 0);
    let field_id = field.id().to_string();
    world.alice.add_own_field(field).unwrap();
    world.alice.set_own_field_public(&field_id).unwrap();

    match sync(&mut world.alice) {
        VauchiSyncOutcome::Ok { sent, .. } => sent,
        other => panic!("{other:?}"),
    }
}

// @scenario: duress_mode :: Duress mode looks identical to normal mode
#[test]
fn the_cover_a_duress_edit_owes_leaves_with_the_workers_sync() {
    let real = sent_by_an_edit_and_sync(APP_PIN, AuthMode::Normal);
    let duress = sent_by_an_edit_and_sync(DURESS_PIN, AuthMode::Duress);

    assert_eq!(real, 1);
    assert_eq!(duress, real);
}

// @scenario: duress_mode :: Duress mode with notification
#[test]
fn what_the_worker_received_is_in_the_real_activity_log_and_never_notifies() {
    let mut world = world(DURESS_PIN, AuthMode::Duress);

    bob_shares_an_email(&mut world.bob, &world.linked.host_contact_id);
    sync(&mut world.alice);
    drop(world.alice);

    let mut real = world.alice_install.open(&world.relays);
    assert_eq!(real.authenticate(APP_PIN).unwrap(), AuthMode::Normal);
    let log = real.storage().activity_log();
    let from_bob: Vec<String> = log
        .activity_log_query_recent(NOW, RETENTION_SECS)
        .unwrap()
        .into_iter()
        .filter(|row| row.contact_id.as_deref() == Some(world.linked.peer_contact_id.as_str()))
        .map(|row| row.category)
        .collect();
    assert_eq!(from_bob, vec!["card_update_received".to_string()]);
    assert_eq!(log.claim_notifications().unwrap().len(), 0);
}
