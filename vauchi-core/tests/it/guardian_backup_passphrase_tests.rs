// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The owner may add a recovery passphrase (vauchi/private#796 step 1h,
//! D16): the guardian backup is then also sealed under an Argon2id key from
//! it, so guardians that collude get nothing without it. The right
//! passphrase opens, a wrong one does not, and clearing it lets the
//! guardians' shares alone open the next backup.

use std::collections::HashMap;

use vauchi_core::api::ReceiveOutcome;
use vauchi_core::sync::guardian_piece::{GuardianDelivery, PieceAssembly};
use vauchi_core::{Storage, SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

const PASSPHRASE: &str = "the owl flies at noon";

/// Alice with three guardians, each in a group of its own.
fn alice_and_guardians() -> (Vauchi, Vec<Recipient>) {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let guardians: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice
            .trust_for_recovery_in_group(&guardian.id_at_sharer, None)
            .unwrap();
    }
    (alice, guardians)
}

/// The newest whole delivery each guardian has received.
fn newest(alice: &Vauchi, guardians: &mut [Recipient]) -> Vec<GuardianDelivery> {
    guardians
        .iter_mut()
        .map(|guardian| {
            let mut assemblies: HashMap<[u8; 16], PieceAssembly> = HashMap::new();
            let mut whole = None;
            for outcome in receive(alice, guardian) {
                if let ReceiveOutcome::GuardianBackupPiece(piece) = outcome
                    && let Some(bytes) = assemblies
                        .entry(piece.backup_id())
                        .or_default()
                        .add(piece)
                        .unwrap()
                {
                    whole = Some(GuardianDelivery::decode(&bytes).unwrap());
                }
            }
            whole.expect("a whole backup")
        })
        .collect()
}

/// A new identity tries to open the backup with every guardian's share and
/// `passphrase`; returns whether it opened as Alice's.
fn opens(
    guardians: &[Recipient],
    deliveries: &[GuardianDelivery],
    passphrase: Option<&str>,
) -> bool {
    let recovering = create_vauchi_with_identity("Alice, new phone");
    let new_pk = *recovering.identity().unwrap().signing_public_key();
    let shares: Vec<Vec<u8>> = guardians
        .iter()
        .zip(deliveries)
        .map(|(guardian, delivery)| {
            guardian
                .vauchi()
                .respond_to_recovery(delivery.sealed_share(), &new_pk)
                .unwrap()
        })
        .collect();
    recovering
        .recover_guardian_backup_with_passphrase(
            &hex::encode(deliveries[0].backup()),
            &shares,
            passphrase,
        )
        .is_ok_and(|envelope| envelope.sections.identity.display_name == "Alice")
}

// @internal
#[test]
fn with_a_passphrase_the_guardians_shares_alone_open_nothing() {
    let (alice, mut guardians) = alice_and_guardians();
    alice.set_recovery_passphrase(PASSPHRASE).unwrap();

    let deliveries = newest(&alice, &mut guardians);

    assert!(alice.has_recovery_passphrase().unwrap());
    assert!(Vauchi::guardian_backup_needs_passphrase(&hex::encode(
        deliveries[0].backup()
    )));
    assert!(
        !opens(&guardians, &deliveries, None),
        "shares alone opened it"
    );
    assert!(!opens(
        &guardians,
        &deliveries,
        Some("the owl flies at nine")
    ));
    assert!(opens(&guardians, &deliveries, Some(PASSPHRASE)));
}

// @internal
#[test]
fn once_the_passphrase_is_cleared_the_shares_alone_open_the_next_backup() {
    let (alice, mut guardians) = alice_and_guardians();
    alice.set_recovery_passphrase(PASSPHRASE).unwrap();
    newest(&alice, &mut guardians);

    alice.clear_recovery_passphrase().unwrap();
    let deliveries = newest(&alice, &mut guardians);

    assert!(!alice.has_recovery_passphrase().unwrap());
    assert!(!Vauchi::guardian_backup_needs_passphrase(&hex::encode(
        deliveries[0].backup()
    )));
    assert!(opens(&guardians, &deliveries, None));
}

// @internal
#[test]
fn an_empty_passphrase_is_refused() {
    let (alice, _guardians) = alice_and_guardians();

    assert!(alice.set_recovery_passphrase("").is_err());
    assert!(!alice.has_recovery_passphrase().unwrap());
}

/// Guardians who collude hold the backup and every share, so they can
/// guess the passphrase offline: it must be as strong as a backup password.
// @internal
#[test]
fn a_weak_passphrase_is_refused_and_a_strong_one_kept() {
    let (alice, _guardians) = alice_and_guardians();

    for weak in ["password", "12345678", "alice", "qwertyuiop"] {
        assert!(alice.set_recovery_passphrase(weak).is_err(), "{weak}");
    }
    assert!(!alice.has_recovery_passphrase().unwrap());

    alice.set_recovery_passphrase(PASSPHRASE).unwrap();
    assert!(alice.has_recovery_passphrase().unwrap());
}

// @internal
#[test]
fn the_kept_passphrase_key_survives_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let key = SymmetricKey::generate();
    storage
        .recovery()
        .save_recovery_passphrase(&[7; 16], &key)
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    let (salt, kept) = storage
        .recovery()
        .load_recovery_passphrase()
        .unwrap()
        .unwrap();
    assert_eq!(salt, [7; 16]);
    assert_eq!(kept.as_bytes(), key.as_bytes());
}
