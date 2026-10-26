// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every valid guardian grouping, by enumeration: for each ordered
//! composition of up to nine guardians into three to nine groups of one to
//! three, every one-member-per-group selection recovers the backup and
//! every selection that leaves a group out does not (vauchi/private#815).
//! The shape space is finite, so this is the implementation's proof by
//! cases; that fewer than all shares reveal nothing is Shamir's theorem.

use vauchi_core::backup::GuardianGroups;
use vauchi_core::{Contact, ContactCard, Vauchi, VauchiError};

const MAX_GUARDIANS: usize = 9;
const MIN_GROUPS: usize = 3;
const MAX_MEMBERS: usize = 3;

fn identity(name: &str) -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

fn signing_pk(wb: &Vauchi) -> [u8; 32] {
    *wb.identity().unwrap().signing_public_key()
}

/// Ordered compositions of `total` guardians into groups of `1..=MAX_MEMBERS`.
fn compositions(total: usize) -> Vec<Vec<usize>> {
    if total == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for first in 1..=MAX_MEMBERS.min(total) {
        for mut rest in compositions(total - first) {
            rest.insert(0, first);
            out.push(rest);
        }
    }
    out
}

fn every_valid_shape() -> Vec<Vec<usize>> {
    (MIN_GROUPS..=MAX_GUARDIANS)
        .flat_map(compositions)
        .filter(|shape| shape.len() >= MIN_GROUPS)
        .collect()
}

/// Every choice of one member index per group, in mixed-radix order.
fn one_per_group(shape: &[usize]) -> Vec<Vec<usize>> {
    shape.iter().fold(vec![Vec::new()], |choices, &size| {
        choices
            .iter()
            .flat_map(|choice| {
                (0..size).map(move |member| {
                    let mut next = choice.clone();
                    next.push(member);
                    next
                })
            })
            .collect()
    })
}

fn is_refused(result: Result<vauchi_core::backup::FullBackupEnvelope, VauchiError>) -> bool {
    matches!(result, Err(VauchiError::Configuration(_)))
}

// @scenario: backup_recovery :: Recover with one guardian from each group
// @scenario: backup_recovery :: A whole group missing blocks recovery
#[test]
fn every_shape_recovers_with_one_from_each_group_and_not_without() {
    let alice = identity("Alice Smith");
    alice
        .add_contact(Contact::from_exchange(
            [7; 32],
            ContactCard::new("Bob"),
            vauchi_core::crypto::SymmetricKey::generate(),
            0,
        ))
        .unwrap();
    let guardians: Vec<Vauchi> = (0..MAX_GUARDIANS)
        .map(|index| identity(&format!("G{index}")))
        .collect();
    let recovering = identity("Alice Recovered");
    let recovering_pk = signing_pk(&recovering);

    let shapes = every_valid_shape();
    assert!(shapes.len() > 100, "{} shapes", shapes.len());
    let mut recoveries = 0;
    let mut refusals = 0;
    for shape in &shapes {
        let mut next_guardian = 0;
        let groups: Vec<Vec<[u8; 32]>> = shape
            .iter()
            .map(|&size| {
                let members = (next_guardian..next_guardian + size)
                    .map(|index| signing_pk(&guardians[index]))
                    .collect();
                next_guardian += size;
                members
            })
            .collect();
        let group_offsets: Vec<usize> = shape
            .iter()
            .scan(0, |offset, &size| {
                let start = *offset;
                *offset += size;
                Some(start)
            })
            .collect();
        let groups = GuardianGroups::new(groups).unwrap_or_else(|e| panic!("{shape:?}: {e}"));
        let (backup_hex, sealed) = alice.export_guardian_backup_for_groups(&groups).unwrap();
        let respond = |group: usize, member: usize| {
            guardians[group_offsets[group] + member]
                .respond_to_recovery(&sealed[group][member], &recovering_pk)
                .unwrap()
        };

        for choice in one_per_group(shape) {
            let responses: Vec<Vec<u8>> = choice
                .iter()
                .enumerate()
                .map(|(group, &member)| respond(group, member))
                .collect();
            let envelope = recovering
                .recover_guardian_backup(&backup_hex, &responses)
                .unwrap_or_else(|e| panic!("{shape:?} with {choice:?}: {e}"));
            assert_eq!(envelope.sections.identity.display_name, "Alice Smith");
            recoveries += 1;
        }

        for silent_group in 0..shape.len() {
            // Everyone except that group answers — the most help a missing
            // group can get.
            let responses: Vec<Vec<u8>> = (0..shape.len())
                .filter(|&group| group != silent_group)
                .flat_map(|group| (0..shape[group]).map(move |member| (group, member)))
                .map(|(group, member)| respond(group, member))
                .collect();
            assert!(
                is_refused(recovering.recover_guardian_backup(&backup_hex, &responses)),
                "{shape:?} recovered without group {silent_group}"
            );
            refusals += 1;
        }
    }
    assert!(recoveries > shapes.len() && refusals >= 3 * shapes.len());
}
