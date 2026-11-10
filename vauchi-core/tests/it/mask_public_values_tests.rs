// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mask profile holds the real public ID and name as public values, kept
//! current while the real profile is in the foreground, so a duress
//! foreground can show them without the real identity (ADR-080 point 4,
//! amended 2026-11-08; vauchi/private#1117).

use std::path::PathBuf;

use vauchi_core::api::AuthMode;
use vauchi_core::storage::Storage;
use vauchi_core::{SymmetricKey, Vauchi, VauchiConfig};

use crate::common::helpers::duress_mask_key;

const APP_PASSWORD: &str = "my-pin-1234";
const DURESS_PIN: &str = "654321";

struct OnDisk {
    _dir: tempfile::TempDir,
    path: PathBuf,
    key: SymmetricKey,
}

impl OnDisk {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        OnDisk {
            path: dir.path().join("vauchi.db"),
            key: SymmetricKey::generate(),
            _dir: dir,
        }
    }

    fn open(&self) -> Vauchi {
        let config = VauchiConfig::with_storage_path(&self.path).with_storage_key(self.key.clone());
        Vauchi::new(config).unwrap()
    }

    /// The public ID and name the mask profile, opened on its own, holds.
    fn mask_public_values(&self) -> Option<(String, String)> {
        let mask = Storage::open_mask_profile(&self.path, duress_mask_key(&self.key)).unwrap();
        let values = mask.mask_public_values().load().unwrap();
        values.map(|v| (v.public_id(), v.display_name().to_string()))
    }
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn the_mask_holds_the_real_public_id_and_the_name_after_a_rename() {
    let disk = OnDisk::new();
    let public_id = {
        let mut vauchi = disk.open();
        vauchi.create_identity("Alice").unwrap();
        vauchi.update_display_name("Alicia").unwrap();
        vauchi.public_id().unwrap()
    };

    assert_eq!(
        disk.mask_public_values(),
        Some((public_id, "Alicia".to_string()))
    );
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn a_profile_saved_without_public_values_gets_them_when_it_next_opens() {
    let disk = OnDisk::new();
    let public_id = {
        let mut vauchi = disk.open();
        vauchi.create_identity("Alice").unwrap();
        vauchi.public_id().unwrap()
    };
    {
        let storage = Storage::open(&disk.path, disk.key.clone()).unwrap();
        storage.mask_public_values().clear().unwrap();
    }
    assert_eq!(disk.mask_public_values(), None);

    drop(disk.open());

    assert_eq!(
        disk.mask_public_values(),
        Some((public_id, "Alice".to_string()))
    );
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn a_rename_under_duress_leaves_the_real_name_in_the_mask() {
    let disk = OnDisk::new();
    let public_id = {
        let mut vauchi = disk.open();
        vauchi.create_identity("Alice").unwrap();
        vauchi.setup_app_password(APP_PASSWORD).unwrap();
        vauchi
            .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
            .unwrap();
        vauchi.public_id().unwrap()
    };
    {
        let mut vauchi = disk.open();
        assert_eq!(vauchi.authenticate(DURESS_PIN).unwrap(), AuthMode::Duress);
        vauchi.update_display_name("Decoy Dana").unwrap();
    }

    assert_eq!(
        disk.mask_public_values(),
        Some((public_id, "Alice".to_string()))
    );
}

// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn an_emergency_wipe_leaves_the_mask_without_the_real_public_values() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let held = vauchi.storage().mask_public_values().load().unwrap();
    assert_eq!(
        held.map(|v| v.public_id()),
        Some(vauchi.public_id().unwrap())
    );

    vauchi.perform_emergency_wipe(true).unwrap();

    assert_eq!(vauchi.storage().mask_public_values().load().unwrap(), None);
}
