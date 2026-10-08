// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! An identity backup is length-checked at every field: the smallest real
//! backup (legacy layout, empty name) restores, and a plaintext whose
//! declared lengths overrun it is refused rather than read past its end
//! (vauchi/private#522).

use vauchi_core::Identity;
use vauchi_core::crypto::{derive_key_argon2id, encrypt};
use vauchi_core::identity::IdentityBackup;

const PASSWORD: &str = "correct horse battery staple";
const SEED: [u8; 32] = [0x5e; 32];

/// A v2 backup sealing `plaintext` exactly as `export_backup` would.
fn sealed(plaintext: &[u8]) -> IdentityBackup {
    let salt = [0x11u8; 16];
    let key = derive_key_argon2id(PASSWORD.as_bytes(), &salt).unwrap();
    let mut data = vec![0x02];
    data.extend_from_slice(&salt);
    data.extend_from_slice(&encrypt(&key, plaintext).unwrap());
    IdentityBackup::new(data)
}

fn restore(plaintext: &[u8]) -> Option<Identity> {
    Identity::import_backup(&sealed(plaintext), PASSWORD, 0).ok()
}

fn legacy(name: &str) -> Vec<u8> {
    [&(name.len() as u32).to_le_bytes(), name.as_bytes(), &SEED].concat()
}

fn with_device(name: &str, index: u32, device_name: &str) -> Vec<u8> {
    [
        legacy(name).as_slice(),
        &index.to_le_bytes(),
        &(device_name.len() as u32).to_le_bytes(),
        device_name.as_bytes(),
    ]
    .concat()
}

// @internal
#[test]
fn the_smallest_backup_is_93_bytes_and_restores() {
    let smallest = sealed(&legacy(""));
    assert_eq!(smallest.as_bytes().len(), 1 + 93);

    let identity = Identity::import_backup(&smallest, PASSWORD, 0).expect("restores");
    assert_eq!(identity.display_name(), "");
}

// @internal
#[test]
fn a_backup_restores_its_name_and_device() {
    let identity = restore(&with_device("Alice", 3, "Laptop")).expect("restores");

    assert_eq!(identity.display_name(), "Alice");
    assert_eq!(identity.device_info().device_index(), 3);
    assert_eq!(identity.device_info().device_name(), "Laptop");
}

// @internal
#[test]
fn a_plaintext_whose_lengths_overrun_it_is_refused() {
    let short_of_seed = &legacy("")[..35];
    let mut name_too_long = legacy("Alice");
    name_too_long[0] = 6;
    let full = with_device("Alice", 3, "Laptop");
    let device_name_cut = &full[..full.len() - 1];

    for (case, plaintext) in [
        ("short of the seed", short_of_seed),
        ("name longer than declared", name_too_long.as_slice()),
        ("device name cut short", device_name_cut),
    ] {
        assert!(restore(plaintext).is_none(), "{case}");
    }
}
