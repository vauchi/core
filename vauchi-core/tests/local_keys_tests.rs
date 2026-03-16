// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for shared local key management functions (T-M1).
//!
//! Feature: security.feature @keys @auth

use tempfile::TempDir;
use vauchi_core::storage::local_keys;

#[test]
fn test_load_or_generate_fallback_key_creates_new_key() {
    let dir = TempDir::new().unwrap();
    let key = local_keys::load_or_generate_fallback_key(dir.path()).unwrap();
    assert_eq!(key.as_bytes().len(), 32);
    assert!(
        key.as_bytes().iter().any(|&b| b != 0),
        "Generated key must not be all zeros"
    );
}

#[test]
fn test_load_or_generate_fallback_key_persists_across_calls() {
    let dir = TempDir::new().unwrap();
    let key1 = local_keys::load_or_generate_fallback_key(dir.path()).unwrap();
    let key2 = local_keys::load_or_generate_fallback_key(dir.path()).unwrap();
    assert_eq!(
        key1.as_bytes(),
        key2.as_bytes(),
        "Same key must be returned on subsequent calls"
    );
}

#[test]
fn test_load_or_generate_fallback_key_different_dirs_produce_different_keys() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();
    let key1 = local_keys::load_or_generate_fallback_key(dir1.path()).unwrap();
    let key2 = local_keys::load_or_generate_fallback_key(dir2.path()).unwrap();
    assert_ne!(
        key1.as_bytes(),
        key2.as_bytes(),
        "Different data directories must produce different keys"
    );
}

#[test]
fn test_load_or_generate_fallback_key_rejects_wrong_length() {
    let dir = TempDir::new().unwrap();
    let key_path = dir.path().join(".fallback-key");
    std::fs::write(&key_path, b"too short").unwrap();
    let result = local_keys::load_or_generate_fallback_key(dir.path());
    assert!(result.is_err(), "expected error");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Invalid fallback key length"),
        "Error should mention invalid length, got: {err_msg}"
    );
}

#[cfg(unix)]
#[test]
fn test_load_or_generate_fallback_key_sets_restrictive_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let _key = local_keys::load_or_generate_fallback_key(dir.path()).unwrap();
    let key_path = dir.path().join(".fallback-key");
    let perms = std::fs::metadata(&key_path).unwrap().permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o600,
        "Fallback key file must have 0600 permissions"
    );
}

#[test]
fn test_load_or_generate_backup_password_creates_new_password() {
    let dir = TempDir::new().unwrap();
    let password = local_keys::load_or_generate_backup_password(dir.path()).unwrap();
    assert_eq!(password.len(), 64, "Password must be 64 hex characters");
    assert!(
        password.chars().all(|c| c.is_ascii_hexdigit()),
        "Password must be valid hex"
    );
}

#[test]
fn test_load_or_generate_backup_password_persists_across_calls() {
    let dir = TempDir::new().unwrap();
    let pw1 = local_keys::load_or_generate_backup_password(dir.path()).unwrap();
    let pw2 = local_keys::load_or_generate_backup_password(dir.path()).unwrap();
    assert_eq!(
        pw1, pw2,
        "Same password must be returned on subsequent calls"
    );
}

#[test]
fn test_load_or_generate_backup_password_different_dirs() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();
    let pw1 = local_keys::load_or_generate_backup_password(dir1.path()).unwrap();
    let pw2 = local_keys::load_or_generate_backup_password(dir2.path()).unwrap();
    assert_ne!(
        pw1, pw2,
        "Different data directories must produce different passwords"
    );
}

#[test]
fn test_load_or_generate_backup_password_not_legacy() {
    let dir = TempDir::new().unwrap();
    let password = local_keys::load_or_generate_backup_password(dir.path()).unwrap();
    assert_ne!(
        password,
        local_keys::LEGACY_BACKUP_PASSWORD,
        "New installations must not use legacy password"
    );
}

#[test]
fn test_load_or_generate_backup_password_rejects_wrong_length() {
    let dir = TempDir::new().unwrap();
    let pw_path = dir.path().join(".backup-password");
    std::fs::write(&pw_path, "tooshort").unwrap();
    let result = local_keys::load_or_generate_backup_password(dir.path());
    assert!(result.is_err(), "expected error");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Invalid backup password length"),
        "Error should mention invalid length, got: {err_msg}"
    );
}

#[cfg(unix)]
#[test]
fn test_load_or_generate_backup_password_sets_restrictive_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let _pw = local_keys::load_or_generate_backup_password(dir.path()).unwrap();
    let pw_path = dir.path().join(".backup-password");
    let perms = std::fs::metadata(&pw_path).unwrap().permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o600,
        "Backup password file must have 0600 permissions"
    );
}

#[test]
fn test_keychain_key_name_produces_deterministic_hash() {
    use std::path::Path;
    let name1 = local_keys::keychain_key_name(Path::new("/data/vauchi"));
    let name2 = local_keys::keychain_key_name(Path::new("/data/vauchi"));
    assert_eq!(name1, name2, "Same path must produce same key name");
}

#[test]
fn test_keychain_key_name_different_paths_produce_different_hashes() {
    use std::path::Path;
    let name1 = local_keys::keychain_key_name(Path::new("/data/vauchi-1"));
    let name2 = local_keys::keychain_key_name(Path::new("/data/vauchi-2"));
    assert_ne!(
        name1, name2,
        "Different paths must produce different key names"
    );
}

#[test]
fn test_keychain_key_name_starts_with_prefix() {
    use std::path::Path;
    let name = local_keys::keychain_key_name(Path::new("/some/path"));
    assert!(
        name.starts_with("storage_key_"),
        "Key name must start with 'storage_key_', got: {name}"
    );
}

#[test]
fn test_keychain_key_name_is_valid_hex_suffix() {
    use std::path::Path;
    let name = local_keys::keychain_key_name(Path::new("/some/path"));
    let suffix = name.strip_prefix("storage_key_").unwrap();
    assert_eq!(suffix.len(), 16, "Hex suffix must be 16 characters");
    assert!(
        suffix.chars().all(|c| c.is_ascii_hexdigit()),
        "Suffix must be valid hex, got: {suffix}"
    );
}

#[test]
fn test_legacy_backup_password_constant() {
    assert_eq!(
        local_keys::LEGACY_BACKUP_PASSWORD,
        "vauchi-local-storage",
        "Legacy constant must match historical value"
    );
}
