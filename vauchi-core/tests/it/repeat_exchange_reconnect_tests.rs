// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A repeat exchange in which only one side still holds the other
//! (vauchi/private#1285). The side that holds nothing saves the exchange's
//! key; the side that still holds the contact keeps its old key unless its
//! person agreed to reconnect, and then takes the exchange's key and keeps
//! everything else it knows about the contact. Without that, the two hold
//! different keys and nothing either sends opens at the other.

use vauchi_core::Vauchi;
use vauchi_core::api::{CardUpdateError, VauchiError, process_single_card_update};

use crate::common::device_sync::device_signed_broadcast;
use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::{Meeting, deliver, exchange_material};
use crate::common::per_device::{Party, two_parties};

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple-reconnect";

fn restore_identity_backup(p: &Party) -> Vauchi {
    let backup = p.bob.export_backup(BACKUP_PASSWORD).unwrap();
    let mut restored = Vauchi::in_memory().unwrap();
    restored.import_backup(&backup, BACKUP_PASSWORD).unwrap();
    restored
}

fn rename_and_deliver(from: &Vauchi, name: &str, to_id: &str, to: &Vauchi, from_id: &str) {
    let mut card = from.own_card().unwrap().unwrap();
    card.set_display_name(name).unwrap();
    from.update_own_card(&card).unwrap();
    from.run_owed_repropagation().unwrap();
    from.queue_registry_pushes().unwrap();
    deliver(from, to_id, to, from_id);
}

fn name_at(holder: &Vauchi, id: &str) -> Option<String> {
    holder
        .get_contact(id)
        .unwrap()
        .map(|contact| contact.display_name().to_string())
}

fn shared_key_of(holder: &Vauchi, id: &str) -> Option<[u8; 32]> {
    holder
        .get_contact(id)
        .unwrap()
        .and_then(|contact| contact.shared_key().map(|key| *key.as_bytes()))
}

fn exchange_key(meeting: &Meeting) -> Option<[u8; 32]> {
    meeting
        .phone_at_guardian
        .shared_key()
        .map(|key| *key.as_bytes())
}

/// Alice still holds Bob; Bob's restored install holds nobody and
/// initiates. Alice's person said yes.
fn alice_reconnects_with_the_restored_install(p: &Party, restored: &Vauchi) -> (Meeting, String) {
    let meeting = exchange_material(&p.alice, restored, false);
    restored
        .save_exchanged_contact(&meeting.guardian_at_phone, &meeting.phone_ratchet, true)
        .unwrap();
    p.alice
        .reconnect_exchanged_contact(&meeting.phone_at_guardian, &meeting.guardian_ratchet, false)
        .unwrap();
    let alice_at_restored = meeting.guardian_at_phone.id().to_string();
    (meeting, alice_at_restored)
}

fn reject_ratchet_writes(phone: &Vauchi) {
    phone
        .storage()
        .connection()
        .execute_batch(
            "CREATE TEMP TRIGGER reject_reconnect_ratchet_insert BEFORE INSERT ON contact_ratchets
             BEGIN SELECT RAISE(ABORT, 'forced reconnect ratchet failure'); END;
             CREATE TEMP TRIGGER reject_reconnect_ratchet_update BEFORE UPDATE ON contact_ratchets
             BEGIN SELECT RAISE(ABORT, 'forced reconnect ratchet failure'); END;",
        )
        .unwrap();
}

// @internal
#[test]
fn a_phone_holds_a_contact_it_exchanged_with_and_no_other() {
    let p = two_parties();
    let stranger = create_vauchi_with_identity("Stranger");
    let stranger_at_alice = exchange_material(&p.alice, &stranger, true)
        .phone_at_guardian
        .id()
        .to_string();

    assert!(p.alice.holds_exchanged_contact(&p.bob_at_alice).unwrap());
    assert!(!p.alice.holds_exchanged_contact(&stranger_at_alice).unwrap());
}

// @internal
#[test]
fn a_restored_install_and_its_old_contact_exchange_updates_after_a_yes() {
    let p = two_parties();
    let restored = restore_identity_backup(&p);

    let (meeting, alice_at_restored) = alice_reconnects_with_the_restored_install(&p, &restored);

    assert_eq!(
        shared_key_of(&p.alice, &p.bob_at_alice),
        exchange_key(&meeting),
        "the contact that said yes must hold the exchange's key"
    );
    rename_and_deliver(
        &restored,
        "Robert",
        &alice_at_restored,
        &p.alice,
        &p.bob_at_alice,
    );
    assert_eq!(
        name_at(&p.alice, &p.bob_at_alice),
        Some("Robert".to_string())
    );
    rename_and_deliver(
        &p.alice,
        "Alicia",
        &p.bob_at_alice,
        &restored,
        &alice_at_restored,
    );
    assert_eq!(
        name_at(&restored, &alice_at_restored),
        Some("Alicia".to_string())
    );
}

// @internal
#[test]
fn a_removed_contact_and_the_one_who_kept_it_exchange_updates_after_a_yes() {
    let p = two_parties();
    assert!(p.alice.remove_contact(&p.bob_at_alice).unwrap());
    let meeting = exchange_material(&p.bob, &p.alice, false);
    p.alice
        .save_exchanged_contact(&meeting.guardian_at_phone, &meeting.phone_ratchet, true)
        .unwrap();

    p.bob
        .reconnect_exchanged_contact(&meeting.phone_at_guardian, &meeting.guardian_ratchet, false)
        .unwrap();

    rename_and_deliver(&p.alice, "Alicia", &p.bob_at_alice, &p.bob, &p.alice_at_bob);
    assert_eq!(name_at(&p.bob, &p.alice_at_bob), Some("Alicia".to_string()));
    rename_and_deliver(&p.bob, "Robert", &p.alice_at_bob, &p.alice, &p.bob_at_alice);
    assert_eq!(
        name_at(&p.alice, &p.bob_at_alice),
        Some("Robert".to_string())
    );
}

// @internal
#[test]
fn a_yes_keeps_what_the_person_knew_about_the_contact() {
    let p = two_parties();
    p.alice
        .set_contact_nickname(&p.bob_at_alice, "Bobby")
        .unwrap();
    p.alice
        .add_personal_note(&p.bob_at_alice, "met at the conference")
        .unwrap();
    let friends = p.alice.create_group("Friends").unwrap();
    p.alice
        .add_contact_to_group(friends.id(), &p.bob_at_alice)
        .unwrap();
    p.alice.verify_contact_fingerprint(&p.bob_at_alice).unwrap();
    let restored = restore_identity_backup(&p);

    alice_reconnects_with_the_restored_install(&p, &restored);

    assert_eq!(
        p.alice.get_contact_nickname(&p.bob_at_alice).unwrap(),
        Some("Bobby".to_string())
    );
    assert_eq!(
        p.alice.read_personal_note(&p.bob_at_alice).unwrap(),
        Some("met at the conference".to_string())
    );
    let groups: Vec<String> = p
        .alice
        .get_groups_for_contact(&p.bob_at_alice)
        .unwrap()
        .iter()
        .map(|group| group.name().to_string())
        .collect();
    assert_eq!(groups, vec!["Friends".to_string()]);
    assert!(
        p.alice
            .get_contact(&p.bob_at_alice)
            .unwrap()
            .unwrap()
            .is_fingerprint_verified(),
        "the identity key did not change, so its verification stands"
    );
}

// @internal
#[test]
fn a_blocked_contact_is_not_reconnected() {
    let p = two_parties();
    let old_key = shared_key_of(&p.alice, &p.bob_at_alice);
    p.alice.block_contact(&p.bob_at_alice).unwrap();
    let restored = restore_identity_backup(&p);
    let meeting = exchange_material(&p.alice, &restored, false);

    let refused = p.alice.reconnect_exchanged_contact(
        &meeting.phone_at_guardian,
        &meeting.guardian_ratchet,
        false,
    );

    assert!(
        matches!(refused, Err(VauchiError::ContactBlocked(ref id)) if id == &p.bob_at_alice),
        "got {refused:?}"
    );
    assert_eq!(shared_key_of(&p.alice, &p.bob_at_alice), old_key);
}

// @internal
#[test]
fn a_phone_that_holds_nothing_cannot_reconnect() {
    let p = two_parties();
    let restored = restore_identity_backup(&p);
    let meeting = exchange_material(&p.alice, &restored, false);

    let refused = restored.reconnect_exchanged_contact(
        &meeting.guardian_at_phone,
        &meeting.phone_ratchet,
        true,
    );

    assert!(
        matches!(refused, Err(VauchiError::ContactNotFound(ref id)) if id == meeting.guardian_at_phone.id()),
        "got {refused:?}"
    );
    assert_eq!(restored.list_contacts().unwrap().len(), 0);
}

// @internal
#[test]
fn a_reconnect_whose_ratchet_write_fails_leaves_the_old_key() {
    let p = two_parties();
    let old_key = shared_key_of(&p.alice, &p.bob_at_alice);
    let old_name = name_at(&p.alice, &p.bob_at_alice);
    let restored = restore_identity_backup(&p);
    let meeting = exchange_material(&p.alice, &restored, false);
    reject_ratchet_writes(&p.alice);

    let error = p
        .alice
        .reconnect_exchanged_contact(&meeting.phone_at_guardian, &meeting.guardian_ratchet, false)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("forced reconnect ratchet failure"),
        "got {error}"
    );
    assert_eq!(shared_key_of(&p.alice, &p.bob_at_alice), old_key);
    assert_eq!(name_at(&p.alice, &p.bob_at_alice), old_name);
}

fn devices_alice_accepts_for_bob(p: &Party) -> Vec<[u8; 32]> {
    p.alice
        .storage()
        .device()
        .load_contact_active_devices(&p.bob_at_alice)
        .unwrap()
        .iter()
        .map(|device| device.device_id)
        .collect()
}

/// The restored install's registry names only itself, and the contact's
/// store replaces: the rule a full backup without a registry already
/// follows (vauchi/private#1247; owner, 2026-11-19, for this path).
// @internal
#[test]
fn after_a_yes_the_contact_accepts_the_restored_install_as_the_only_device() {
    let p = two_parties();
    p.push_to_alice(&device_signed_broadcast(&p.bob)).unwrap();
    assert_eq!(devices_alice_accepts_for_bob(&p), vec![p.bob_device_id()]);
    let restored = restore_identity_backup(&p);
    let restored_device = *restored.identity().unwrap().device_id();
    let (_, alice_at_restored) = alice_reconnects_with_the_restored_install(&p, &restored);

    assert_eq!(restored.queue_registry_pushes().unwrap(), 1);
    deliver(&restored, &alice_at_restored, &p.alice, &p.bob_at_alice);

    assert_eq!(devices_alice_accepts_for_bob(&p), vec![restored_device]);
}

// @internal
#[test]
fn a_yes_drops_the_sessions_the_contact_held_for_single_devices() {
    let p = two_parties();
    let ratchets = p.alice.storage().ratchets();
    let (old_session, _) = ratchets
        .load_ratchet_state(&p.bob_at_alice)
        .unwrap()
        .unwrap();
    ratchets
        .save_ratchet_state_for_device(&p.bob_at_alice, &p.bob_device_id(), &old_session, false)
        .unwrap();
    assert_eq!(ratchets.peer_devices(&p.bob_at_alice).unwrap().len(), 2);
    let restored = restore_identity_backup(&p);

    alice_reconnects_with_the_restored_install(&p, &restored);

    assert_eq!(
        p.alice
            .storage()
            .ratchets()
            .peer_devices(&p.bob_at_alice)
            .unwrap(),
        vec![[0u8; 32]],
        "only the session shared by the whole identity is left"
    );
}

/// The backed-up device still holds the connection the reconnect
/// replaced. If it is lost or stolen, nothing it sends reaches the contact.
// @internal
#[test]
fn after_a_yes_the_backed_up_device_can_no_longer_update_the_contact() {
    let p = two_parties();
    let restored = restore_identity_backup(&p);
    alice_reconnects_with_the_restored_install(&p, &restored);
    let mut card = p.bob.own_card().unwrap().unwrap();
    card.set_display_name("Bob, old phone").unwrap();
    p.bob.update_own_card(&card).unwrap();
    p.bob.run_owed_repropagation().unwrap();
    p.bob.queue_registry_pushes().unwrap();
    let queued = p
        .bob
        .storage()
        .pending()
        .get_pending_updates(&p.alice_at_bob)
        .unwrap();
    assert!(
        !queued.is_empty(),
        "the old device must have sent something"
    );

    for update in &queued {
        let outcome = process_single_card_update(
            p.alice.identity().unwrap(),
            p.alice.storage(),
            &p.bob_at_alice,
            &update.payload,
        );
        assert!(
            matches!(outcome, Err(CardUpdateError::DecryptionFailed)),
            "got {outcome:?}"
        );
    }
    assert_eq!(
        name_at(&p.alice, &p.bob_at_alice),
        Some("Phone".to_string())
    );
}
