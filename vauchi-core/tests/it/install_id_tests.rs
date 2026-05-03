// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `vauchi_core::install_id`.

use std::fs;
use std::io;

use tempfile::tempdir;
use uuid::Uuid;
use vauchi_core::install_id::read_or_create_install_id;

const INSTALL_ID_FILE: &str = "install_id";

// @internal
#[test]
fn creates_new_uuid_on_first_call() {
    let dir = tempdir().unwrap();
    let id = read_or_create_install_id(dir.path()).unwrap();
    assert!(Uuid::parse_str(&id).is_ok());
    assert!(dir.path().join(INSTALL_ID_FILE).exists());
}

// @internal
#[test]
fn persists_across_calls() {
    let dir = tempdir().unwrap();
    let id1 = read_or_create_install_id(dir.path()).unwrap();
    let id2 = read_or_create_install_id(dir.path()).unwrap();
    assert_eq!(id1, id2);
}

// @internal
#[test]
fn distinct_dirs_get_distinct_ids() {
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let id_a = read_or_create_install_id(dir_a.path()).unwrap();
    let id_b = read_or_create_install_id(dir_b.path()).unwrap();
    assert_ne!(id_a, id_b);
}

// @internal
#[test]
fn creates_data_dir_if_missing() {
    let parent = tempdir().unwrap();
    let nested = parent.path().join("nested").join("data");
    assert!(!nested.exists());
    let id = read_or_create_install_id(&nested).unwrap();
    assert!(Uuid::parse_str(&id).is_ok());
    assert!(nested.exists());
}

// @internal
#[test]
fn invalid_uuid_in_file_returns_error() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(INSTALL_ID_FILE), b"not-a-uuid").unwrap();
    let err = read_or_create_install_id(dir.path()).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

// @internal
#[test]
fn id_survives_data_dir_rename() {
    // Regression: pre-fix, frontends derived OS-keychain entry names from
    // an FNV-1a hash of `data_dir`, so renaming the data directory orphaned
    // the keychain entry. install_id moves with the data — rename is now a
    // no-op for keychain lookup.
    let parent = tempdir().unwrap();
    let original = parent.path().join("original");
    fs::create_dir_all(&original).unwrap();
    let id_before = read_or_create_install_id(&original).unwrap();

    let renamed = parent.path().join("renamed");
    fs::rename(&original, &renamed).unwrap();

    let id_after = read_or_create_install_id(&renamed).unwrap();
    assert_eq!(id_before, id_after);
}

#[cfg(unix)]
// @internal
#[test]
fn install_id_file_has_0600_perms_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let _ = read_or_create_install_id(dir.path()).unwrap();
    let perms = fs::metadata(dir.path().join(INSTALL_ID_FILE))
        .unwrap()
        .permissions();
    assert_eq!(perms.mode() & 0o777, 0o600);
}
