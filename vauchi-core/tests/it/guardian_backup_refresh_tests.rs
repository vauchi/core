// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The guardian backup is sent again when what it holds changed, at most
//! once a day (vauchi/private#796 step 1e, D11), and never from a duress
//! session (D12, ADR-032).

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use vauchi_core::api::ReceiveOutcome;
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::RecoverySettings;
use vauchi_core::{AuthMode, SymmetricKey, Vauchi};

use crate::common::two_recipient::{Recipient, add_recipient, receive};

const NOW: u64 = 1_700_000_000;
const HOUR: Duration = Duration::from_secs(3_600);
const DAY: Duration = Duration::from_secs(86_400);

fn alice_with_three_guardians() -> (Vauchi, Arc<FakeClock>, Vec<Recipient>) {
    let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(NOW)));
    let mut alice = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    alice.create_identity("Alice").unwrap();
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let mut guardians: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }
    for guardian in &mut guardians {
        receive(&alice, guardian);
    }
    (alice, clock, guardians)
}

fn add_a_contact(alice: &Vauchi) {
    let pk = *SigningKeyPair::generate().public_key().as_bytes();
    alice
        .add_contact(Contact::from_exchange(
            pk,
            ContactCard::new("Carol"),
            SymmetricKey::generate(),
            0,
        ))
        .unwrap();
}

/// How many backup pieces reached `guardian` since it last received.
fn pieces_received(alice: &Vauchi, guardian: &mut Recipient) -> usize {
    receive(alice, guardian)
        .iter()
        .filter(|outcome| matches!(outcome, ReceiveOutcome::GuardianBackupPiece(_)))
        .count()
}

fn kept_at(guardian: &Recipient) -> u64 {
    guardian
        .vauchi()
        .storage()
        .guardian_backups()
        .kept(guardian.sharer_id())
        .unwrap()
        .expect("a kept backup")
        .created_at()
}

// @internal
#[test]
fn a_changed_backup_is_sent_again_once_a_day_has_passed_and_not_before() {
    let (alice, clock, mut guardians) = alice_with_three_guardians();
    add_a_contact(&alice);
    clock.advance(HOUR);

    assert!(
        !alice.refresh_guardian_backup_if_stale().unwrap(),
        "within the day"
    );
    assert_eq!(pieces_received(&alice, &mut guardians[0]), 0);

    clock.advance(DAY);

    assert!(alice.refresh_guardian_backup_if_stale().unwrap());
    for guardian in &mut guardians {
        assert!(pieces_received(&alice, guardian) > 0);
        assert_eq!(kept_at(guardian), NOW + 3_600 + 86_400);
    }
}

// @internal
#[test]
fn an_unchanged_backup_is_not_sent_again() {
    let (alice, clock, mut guardians) = alice_with_three_guardians();
    clock.advance(DAY * 2);

    assert!(!alice.refresh_guardian_backup_if_stale().unwrap());
    assert_eq!(pieces_received(&alice, &mut guardians[0]), 0);
    assert_eq!(kept_at(&guardians[0]), NOW);
}

// @internal
#[test]
fn a_voucher_threshold_change_makes_the_backup_stale() {
    let (alice, clock, _guardians) = alice_with_three_guardians();
    alice
        .storage()
        .recovery()
        .save_recovery_settings(&RecoverySettings::new(2, 1).unwrap())
        .unwrap();
    clock.advance(DAY);

    assert!(alice.refresh_guardian_backup_if_stale().unwrap());
}

// @internal
#[test]
fn a_duress_session_never_refreshes_and_the_next_normal_one_does() {
    let (mut alice, clock, mut guardians) = alice_with_three_guardians();
    alice.setup_app_password("normal-pin").unwrap();
    alice.setup_duress_password("112233").unwrap();
    add_a_contact(&alice);
    assert_eq!(alice.authenticate("112233").unwrap(), AuthMode::Duress);
    clock.advance(DAY * 2);

    assert!(!alice.refresh_guardian_backup_if_stale().unwrap());
    assert_eq!(pieces_received(&alice, &mut guardians[0]), 0);

    assert_eq!(alice.authenticate("normal-pin").unwrap(), AuthMode::Normal);
    assert!(alice.refresh_guardian_backup_if_stale().unwrap());
    assert!(pieces_received(&alice, &mut guardians[0]) > 0);
}
