// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::backup::{GuardianGroups, passphrase_layer};
use vauchi_core::sync::guardian_piece::{HandBackShare, split_backup_for_hand_back};
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey, Vauchi, VauchiConfig};

pub const PASSPHRASE: &str = "the owl flies at noon";

fn named(name: &str) -> Vauchi {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity(name).unwrap();
    phone
}

pub struct Saved {
    pub config: VauchiConfig,
    pub target: [u8; 32],
    pub temporary: [u8; 32],
    pub contact_id: String,
    pub contact_key: [u8; 32],
    pub owner: Vauchi,
    pub guardians: Vec<Vauchi>,
}

pub fn save_meeting(
    directory: &std::path::Path,
    shares: usize,
    partial_pieces: bool,
    passphrase: bool,
) -> Saved {
    let lost = named("Alice");
    let target = *lost.identity().unwrap().signing_public_key();
    let contact = Contact::from_exchange(
        *Identity::create("Dave", 0).signing_public_key(),
        ContactCard::new("Dave"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_owned();
    let contact_key = *contact.shared_key().unwrap().as_bytes();
    lost.add_contact(contact).unwrap();
    let guardians: Vec<_> = (0..3).map(|i| named(&format!("G{i}"))).collect();
    let groups = GuardianGroups::new(
        guardians
            .iter()
            .map(|g| vec![*g.identity().unwrap().signing_public_key()])
            .collect(),
    )
    .unwrap();
    let (backup, sealed) = lost.export_guardian_backup_for_groups(&groups).unwrap();
    let mut backup = hex::decode(backup).unwrap();
    if passphrase {
        let salt = [9; 16];
        let key = passphrase_layer::passphrase_key(PASSPHRASE, &salt).unwrap();
        backup = passphrase_layer::seal(&key, &salt, &backup).unwrap();
    }
    if partial_pieces {
        backup = vec![7; 90_000];
    }
    let pieces = split_backup_for_hand_back(&backup).unwrap();
    let config = VauchiConfig::with_storage_path(directory.join("phone.db"))
        .with_storage_key(SymmetricKey::generate());
    let temporary = {
        let phone = Vauchi::new(config.clone()).unwrap();
        let claim = phone.begin_recovery(&hex::encode(target)).unwrap();
        let temporary = *phone
            .parse_recovery_claim_b64(&claim)
            .unwrap()
            .new_pk()
            .as_bytes();
        for (i, guardian) in guardians.iter().enumerate().take(shares) {
            let resealed = guardian
                .respond_to_recovery(&sealed[i][0], &temporary)
                .unwrap();
            let hand_back = HandBackShare::new(1, resealed, pieces[0].hash()).unwrap();
            assert_eq!(phone.take_hand_back(&hand_back.encode()).unwrap(), i + 1);
        }
        let count = if partial_pieces { 1 } else { pieces.len() };
        for piece in pieces.iter().take(count) {
            phone.take_hand_back_piece(&piece.encode()).unwrap();
        }
        temporary
    };
    Saved {
        config,
        target,
        temporary,
        contact_id,
        contact_key,
        owner: lost,
        guardians,
    }
}
