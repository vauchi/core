// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(feature = "testing")]

use vauchi_core::api::storage_reset::delete_unreadable_data;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage};

#[test]
fn a_database_path_does_not_authorize_emergency_directory_deletion() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = root.path().join("unrelated");
    std::fs::write(&sentinel, b"keep").unwrap();
    let config = VauchiConfig::with_storage_path(root.path().join("custom.db"))
        .with_storage_key(SymmetricKey::generate());
    let mut app = Vauchi::new(config).unwrap();
    app.create_identity("Fixture").unwrap();
    assert!(matches!(
        app.perform_emergency_wipe(true),
        Err(vauchi_core::VauchiError::Configuration(_))
    ));
    assert_eq!(std::fs::read(sentinel).unwrap(), b"keep");
    assert!(app.identity().is_some());
}

#[test]
fn database_only_reset_preserves_neighboring_identity_and_key_files() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("custom.db");
    std::fs::write(&database, b"unreadable").unwrap();
    std::fs::write(root.path().join("identity.json"), b"neighbor identity").unwrap();
    std::fs::create_dir(root.path().join("keys")).unwrap();
    std::fs::write(root.path().join("keys/neighbor"), b"neighbor key").unwrap();
    let keys = MemoryKeyStorage::new();
    keys.save_key("smk", &[17; 32]).unwrap();
    delete_unreadable_data(&database, &keys).unwrap();
    assert!(!database.exists());
    assert_eq!(keys.load_key("smk").unwrap(), None);
    assert_eq!(
        std::fs::read(root.path().join("identity.json")).unwrap(),
        b"neighbor identity"
    );
    assert_eq!(
        std::fs::read(root.path().join("keys/neighbor")).unwrap(),
        b"neighbor key"
    );
}
