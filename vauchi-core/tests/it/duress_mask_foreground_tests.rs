// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Under duress the foreground holds no real secret (ADR-080). These are the
//! acceptance tests for ADR-080 step 3: the duress foreground is the mask
//! profile's engine, and neither the real seed, contacts nor emergency
//! config are reachable from it (vauchi/private#1117).

use crate::common;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::{AuthMode, Vauchi};

use common::helpers::setup_alice_bob_exchange;

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-mask-9";
const REAL_ALERT: &str = "REAL-DISTRESS-MARKER";

struct DuressSession {
    vauchi: Vauchi,
    real_public_id: String,
    real_contact_id: String,
}

/// Alice has exchanged with Bob, listed him for the emergency broadcast and
/// set a decoy, then unlocks with her duress PIN.
fn duress_session() -> DuressSession {
    let (mut alice, _bob, _secret, bob_id, _alice_id) = setup_alice_bob_exchange();
    let real_public_id = alice.public_id().expect("public id");
    alice
        .setup_app_password("normal-pin-1234")
        .expect("app password");
    alice.setup_duress_password("112233").expect("duress PIN");
    alice
        .add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
        .expect("decoy");
    alice
        .configure_emergency_broadcast(vec![bob_id.clone()], REAL_ALERT.into(), false)
        .expect("emergency broadcast");
    assert_eq!(
        alice.authenticate("112233").expect("unlock"),
        AuthMode::Duress
    );
    DuressSession {
        vauchi: alice,
        real_public_id,
        real_contact_id: bob_id,
    }
}

fn restore_full(backup: &str) -> Vauchi {
    let mut restored = Vauchi::in_memory().expect("fresh install");
    restored
        .import_full_backup(backup, BACKUP_PASSWORD)
        .expect("a duress backup restores");
    restored
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn a_full_backup_made_under_duress_restores_the_decoys_not_the_real_identity() {
    let session = duress_session();

    let backup = session
        .vauchi
        .export_full_backup(BACKUP_PASSWORD)
        .expect("a duress export looks like it works");
    let restored = restore_full(&backup);

    assert_ne!(restored.public_id().expect("id"), session.real_public_id);
    assert!(
        restored
            .get_contact(&session.real_contact_id)
            .expect("lookup")
            .is_none()
    );
    let names: Vec<String> = restored
        .list_contacts()
        .expect("contacts")
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    assert_eq!(names, vec!["Decoy Dana".to_string()]);
}

// @scenario: duress_mode :: Real keys never loaded in duress mode
#[test]
fn an_identity_backup_made_under_duress_does_not_hold_the_real_seed() {
    let session = duress_session();

    let backup = session
        .vauchi
        .export_backup(BACKUP_PASSWORD)
        .expect("a duress export looks like it works");
    let mut restored = Vauchi::in_memory().expect("fresh install");
    restored
        .import_backup(&backup, BACKUP_PASSWORD)
        .expect("a duress backup restores");

    assert_ne!(restored.public_id().expect("id"), session.real_public_id);
}

// @scenario: duress_mode :: Settings reflect decoy state
#[test]
fn the_real_emergency_message_is_not_readable_under_duress() {
    let session = duress_session();

    let config = session.vauchi.load_emergency_config().expect("load");

    assert!(config.is_none_or(|c| c.message != REAL_ALERT));
}

// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn a_contact_listed_under_duress_can_be_archived() {
    let session = duress_session();
    let listed = session.vauchi.list_contacts().expect("contacts");
    let decoy_id = listed[0].id().to_string();

    session
        .vauchi
        .archive_contact(&decoy_id)
        .expect("archiving a listed contact works");

    assert!(
        session
            .vauchi
            .list_contacts()
            .expect("contacts")
            .iter()
            .all(|c| c.id() != decoy_id)
    );
}
