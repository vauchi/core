// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A full backup whose registry another identity signed restores a registry
//! naming only this device (ADR-079, private#970). The snapshot codec is
//! crate-private, which is why this test lives in the crate.

use crate::Vauchi;
use crate::backup::snapshot::{self, BackupSnapshot};
use crate::identity::{DeviceRegistry, Identity};

const PASSWORD: &str = "a-long-backup-passphrase-42";

/// Alice's full backup carrying `registry`, as an export made before
/// core!2413 could from a registry a previous identity left on the phone.
fn alices_backup_carrying(registry: DeviceRegistry) -> String {
    let mut phone = Vauchi::in_memory().unwrap();
    phone.create_identity("Alice").unwrap();
    let identity = phone.identity().unwrap();
    let mut backup = BackupSnapshot::capture(phone.storage(), identity, 0).unwrap();
    backup.device_registry = Some(registry);
    hex::encode(snapshot::encode(&backup, PASSWORD).unwrap())
}

// @internal
#[test]
fn a_backup_carrying_another_identitys_registry_restores_one_naming_only_this_device() {
    let mallory = Identity::create("Mallory", 0);
    let backup = alices_backup_carrying(mallory.initial_device_registry());

    let mut restored = Vauchi::in_memory().unwrap();
    restored.import_full_backup(&backup, PASSWORD).unwrap();

    let identity = restored.identity().unwrap();
    let registry = restored
        .storage()
        .device()
        .load_own_device_registry(identity)
        .unwrap()
        .unwrap();
    assert!(
        registry.find_device(mallory.device_id()).is_none(),
        "Mallory's device was re-signed into Alice's registry"
    );
    assert_eq!(registry.all_devices().len(), 1);
    assert!(registry.find_device(identity.device_id()).is_some());
}
