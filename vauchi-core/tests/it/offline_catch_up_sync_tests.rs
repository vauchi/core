// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Catch-up through the public sync API against the faithful mailbox
//! fixture (vauchi/private#691): a client offline for 100 days receives
//! what the relay kept, in bounded steps, and its receive checkpoint
//! reaches today.

#![cfg(all(feature = "network-http", feature = "testing"))]

use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::common::linked_peer::link_contacts;
use crate::common::mock_relay::MockRelay;
use crate::common::signed_gateway::{DAY, NOW, SignedGateway};
use vauchi_core::api::{Vauchi, VauchiConfig, VauchiSyncOutcome};
use vauchi_core::clock::FakeClock;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::network::OhttpClient;
use vauchi_core::network::mailbox_token::{
    CATCH_UP_DAYS_PER_CYCLE, RETAINED_DAYS, compute_mailbox_token, current_day_epoch, token_hex,
};
use vauchi_core::network::{HttpTransport, HttpTransportConfig};
use vauchi_core::rng::OsSecureRng;
use vauchi_core::sleeper::FakeSleeper;
use vauchi_core::storage::ReceiveScope;
use vauchi_protocol::ohttp_key::window_of;

const OFFLINE_DAYS: u64 = 100;

/// A client's durable half: its database and the key that opens it.
struct Install {
    dir: tempfile::TempDir,
    key: SymmetricKey,
}

/// Opens (or reopens) the client on `install`'s database.
fn open(
    install: &Install,
    outer: &MockRelay,
    application: &MockRelay,
    clock: Arc<FakeClock>,
) -> Vauchi {
    let config = VauchiConfig::with_storage_path(install.dir.path().join("vauchi.db"))
        .with_storage_key(install.key.clone())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    let mut client = Vauchi::new_with(config, clock, OsSecureRng::shared(), None)
        .unwrap()
        .with_sleeper(Arc::new(FakeSleeper::new()));
    client.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).unwrap());
    client
}

fn client(
    outer: &MockRelay,
    application: &MockRelay,
    clock: Arc<FakeClock>,
    name: &str,
) -> (Vauchi, Install) {
    let install = Install {
        dir: tempfile::tempdir().unwrap(),
        key: SymmetricKey::generate(),
    };
    let mut client = open(&install, outer, application, clock);
    client.create_identity(name).unwrap();
    (client, install)
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

fn checkpoint(client: &Vauchi, application: &MockRelay) -> Option<u64> {
    let identity = client.identity().unwrap();
    let scope = ReceiveScope::new(
        &application.url(),
        identity.signing_public_key(),
        identity.device_id(),
    );
    client
        .storage()
        .sync()
        .load_receive_checkpoint(&scope)
        .unwrap()
        .map(|checkpoint| checkpoint.day)
}

/// A reader that has never read these mailboxes. New identities now start
/// at their creation day (private#912), so the catch-up tests set this
/// state themselves: a checkpoint at day 0 plans like none, clamped to
/// the retention horizon and owing a resync.
fn as_if_never_synced(client: &Vauchi, application: &MockRelay) {
    let identity = client.identity().unwrap();
    let scope = ReceiveScope::new(
        &application.url(),
        identity.signing_public_key(),
        identity.device_id(),
    );
    client
        .storage()
        .sync()
        .save_receive_checkpoint(
            &scope,
            &vauchi_core::storage::ReceiveCheckpoint {
                day: 0,
                resync_owed: false,
            },
        )
        .unwrap();
}

// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_client_offline_100_days_catches_up_in_bounded_steps() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x63), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let (mut alice, _alice_install) = client(&outer, &application, clock.clone(), "Alice");
    as_if_never_synced(&alice, &application);
    let (mut bob, _bob_install) = client(&outer, &application, clock.clone(), "Bob");
    let linked = link_contacts(&alice, &bob, "Bob");
    let send_day = current_day_epoch(NOW);

    // Bob shares a field with Alice while she is away.
    let mut updated = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
    let field_id = field.id().to_string();
    updated.add_field(field).unwrap();
    bob.update_own_card(&updated).unwrap();
    bob.set_contact_visibility_override(&linked.host_contact_id, &field_id, true)
        .unwrap();
    let sent = sync(&mut bob);
    assert!(
        matches!(&sent, VauchiSyncOutcome::Ok { sent, .. } if *sent > 0),
        "{sent:?}"
    );

    clock.advance(Duration::from_secs(OFFLINE_DAYS * DAY));
    let today = send_day + OFFLINE_DAYS;
    assert_eq!(checkpoint(&alice, &application), Some(0));

    // Each sync takes a bounded chunk of the retained interval, oldest
    // first, and records how far it read; the update arrives in the cycle
    // whose chunk covers the day it was sent.
    let horizon = today - RETAINED_DAYS;
    // The first chunk covers CATCH_UP_DAYS_PER_CYCLE days from the horizon;
    // every later chunk re-reads one day for rotation skew, so it advances
    // the checkpoint by two days less than it covers.
    let first_end = horizon + CATCH_UP_DAYS_PER_CYCLE - 1;
    let step = CATCH_UP_DAYS_PER_CYCLE - 2;
    let cycles_until_sent = if send_day <= first_end {
        1
    } else {
        1 + (send_day - first_end).div_ceil(step)
    };
    let mut cycles = 0;
    let fields = |alice: &Vauchi| {
        alice
            .get_contact(&linked.peer_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .iter()
            .map(|field| field.value().to_string())
            .collect::<Vec<_>>()
    };
    while fields(&alice).is_empty() {
        cycles += 1;
        assert!(
            cycles <= cycles_until_sent,
            "no update after {cycles} syncs"
        );
        sync(&mut alice);
        let expected_checkpoint = (first_end + (cycles - 1) * step).min(today);
        assert_eq!(
            checkpoint(&alice, &application),
            Some(expected_checkpoint),
            "after sync {cycles}"
        );
    }
    assert_eq!(cycles, cycles_until_sent);
    assert_eq!(fields(&alice), vec!["bob@example.invalid"]);

    // The remaining chunks bring the checkpoint to today, then it stays.
    while checkpoint(&alice, &application) != Some(today) {
        cycles += 1;
        assert!(cycles <= RETAINED_DAYS / CATCH_UP_DAYS_PER_CYCLE + 2);
        sync(&mut alice);
    }
    sync(&mut alice);
    assert_eq!(checkpoint(&alice, &application), Some(today));

    // The relay no longer holds the envelope: Alice acknowledged it.
    let mut probe = HttpTransport::new(HttpTransportConfig::for_testing(outer.url(), 2_000));
    probe.set_ohttp(OhttpClient::new(outer.ohttp_key_config()).unwrap());
    let old_token = token_hex(&compute_mailbox_token(
        linked.shared_secret.as_bytes(),
        alice.identity().unwrap().signing_public_key(),
        send_day,
    ));
    assert!(probe.fetch(&[old_token]).unwrap().is_empty());
}

/// Bob shares a field, then revokes it a month later, both while Alice is
/// away; her catch-up applies the share before the revocation, keeps its
/// place across a restart, and ends with the field gone.
// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_revocation_sent_while_offline_lands_after_the_share_and_survives_a_restart() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x64), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let (alice, alice_install) = client(&outer, &application, clock.clone(), "Alice");
    as_if_never_synced(&alice, &application);
    let (mut bob, _bob_install) = client(&outer, &application, clock.clone(), "Bob");
    let linked = link_contacts(&alice, &bob, "Bob");
    let share_day = current_day_epoch(NOW);

    let mut updated = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
    let field_id = field.id().to_string();
    updated.add_field(field).unwrap();
    bob.update_own_card(&updated).unwrap();
    bob.set_contact_visibility_override(&linked.host_contact_id, &field_id, true)
        .unwrap();
    sync(&mut bob);

    clock.advance(Duration::from_secs(30 * DAY));
    let revoke_day = share_day + 30;
    bob.set_contact_visibility_override(&linked.host_contact_id, &field_id, false)
        .unwrap();
    let revoked = sync(&mut bob);
    assert!(
        matches!(&revoked, VauchiSyncOutcome::Ok { sent, .. } if *sent > 0),
        "{revoked:?}"
    );

    clock.advance(Duration::from_secs(70 * DAY));
    let today = share_day + OFFLINE_DAYS;
    let fields = |alice: &Vauchi| {
        alice
            .get_contact(&linked.peer_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .iter()
            .map(|field| field.value().to_string())
            .collect::<Vec<_>>()
    };

    // The share's day is in the second chunk, the revocation's in the fifth.
    let mut alice = alice;
    for _ in 0..3 {
        sync(&mut alice);
    }
    assert_eq!(fields(&alice), vec!["bob@example.invalid"]);
    let resumed_from = checkpoint(&alice, &application).unwrap();
    assert!(resumed_from < revoke_day);

    drop(alice);
    let mut alice = open(&alice_install, &outer, &application, clock.clone());
    assert_eq!(checkpoint(&alice, &application), Some(resumed_from));
    let mut cycles = 0;
    while checkpoint(&alice, &application) != Some(today) {
        cycles += 1;
        assert!(cycles <= RETAINED_DAYS / (CATCH_UP_DAYS_PER_CYCLE - 2) + 2);
        sync(&mut alice);
    }

    assert_eq!(fields(&alice), Vec::<String>::new());
    let mut probe = HttpTransport::new(HttpTransportConfig::for_testing(outer.url(), 2_000));
    probe.set_ohttp(OhttpClient::new(outer.ohttp_key_config()).unwrap());
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    for day in [share_day, revoke_day] {
        let token = token_hex(&compute_mailbox_token(
            linked.shared_secret.as_bytes(),
            &alice_pk,
            day,
        ));
        assert!(
            probe.fetch(&[token]).unwrap().is_empty(),
            "day {day}'s mailbox must be acknowledged"
        );
    }
}

/// A pass the relay cuts short with a rate limit has not read every
/// planned mailbox, so the checkpoint stays; the next full pass moves it.
// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_rate_limited_pass_does_not_move_the_checkpoint() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x65), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let (mut alice, _alice_install) = client(&outer, &application, clock.clone(), "Alice");
    as_if_never_synced(&alice, &application);
    // Three contacts: sixteen planned days times seven tokens is more than
    // one fetch chunk, so the second chunk can hit the limit.
    let (mut bob, _bob_install) = client(&outer, &application, clock.clone(), "Bob");
    let (carol, _carol_install) = client(&outer, &application, clock.clone(), "Carol");
    let (dave, _dave_install) = client(&outer, &application, clock.clone(), "Dave");
    let linked_bob = link_contacts(&alice, &bob, "Bob");
    link_contacts(&alice, &carol, "Carol");
    link_contacts(&alice, &dave, "Dave");
    let mut updated = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
    let field_id = field.id().to_string();
    updated.add_field(field).unwrap();
    bob.update_own_card(&updated).unwrap();
    bob.set_contact_visibility_override(&linked_bob.host_contact_id, &field_id, true)
        .unwrap();
    sync(&mut bob);
    let send_day = current_day_epoch(NOW);

    // Away long enough that the send day falls in the first planned chunk,
    // so the served chunks can hold the blob.
    let offline_days = RETAINED_DAYS - 10;
    clock.advance(Duration::from_secs(offline_days * DAY));
    let today = send_day + offline_days;
    let horizon = today - RETAINED_DAYS;
    assert!(send_day < horizon + CATCH_UP_DAYS_PER_CYCLE);

    // 256 registered tokens fetch in three chunks; the third hits the limit.
    outer.limit_gateway_fetches(2);
    alice.set_next_sync_allowed(Instant::now());
    let limited = alice.sync();
    assert!(
        !matches!(&limited, Ok(VauchiSyncOutcome::Ok { errors, .. }) if errors.is_empty() && checkpoint(&alice, &application).is_some()),
        "a cut-short pass must not look complete: {limited:?}"
    );
    assert_eq!(checkpoint(&alice, &application), Some(0));

    outer.lift_gateway_fetch_limit();
    sync(&mut alice);
    assert_eq!(
        checkpoint(&alice, &application),
        Some(horizon + CATCH_UP_DAYS_PER_CYCLE - 1)
    );
    let saved = alice
        .get_contact(&linked_bob.peer_contact_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        saved
            .card()
            .fields()
            .iter()
            .map(|field| field.value())
            .collect::<Vec<_>>(),
        vec!["bob@example.invalid"]
    );
}

/// What converges by itself after an expiry gap: the updates Bob sent
/// while Alice was away longer than retention are gone, and nothing lets
/// her ask for them — but Bob's next edit repropagates his whole visible
/// card from an empty baseline, and the ratchet's skipped-key window
/// absorbs the lost messages, so the card converges then.
// @scenario: message_delivery :: Message expires after TTL
#[test]
fn after_an_expiry_gap_the_senders_next_edit_converges_the_card() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x66), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let (mut alice, _alice_install) = client(&outer, &application, clock.clone(), "Alice");
    let (mut bob, _bob_install) = client(&outer, &application, clock.clone(), "Bob");
    let linked = link_contacts(&alice, &bob, "Bob");
    let share = |bob: &mut Vauchi, label: &str, value: &str| {
        let mut updated = bob.own_card().unwrap().unwrap().clone();
        let field = ContactField::new(FieldType::Custom, label, value, 0);
        let field_id = field.id().to_string();
        updated.add_field(field).unwrap();
        bob.update_own_card(&updated).unwrap();
        bob.set_contact_visibility_override(&linked.host_contact_id, &field_id, true)
            .unwrap();
    };
    let fields = |alice: &Vauchi| {
        let mut values = alice
            .get_contact(&linked.peer_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .iter()
            .map(|field| field.value().to_string())
            .collect::<Vec<_>>();
        values.sort();
        values
    };

    share(&mut bob, "first", "one");
    sync(&mut bob);
    clock.advance(Duration::from_secs(DAY));
    share(&mut bob, "second", "two");
    sync(&mut bob);

    // Away longer than retention: both updates expire unread.
    clock.advance(Duration::from_secs((RETAINED_DAYS + 10) * DAY));
    let today = current_day_epoch(NOW) + 1 + RETAINED_DAYS + 10;
    let mut cycles = 0;
    while checkpoint(&alice, &application) != Some(today) {
        cycles += 1;
        assert!(cycles <= RETAINED_DAYS / (CATCH_UP_DAYS_PER_CYCLE - 2) + 2);
        sync(&mut alice);
    }
    assert_eq!(
        fields(&alice),
        Vec::<String>::new(),
        "nothing survived the gap"
    );

    // Bob's next edit carries his whole visible card.
    clock.advance(Duration::from_secs(DAY));
    share(&mut bob, "third", "three");
    sync(&mut bob);
    let collected = sync(&mut alice);
    assert!(
        matches!(
            &collected,
            VauchiSyncOutcome::Ok {
                received: 1,
                unresolved: 0,
                ..
            }
        ),
        "{collected:?}"
    );
    assert_eq!(fields(&alice), vec!["one", "three", "two"]);
}

/// Bob never edits after the share Alice missed; when her catch-up that
/// started behind retention completes, she asks him to repropagate, he
/// does on his next sync, and she has his card on hers.
// @scenario: message_delivery :: A client away longer than retention asks its contacts for their cards
#[test]
fn a_catch_up_that_started_behind_retention_ends_with_a_resync_request() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    outer.enable_ohttp_gateway(&SignedGateway::new(0x67), window_of(NOW));
    outer.enable_gateway_mailboxes(clock.clone(), 2);
    let (mut alice, _alice_install) = client(&outer, &application, clock.clone(), "Alice");
    let (mut bob, _bob_install) = client(&outer, &application, clock.clone(), "Bob");
    // Alice must be able to send first, so she links as the initiator:
    // `link_contacts` makes its host the responder.
    let linked = link_contacts(&bob, &alice, "Alice");
    let fields = |alice: &Vauchi| {
        alice
            .get_contact(&linked.host_contact_id)
            .unwrap()
            .unwrap()
            .card()
            .fields()
            .iter()
            .map(|field| field.value().to_string())
            .collect::<Vec<_>>()
    };

    let mut updated = bob.own_card().unwrap().unwrap().clone();
    let field = ContactField::new(FieldType::Email, "Email", "bob@example.invalid", 0);
    let field_id = field.id().to_string();
    updated.add_field(field).unwrap();
    bob.update_own_card(&updated).unwrap();
    bob.set_contact_visibility_override(&linked.peer_contact_id, &field_id, true)
        .unwrap();
    sync(&mut bob);

    clock.advance(Duration::from_secs((RETAINED_DAYS + 10) * DAY));
    let today = current_day_epoch(NOW) + RETAINED_DAYS + 10;
    let mut cycles = 0;
    while checkpoint(&alice, &application) != Some(today) {
        cycles += 1;
        assert!(cycles <= RETAINED_DAYS / (CATCH_UP_DAYS_PER_CYCLE - 2) + 2);
        sync(&mut alice);
    }
    assert_eq!(fields(&alice), Vec::<String>::new(), "the share expired");

    // Bob's next sync receives the request and sends his card; Alice's
    // next sync has it.
    // The request travels with F4 registry-handshake traffic, so count at
    // least it going in and at least his card going out.
    let honored = sync(&mut bob);
    assert!(
        matches!(&honored, VauchiSyncOutcome::Ok { received, sent, .. } if *received >= 1 && *sent > 0),
        "{honored:?}"
    );
    let answered = sync(&mut alice);
    assert!(
        matches!(
            &answered,
            VauchiSyncOutcome::Ok {
                received,
                unresolved: 0,
                ..
            } if *received >= 1
        ),
        "{answered:?}"
    );
    // (Bob also fans a device-scoped copy out to the device Alice's
    // registry push named; she holds no session for it yet — the F4
    // one-sided activation, outside this issue.)
    assert_eq!(fields(&alice), vec!["bob@example.invalid"]);

    // Asked once and honored once today; the handshake tail that follows
    // carries no further request.
    assert_eq!(
        bob.storage()
            .sync()
            .load_resync_honored_day(&linked.peer_contact_id)
            .unwrap(),
        Some(today)
    );
    let alice_identity = alice.identity().unwrap();
    let scope = ReceiveScope::new(
        &application.url(),
        alice_identity.signing_public_key(),
        alice_identity.device_id(),
    );
    let state = alice
        .storage()
        .sync()
        .load_receive_checkpoint(&scope)
        .unwrap()
        .unwrap();
    assert_eq!((state.day, state.resync_owed), (today, false));
}

/// Nothing can be addressed to an identity before it exists, so its first
/// sync owes no catch-up: since #890 each owed day costs a padded batch
/// and its fetches, and sixteen of them tripped the OHTTP relay's
/// per-address limit on a fresh install (private#912).
// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_new_identity_starts_reading_its_mailboxes_from_the_day_it_was_created() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x63), window_of(NOW));
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));

    let (client, _install) = client(&outer, &application, clock, "Alice");

    assert_eq!(
        checkpoint(&client, &application),
        Some(current_day_epoch(NOW))
    );
}

/// An identity brought onto a new install may have mail waiting from
/// before, so it keeps the full catch-up (private#691).
// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn an_existing_identity_on_a_new_install_still_catches_up() {
    let outer = MockRelay::start();
    let application = MockRelay::start();
    outer.enable_ohttp_gateway(&SignedGateway::new(0x63), window_of(NOW));
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    let (original, _original_install) = client(&outer, &application, clock.clone(), "Alice");
    let identity = vauchi_core::Identity::from_storage_bytes(
        &original.identity().unwrap().to_storage_bytes(),
        NOW,
    )
    .unwrap();
    let install = Install {
        dir: tempfile::tempdir().unwrap(),
        key: SymmetricKey::generate(),
    };
    let mut restored = open(&install, &outer, &application, clock);

    restored.set_identity(identity).unwrap();

    assert_eq!(checkpoint(&restored, &application), None);
}
