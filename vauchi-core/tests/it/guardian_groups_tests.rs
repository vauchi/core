// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Guardian groups: recovery needs one member from each group, members of
//! a group count as one, and every guardian may be needed
//! (vauchi/private#815).

use vauchi_core::backup::{GuardianGroups, KeyShardError};
use vauchi_core::{Contact, ContactCard, Vauchi, VauchiError};

fn alice() -> Vauchi {
    let mut alice = Vauchi::in_memory().unwrap();
    alice.create_identity("Alice Smith").unwrap();
    alice
        .add_contact(Contact::from_exchange(
            [7; 32],
            ContactCard::new("Bob"),
            vauchi_core::crypto::SymmetricKey::generate(),
            0,
        ))
        .unwrap();
    alice
}

fn guardians(count: usize) -> Vec<Vauchi> {
    (0..count)
        .map(|index| {
            let mut guardian = Vauchi::in_memory().unwrap();
            guardian.create_identity(&format!("G{index}")).unwrap();
            guardian
        })
        .collect()
}

fn signing_pk(wb: &Vauchi) -> [u8; 32] {
    *wb.identity().unwrap().signing_public_key()
}

fn recovering() -> (Vauchi, [u8; 32]) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice Recovered").unwrap();
    let pk = signing_pk(&wb);
    (wb, pk)
}

/// Alice's backup for groups {G0}, {G1, G2}, {G3}: the sealed shares per
/// group, and the guardians in that order.
struct GroupedBackup {
    backup_hex: String,
    sealed: Vec<Vec<Vec<u8>>>,
    guardians: Vec<Vauchi>,
}

fn grouped_backup() -> GroupedBackup {
    let guardians = guardians(4);
    let pk = |index: usize| signing_pk(&guardians[index]);
    let groups = GuardianGroups::new(vec![vec![pk(0)], vec![pk(1), pk(2)], vec![pk(3)]]).unwrap();
    let (backup_hex, sealed) = alice().export_guardian_backup_for_groups(&groups).unwrap();
    GroupedBackup {
        backup_hex,
        sealed,
        guardians,
    }
}

impl GroupedBackup {
    /// `responders` as (group, member) pairs re-seal their share to `to`.
    fn responses(&self, responders: &[(usize, usize)], to: &[u8; 32]) -> Vec<Vec<u8>> {
        let member_offsets = [0usize, 1, 3];
        responders
            .iter()
            .map(|&(group, member)| {
                self.guardians[member_offsets[group] + member]
                    .respond_to_recovery(&self.sealed[group][member], to)
                    .unwrap()
            })
            .collect()
    }
}

// @scenario: backup_recovery :: Recover with one guardian from each group
#[test]
fn one_member_from_each_group_recovers() {
    let backup = grouped_backup();
    let bytes = hex::decode(&backup.backup_hex).unwrap();
    assert_eq!(
        (bytes[1], bytes[2]),
        (3, 3),
        "one share per group, all needed"
    );
    assert_eq!(
        backup.sealed.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 2, 1],
        "one sealed share per member"
    );
    let (recovering, pk) = recovering();

    let envelope = recovering
        .recover_guardian_backup(
            &backup.backup_hex,
            &backup.responses(&[(0, 0), (1, 1), (2, 0)], &pk),
        )
        .unwrap();

    assert_eq!(envelope.sections.identity.display_name, "Alice Smith");
}

// @scenario: backup_recovery :: Two guardians of one group count as one
#[test]
fn members_of_one_group_count_as_one() {
    let backup = grouped_backup();
    let (recovering, pk) = recovering();

    let only_two_groups = backup.responses(&[(0, 0), (1, 0), (1, 1)], &pk);
    let result = recovering.recover_guardian_backup(&backup.backup_hex, &only_two_groups);
    assert!(
        matches!(result, Err(VauchiError::Configuration(_))),
        "two members of a group are one group: {result:?}"
    );

    let all_groups_one_twice = backup.responses(&[(0, 0), (1, 0), (1, 1), (2, 0)], &pk);
    let envelope = recovering
        .recover_guardian_backup(&backup.backup_hex, &all_groups_one_twice)
        .unwrap();
    assert_eq!(envelope.sections.identity.display_name, "Alice Smith");
}

// @scenario: backup_recovery :: A whole group missing blocks recovery
#[test]
fn a_whole_group_missing_blocks_recovery() {
    let backup = grouped_backup();
    let (recovering, pk) = recovering();

    let group_two_silent = backup.responses(&[(0, 0), (1, 0), (1, 1)], &pk);
    let result = recovering.recover_guardian_backup(&backup.backup_hex, &group_two_silent);

    assert!(
        matches!(result, Err(VauchiError::Configuration(_))),
        "got {result:?}"
    );
}

// @scenario: backup_recovery :: A guardian backup may need every guardian
#[test]
fn all_of_three_needs_all_three() {
    let alice = alice();
    let guardians = guardians(3);
    let pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();
    let (backup_hex, sealed) = alice.export_guardian_backup_with_shards(&pks, 3).unwrap();
    let (recovering, pk) = recovering();
    let respond = |index: usize| {
        guardians[index]
            .respond_to_recovery(&sealed[index], &pk)
            .unwrap()
    };

    let two = recovering.recover_guardian_backup(&backup_hex, &[respond(0), respond(1)]);
    assert!(
        matches!(two, Err(VauchiError::Configuration(_))),
        "got {two:?}"
    );

    let envelope = recovering
        .recover_guardian_backup(&backup_hex, &[respond(0), respond(1), respond(2)])
        .unwrap();
    assert_eq!(envelope.sections.identity.display_name, "Alice Smith");
}

// @scenario: backup_recovery :: Recover with one guardian from each group
#[test]
fn nine_members_across_three_groups_is_the_largest_shape() {
    let guardians = guardians(9);
    let pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();
    let groups = GuardianGroups::new(vec![
        pks[0..3].to_vec(),
        pks[3..6].to_vec(),
        pks[6..9].to_vec(),
    ])
    .unwrap();
    let (backup_hex, sealed) = alice().export_guardian_backup_for_groups(&groups).unwrap();
    assert_eq!(sealed.iter().map(Vec::len).sum::<usize>(), 9);
    let (recovering, pk) = recovering();
    let respond = |group: usize, member: usize| {
        guardians[group * 3 + member]
            .respond_to_recovery(&sealed[group][member], &pk)
            .unwrap()
    };

    let envelope = recovering
        .recover_guardian_backup(&backup_hex, &[respond(0, 2), respond(1, 0), respond(2, 1)])
        .unwrap();

    assert_eq!(envelope.sections.identity.display_name, "Alice Smith");
}

fn key(byte: u8) -> [u8; 32] {
    [byte; 32]
}

// @internal
#[test]
fn three_groups_of_one_is_all_of_three() {
    let groups = GuardianGroups::new(vec![vec![key(1)], vec![key(2)], vec![key(3)]]).unwrap();

    assert_eq!(groups.config().threshold(), 3);
    assert_eq!(groups.config().count(), 3);
    assert_eq!(groups.groups().len(), 3);
}

// @internal
#[test]
fn shapes_outside_the_bounds_are_refused() {
    let two_groups = vec![vec![key(1)], vec![key(2)]];
    assert_eq!(
        GuardianGroups::new(two_groups),
        Err(KeyShardError::InvalidParameters)
    );

    let empty_group = vec![vec![key(1)], vec![], vec![key(3)]];
    assert_eq!(
        GuardianGroups::new(empty_group),
        Err(KeyShardError::InvalidParameters)
    );

    let ten_members = vec![
        (1..=4).map(key).collect(),
        (5..=8).map(key).collect(),
        vec![key(9), key(10)],
    ];
    assert_eq!(
        GuardianGroups::new(ten_members),
        Err(KeyShardError::InvalidParameters)
    );

    let ten_groups: Vec<Vec<[u8; 32]>> = (1..=10).map(|b| vec![key(b)]).collect();
    assert_eq!(
        GuardianGroups::new(ten_groups),
        Err(KeyShardError::InvalidParameters)
    );
}

// @internal
#[test]
fn a_guardian_in_two_groups_is_refused() {
    let twice = vec![vec![key(1)], vec![key(2), key(1)], vec![key(3)]];

    assert_eq!(
        GuardianGroups::new(twice),
        Err(KeyShardError::DuplicateGuardian)
    );
}
