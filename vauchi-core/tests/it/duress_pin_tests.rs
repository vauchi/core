// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress PIN / App Password Tests
//!
//! TDD tests for the duress PIN authentication system:
//! - Password KDF (Argon2id hashing + constant-time verification)
//! - Password storage (save/load app password and duress password)
//! - Decoy contacts (CRUD for fake contacts shown in duress mode)
//! - Auth mode (authenticate → Normal/Duress/Unauthenticated)
//! - Mode-aware contact loading (real vs decoy based on auth mode)

use crate::common;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::storage::Storage;
use vauchi_core::{
    AppPasswordConfig, AuthMode, AuthResult, BiometricUnlockOutcome, Contact, VauchiError,
};

use common::helpers::{create_vauchi_with_identity, setup_alice_bob_exchange};

// =============================================================================
// =============================================================================

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn test_create_password_config_produces_valid_hash() {
    let config = AppPasswordConfig::create("secure-pin-1234").expect("create should succeed");

    assert_ne!(config.password_hash(), &[0u8; 32]);
    assert_ne!(config.password_salt(), &[0u8; 16]);

    assert!(!config.duress_enabled());
    assert!(config.duress_hash().is_none());
    assert!(config.duress_salt().is_none());
}

// @scenario: duress_mode :: Normal credential shows real contacts
#[test]
fn test_verify_correct_password_returns_normal() {
    let config = AppPasswordConfig::create("my-password").expect("create should succeed");

    let result = config.verify("my-password");
    assert!(
        matches!(result, AuthResult::Normal),
        "correct password should return Normal, got {:?}",
        result
    );
}

// @scenario: duress_mode :: Wrong credential handling
#[test]
fn test_verify_wrong_password_returns_invalid() {
    let config = AppPasswordConfig::create("my-password").expect("create should succeed");

    let result = config.verify("wrong-password");
    assert!(
        matches!(result, AuthResult::Invalid),
        "wrong password should return Invalid, got {:?}",
        result
    );
}

// @scenario: duress_mode :: Duress credential shows decoy contacts
#[test]
fn test_verify_duress_password_returns_duress() {
    let mut config = AppPasswordConfig::create("my-password").expect("create should succeed");
    config
        .setup_duress("duress-pin")
        .expect("setup duress should succeed");

    let result = config.verify("duress-pin");
    assert!(
        matches!(result, AuthResult::Duress),
        "duress password should return Duress, got {:?}",
        result
    );

    let result = config.verify("my-password");
    assert!(
        matches!(result, AuthResult::Normal),
        "normal password should still return Normal, got {:?}",
        result
    );
}

// @scenario: duress_mode :: Both databases use strong encryption
#[test]
fn test_verify_constant_time_both_checked() {
    // Both hashes should always be checked to prevent timing attacks.
    // We can't directly test timing, but we can verify that:
    // 1. When duress is enabled, a wrong password checks both hashes
    // 2. The function signature ensures both are evaluated

    let mut config = AppPasswordConfig::create("my-password").expect("create should succeed");
    config
        .setup_duress("duress-pin")
        .expect("setup duress should succeed");

    // Wrong password should still be Invalid (not Normal or Duress)
    let result = config.verify("completely-wrong");
    assert!(
        matches!(result, AuthResult::Invalid),
        "completely wrong password should return Invalid, got {:?}",
        result
    );
}

// @scenario: duress_mode :: Both databases use strong encryption
#[test]
fn test_create_different_passwords_produce_different_hashes() {
    let config1 = AppPasswordConfig::create("password-one").expect("create should succeed");
    let config2 = AppPasswordConfig::create("password-two").expect("create should succeed");

    // Different passwords should produce different hashes (different salts too)
    assert_ne!(config1.password_hash(), config2.password_hash());
}

// @scenario: duress_mode :: Duress PIN must differ from normal PIN
#[test]
fn test_setup_duress_same_as_normal_rejected() {
    // Duress password must differ from the normal password
    let mut config = AppPasswordConfig::create("my-password").expect("create should succeed");

    let result = config.setup_duress("my-password");
    assert!(
        result.is_err(),
        "setting duress password same as normal should be rejected"
    );
}

// =============================================================================
// Change Password — KDF-level (AppPasswordConfig)
// =============================================================================

// @internal
#[test]
fn test_change_password_rotates_salt_and_hash() {
    let mut config = AppPasswordConfig::create("old-password").expect("create");
    let old_hash = *config.password_hash();
    let old_salt = *config.password_salt();

    config.change_password("new-password").expect("rotate");

    assert_ne!(
        config.password_hash(),
        &old_hash,
        "password_hash must change after rotation"
    );
    assert_ne!(
        config.password_salt(),
        &old_salt,
        "password_salt must change after rotation (random regenerate)"
    );
}

// @internal
#[test]
fn test_change_password_old_no_longer_verifies() {
    let mut config = AppPasswordConfig::create("old-password").expect("create");
    config.change_password("new-password").expect("rotate");

    assert!(
        matches!(config.verify("old-password"), AuthResult::Invalid),
        "old password must no longer verify after rotation"
    );
    assert!(
        matches!(config.verify("new-password"), AuthResult::Normal),
        "new password must verify as Normal after rotation"
    );
}

// @internal
#[test]
fn test_change_password_preserves_duress_pin() {
    let mut config = AppPasswordConfig::create("old-password").expect("create");
    config.setup_duress("duress-pin").expect("setup duress");
    let duress_hash_before = *config.duress_hash().expect("duress configured");
    let duress_salt_before = *config.duress_salt().expect("duress configured");

    config.change_password("new-password").expect("rotate");

    // Duress hash and salt are byte-for-byte preserved
    assert_eq!(
        config.duress_hash().expect("still configured"),
        &duress_hash_before,
        "duress_hash must not change when only normal password rotates"
    );
    assert_eq!(
        config.duress_salt().expect("still configured"),
        &duress_salt_before,
        "duress_salt must not change when only normal password rotates"
    );
    assert!(config.duress_enabled());

    // Duress PIN still verifies as Duress
    assert!(
        matches!(config.verify("duress-pin"), AuthResult::Duress),
        "duress PIN must still authenticate after rotation"
    );
}

// @internal
#[test]
fn test_change_password_collision_with_duress_rejected() {
    // Maintains the setup_duress invariant: normal != duress.
    let mut config = AppPasswordConfig::create("old-password").expect("create");
    config.setup_duress("duress-pin").expect("setup duress");

    let result = config.change_password("duress-pin");
    assert!(
        result.is_err(),
        "rotating to a password equal to the configured duress PIN must be rejected"
    );

    // Config state is untouched on rejection
    assert!(
        matches!(config.verify("old-password"), AuthResult::Normal),
        "old password must still verify after a rejected rotation"
    );
}

// @internal
#[test]
fn test_change_password_no_duress_configured_works() {
    let mut config = AppPasswordConfig::create("old-password").expect("create");
    assert!(!config.duress_enabled());

    config.change_password("new-password").expect("rotate");
    assert!(
        matches!(config.verify("new-password"), AuthResult::Normal),
        "rotation works without duress configured"
    );
}

// =============================================================================
// =============================================================================

// @scenario: duress_mode :: Duress mode is opt-in and disabled by default
#[test]
fn test_load_password_config_returns_none_initially() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let config = storage
        .identity()
        .load_password_config()
        .expect("load should succeed");
    assert!(
        config.is_none(),
        "password config should be None before any password is set"
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn test_save_load_app_password_roundtrip() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    // Create and save identity first (password columns are on identity table)
    // Create identity row so password columns can be updated
    let backup_data = b"dummy-backup-data";
    storage
        .identity()
        .save_identity(backup_data, "Test User")
        .expect("save identity should succeed");

    let password_config =
        AppPasswordConfig::create("test-password").expect("create should succeed");
    storage
        .identity()
        .save_app_password(
            password_config.password_hash(),
            password_config.password_salt(),
        )
        .expect("save should succeed");

    let loaded = storage
        .identity()
        .load_password_config()
        .expect("load should succeed")
        .expect("should have password config");

    assert_eq!(loaded.password_hash(), password_config.password_hash());
    assert_eq!(loaded.password_salt(), password_config.password_salt());
    assert!(!loaded.duress_enabled());
}

// @scenario: duress_mode :: Enable duress PIN in settings
// @scenario: duress_mode :: App update preserves duress configuration
#[test]
fn test_save_load_duress_password_roundtrip() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    // Create identity row so password columns can be updated
    let backup_data = b"dummy-backup-data";
    storage
        .identity()
        .save_identity(backup_data, "Test User")
        .expect("save identity should succeed");

    let mut password_config =
        AppPasswordConfig::create("test-password").expect("create should succeed");
    storage
        .identity()
        .save_app_password(
            password_config.password_hash(),
            password_config.password_salt(),
        )
        .expect("save app password should succeed");

    password_config
        .setup_duress("duress-pin")
        .expect("setup duress should succeed");
    storage
        .identity()
        .save_duress_password(
            password_config
                .duress_hash()
                .expect("duress hash should exist"),
            password_config
                .duress_salt()
                .expect("duress salt should exist"),
        )
        .expect("save duress should succeed");

    let loaded = storage
        .identity()
        .load_password_config()
        .expect("load should succeed")
        .expect("should have password config");

    assert!(loaded.duress_enabled());
    assert_eq!(loaded.duress_hash(), password_config.duress_hash());
    assert_eq!(loaded.duress_salt(), password_config.duress_salt());
}

// @scenario: duress_mode :: Disable duress mode from settings
#[test]
fn test_disable_duress_clears_data() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    // Create identity row so password columns can be updated
    let backup_data = b"dummy-backup-data";
    storage
        .identity()
        .save_identity(backup_data, "Test User")
        .expect("save identity should succeed");

    let mut password_config =
        AppPasswordConfig::create("test-password").expect("create should succeed");
    storage
        .identity()
        .save_app_password(
            password_config.password_hash(),
            password_config.password_salt(),
        )
        .expect("save app password should succeed");
    password_config
        .setup_duress("duress-pin")
        .expect("setup duress should succeed");
    storage
        .identity()
        .save_duress_password(
            password_config.duress_hash().expect("duress hash"),
            password_config.duress_salt().expect("duress salt"),
        )
        .expect("save duress should succeed");

    storage
        .identity()
        .disable_duress()
        .expect("disable should succeed");

    let loaded = storage
        .identity()
        .load_password_config()
        .expect("load should succeed")
        .expect("should have password config");

    assert!(!loaded.duress_enabled());
    assert!(loaded.duress_hash().is_none());
    assert!(loaded.duress_salt().is_none());
}

// =============================================================================
// =============================================================================

// @scenario: duress_mode :: Duress mode is opt-in and disabled by default
#[test]
fn test_load_decoy_contacts_empty_initially() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let contacts = storage
        .decoy()
        .load_decoy_contacts()
        .expect("load should succeed");
    assert!(
        contacts.is_empty(),
        "decoy contacts should be empty initially"
    );
}

// @scenario: duress_mode :: Configure decoy contacts
// @scenario: duress_mode :: Decoy profile has separate database
#[test]
fn test_save_load_decoy_contact() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let card = ContactCard::new("Fake Alice");
    storage
        .decoy()
        .save_decoy_contact("decoy-1", "Fake Alice", &card)
        .expect("save should succeed");

    let contacts = storage
        .decoy()
        .load_decoy_contacts()
        .expect("load should succeed");
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].0, "decoy-1"); // id
    assert_eq!(contacts[0].1, "Fake Alice"); // display_name
    assert_eq!(contacts[0].2.display_name(), "Fake Alice"); // card
}

// @scenario: duress_mode :: Configure decoy contacts
#[test]
fn test_delete_decoy_contact() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let card = ContactCard::new("Fake Alice");
    storage
        .decoy()
        .save_decoy_contact("decoy-1", "Fake Alice", &card)
        .expect("save should succeed");

    storage
        .decoy()
        .delete_decoy_contact("decoy-1")
        .expect("delete should succeed");

    let contacts = storage
        .decoy()
        .load_decoy_contacts()
        .expect("load should succeed");
    assert!(
        contacts.is_empty(),
        "decoy contacts should be empty after delete"
    );
}

// @scenario: duress_mode :: Configure decoy contacts
#[test]
fn test_clear_all_decoy_contacts() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let card1 = ContactCard::new("Fake Alice");
    let card2 = ContactCard::new("Fake Bob");
    storage
        .decoy()
        .save_decoy_contact("decoy-1", "Fake Alice", &card1)
        .expect("save 1 should succeed");
    storage
        .decoy()
        .save_decoy_contact("decoy-2", "Fake Bob", &card2)
        .expect("save 2 should succeed");

    storage
        .decoy()
        .clear_all_decoy_contacts()
        .expect("clear should succeed");

    let contacts = storage
        .decoy()
        .load_decoy_contacts()
        .expect("load should succeed");
    assert!(
        contacts.is_empty(),
        "decoy contacts should be empty after clear"
    );
}

// =============================================================================
// =============================================================================

// @scenario: duress_mode :: Duress mode is opt-in and disabled by default
#[test]
fn test_unauthenticated_mode_default() {
    let wb = create_vauchi_with_identity("Alice");

    assert_eq!(
        wb.auth_mode(),
        AuthMode::Unauthenticated,
        "default auth mode should be Unauthenticated"
    );
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn test_setup_app_password() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");

    assert!(
        wb.is_password_enabled().expect("check should succeed"),
        "password should be enabled after setup"
    );
}

// @internal
#[test]
fn test_setup_app_password_refuses_to_clobber_existing() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("first-pin-1234")
        .expect("first setup should succeed");

    // A second setup must NOT silently overwrite — rotation goes through
    // change_app_password (which verifies the current password).
    let err = wb
        .setup_app_password("second-pin-9876")
        .expect_err("second setup must be rejected, not clobber");
    assert!(
        matches!(err, VauchiError::InvalidState(_)),
        "expected InvalidState, got {err:?}"
    );

    // Storage is untouched — the original password still authenticates,
    // the attempted overwrite does not.
    assert!(wb.authenticate("first-pin-1234").is_ok());
    assert!(wb.authenticate("second-pin-9876").is_err());
}

// @scenario: duress_mode :: Normal credential shows real contacts
#[test]
fn test_authenticate_normal_password() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");

    let mode = wb.authenticate("my-pin-1234").expect("auth should succeed");

    assert_eq!(
        mode,
        AuthMode::Normal,
        "correct password should set Normal mode"
    );
    assert_eq!(wb.auth_mode(), AuthMode::Normal);
}

// @scenario: duress_mode :: Wrong credential handling
#[test]
fn test_authenticate_invalid_password_fails() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");

    let result = wb.authenticate("wrong-pin");
    assert!(result.is_err(), "wrong password should return an error");
    assert_eq!(wb.auth_mode(), AuthMode::Unauthenticated);
}

// @scenario: duress_mode :: Duress credential shows decoy contacts
#[test]
fn test_authenticate_duress_password_sets_mode() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");
    wb.setup_duress_password("duress-999")
        .expect("setup duress should succeed");

    let mode = wb.authenticate("duress-999").expect("auth should succeed");

    assert_eq!(
        mode,
        AuthMode::Duress,
        "duress password should set Duress mode"
    );
    assert_eq!(wb.auth_mode(), AuthMode::Duress);
}

// @scenario: duress_mode :: Enable duress PIN in settings
#[test]
fn test_setup_duress_password_requires_app_password_first() {
    let mut wb = create_vauchi_with_identity("Alice");

    let result = wb.setup_duress_password("duress-999");
    assert!(
        result.is_err(),
        "setting up duress without app password should fail"
    );
}

// @scenario: duress_mode :: Disable duress mode from settings
#[test]
fn test_disable_duress() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");
    wb.setup_duress_password("duress-999")
        .expect("setup duress should succeed");

    assert!(wb.is_duress_enabled().expect("check should succeed"));

    wb.disable_duress().expect("disable should succeed");

    assert!(!wb.is_duress_enabled().expect("check should succeed"));
}

// =============================================================================
// Change App Password — Vauchi integration (round-trips through storage)
// =============================================================================

// @internal
#[test]
fn test_change_app_password_round_trip() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("old-pin-1234").expect("setup");

    wb.change_app_password("old-pin-1234", "new-pin-9876")
        .expect("rotate");

    let old_result = wb.authenticate("old-pin-1234");
    assert!(
        old_result.is_err(),
        "old password must not authenticate after change_app_password"
    );

    let new_mode = wb
        .authenticate("new-pin-9876")
        .expect("new password authenticates");
    assert_eq!(new_mode, AuthMode::Normal);
}

// @internal
#[test]
fn test_change_app_password_wrong_current_rejected_no_storage_change() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("old-pin-1234").expect("setup");

    let err = wb
        .change_app_password("WRONG", "new-pin-9876")
        .expect_err("wrong current password must error");
    let _ = err; // Not asserting variant — InvalidState is the contract.

    // Old still authenticates — storage was not mutated on rejection
    let mode = wb
        .authenticate("old-pin-1234")
        .expect("old still authenticates after rejected rotation");
    assert_eq!(mode, AuthMode::Normal);

    let new_result = wb.authenticate("new-pin-9876");
    assert!(
        new_result.is_err(),
        "new password must not authenticate after rejected rotation"
    );
}

// @internal
#[test]
fn test_change_app_password_no_existing_config_errors() {
    // Identity exists but no password has ever been configured.
    let mut wb = create_vauchi_with_identity("Alice");

    let err = wb
        .change_app_password("anything", "new-pin")
        .expect_err("change without existing config must error");
    let _ = err;
}

// @internal
#[test]
fn test_change_app_password_duress_unlock_cannot_change_password() {
    // Security invariant: if the user (under duress) enters the duress
    // PIN as the "current" password, change_app_password must NOT allow
    // the rotation to succeed. The duress unlock is for read-only decoy
    // access; it must never escalate to credential management.
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("real-pin-1234").expect("setup");
    wb.setup_duress_password("duress-9999")
        .expect("setup duress");

    let err = wb
        .change_app_password("duress-9999", "attacker-pin")
        .expect_err("duress PIN must not authorize a password change");
    let _ = err;

    // Real password still works; attacker pin does not
    let real_mode = wb
        .authenticate("real-pin-1234")
        .expect("real password still works");
    assert_eq!(real_mode, AuthMode::Normal);
    assert!(wb.authenticate("attacker-pin").is_err());
}

// @internal
#[test]
fn test_change_app_password_persists_across_storage_reload() {
    // The new hash + salt must be persisted to storage, not just the
    // in-memory AppPasswordConfig.
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("old-pin-1234").expect("setup");
    wb.change_app_password("old-pin-1234", "new-pin-9876")
        .expect("rotate");

    // Reload via load_password_config — same path Vauchi::authenticate uses
    let reloaded = wb
        .storage()
        .identity()
        .load_password_config()
        .expect("load")
        .expect("config exists");
    assert!(matches!(
        reloaded.verify("new-pin-9876"),
        AuthResult::Normal
    ));
    assert!(matches!(
        reloaded.verify("old-pin-1234"),
        AuthResult::Invalid
    ));
}

// =============================================================================
// Biometric Unlock Tests (P2-B — constant-time check moved to core)
// =============================================================================

// Lifecycle / Session Residue umbrella, item P2-B: the duress-aware
// post-biometric step lives in core so iOS and Android cannot drift on
// the floor or on the decision logic.

// @internal
#[test]
fn biometric_unlock_decision_unlocked_when_no_duress() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234").expect("setup app pw");

    let outcome = wb
        .biometric_unlock_check()
        .expect("biometric check should succeed");

    assert_eq!(
        outcome,
        BiometricUnlockOutcome::Unlocked,
        "no duress configured -> Unlocked"
    );
    assert_eq!(
        wb.auth_mode(),
        AuthMode::Normal,
        "Unlocked outcome must promote auth_mode to Normal"
    );
}

// @internal
#[test]
fn biometric_unlock_decision_prompts_for_pin_when_duress_enabled() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234").expect("setup app pw");
    wb.setup_duress_password("duress-999")
        .expect("setup duress");

    let outcome = wb
        .biometric_unlock_check()
        .expect("biometric check should succeed");

    assert_eq!(
        outcome,
        BiometricUnlockOutcome::PromptForDuressPin,
        "duress configured -> PromptForDuressPin"
    );
    assert_eq!(
        wb.auth_mode(),
        AuthMode::Unauthenticated,
        "PromptForDuressPin must NOT promote auth_mode — the PIN \
         step decides Normal vs Duress"
    );
}

// @internal
#[test]
fn biometric_unlock_check_pads_to_minimum_duration() {
    use std::time::Instant;
    use vauchi_core::api::vauchi::BIOMETRIC_UNLOCK_MIN_DURATION;

    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234").expect("setup app pw");

    let start = Instant::now();
    let _ = wb.biometric_unlock_check().expect("check");
    let elapsed = start.elapsed();

    assert!(
        elapsed >= BIOMETRIC_UNLOCK_MIN_DURATION,
        "biometric_unlock_check must pad to ≥{:?}, observed {:?}",
        BIOMETRIC_UNLOCK_MIN_DURATION,
        elapsed
    );
}

// @internal
#[test]
fn biometric_unlock_check_constant_time_across_duress_states() {
    use std::time::Instant;
    use vauchi_core::api::vauchi::BIOMETRIC_UNLOCK_MIN_DURATION;

    let mut wb_no_duress = create_vauchi_with_identity("Alice");
    wb_no_duress.setup_app_password("pw").expect("setup");

    let mut wb_with_duress = create_vauchi_with_identity("Bob");
    wb_with_duress.setup_app_password("pw").expect("setup");
    wb_with_duress
        .setup_duress_password("duress-999")
        .expect("setup duress");

    let t_no = {
        let s = Instant::now();
        let _ = wb_no_duress.biometric_unlock_check().expect("check");
        s.elapsed()
    };
    let t_with = {
        let s = Instant::now();
        let _ = wb_with_duress.biometric_unlock_check().expect("check");
        s.elapsed()
    };

    assert!(t_no >= BIOMETRIC_UNLOCK_MIN_DURATION);
    assert!(t_with >= BIOMETRIC_UNLOCK_MIN_DURATION);
}

// Phase 1 / Task 1.3 floor invariant. The two preceding tests verify
// the wall-clock floor end-to-end via `SystemSleeper`. This test
// checks the *seam contract*: the pad request is issued through
// `Sleeper::sleep` with a duration that closes the gap to
// `BIOMETRIC_UNLOCK_MIN_DURATION`. A future refactor that bypasses
// the seam (returns early, computes a zero gap, drops the call)
// fails here without paying the 300 ms wall-clock cost — keeping the
// regression net affordable as more sites migrate.
// @internal
#[test]
fn biometric_unlock_check_requests_floor_via_sleeper_seam() {
    use std::sync::Arc;
    use vauchi_core::api::vauchi::BIOMETRIC_UNLOCK_MIN_DURATION;
    use vauchi_core::sleeper::FakeSleeper;

    let fake = Arc::new(FakeSleeper::new());
    let mut wb = create_vauchi_with_identity("Alice").with_sleeper(fake.clone());
    wb.setup_app_password("pw").expect("setup app pw");

    let _ = wb.biometric_unlock_check().expect("check");

    let calls = fake.calls();
    assert_eq!(
        calls.len(),
        1,
        "biometric_unlock_check must request exactly one sleep on the seam, got {} \
         — bypassing the seam erases the BIOMETRIC_UNLOCK_MIN_DURATION defense",
        calls.len()
    );
    let requested = calls[0];
    assert!(
        requested > std::time::Duration::ZERO,
        "sleep request was zero — the floor must close a real gap (elapsed work \
         is microseconds; pad duration is the floor minus elapsed)"
    );
    assert!(
        requested <= BIOMETRIC_UNLOCK_MIN_DURATION,
        "sleep request {:?} exceeded the floor {:?} — pad_to_minimum must not \
         over-shoot the configured floor",
        requested,
        BIOMETRIC_UNLOCK_MIN_DURATION
    );
}

// =============================================================================
// =============================================================================

// @scenario: duress_mode :: Normal credential shows real contacts
#[test]
fn test_list_contacts_unauthenticated_returns_real() {
    let (alice_wb, _bob_wb, _secret, _bob_id, _alice_id) = setup_alice_bob_exchange();

    // Alice is unauthenticated (no password set), should see real contacts
    assert_eq!(alice_wb.auth_mode(), AuthMode::Unauthenticated);
    let contacts = alice_wb.list_contacts().expect("list should succeed");
    // Alice has one real contact (Bob) — hidden filtering applies, but Bob isn't hidden
    assert!(
        !contacts.is_empty(),
        "unauthenticated mode should return real contacts"
    );
}

// @scenario: duress_mode :: Normal credential shows real contacts
#[test]
fn test_list_contacts_normal_mode_returns_real() {
    let (mut alice_wb, _bob_wb, _secret, _bob_id, _alice_id) = setup_alice_bob_exchange();

    alice_wb
        .setup_app_password("my-pin")
        .expect("setup should succeed");
    let mode = alice_wb
        .authenticate("my-pin")
        .expect("auth should succeed");
    assert_eq!(mode, AuthMode::Normal);

    let contacts = alice_wb.list_contacts().expect("list should succeed");
    assert!(
        !contacts.is_empty(),
        "normal mode should return real contacts"
    );
}

// @scenario: duress_mode :: Duress credential shows decoy contacts
// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @scenario: duress_mode :: Duress mode looks identical to normal mode
// @scenario: duress_mode :: Decoy profile functions normally
#[test]
fn test_list_contacts_duress_mode_returns_decoy() {
    let (mut alice_wb, _bob_wb, _secret, _bob_id, _alice_id) = setup_alice_bob_exchange();

    alice_wb
        .setup_app_password("my-pin")
        .expect("setup should succeed");
    alice_wb
        .setup_duress_password("duress-pin")
        .expect("setup duress should succeed");

    let decoy_card = ContactCard::new("Decoy Contact");
    alice_wb
        .add_decoy_contact("decoy-1", "Decoy Contact", &decoy_card)
        .expect("add decoy should succeed");

    let mode = alice_wb
        .authenticate("duress-pin")
        .expect("auth should succeed");
    assert_eq!(mode, AuthMode::Duress);

    let contacts = alice_wb.list_contacts().expect("list should succeed");
    assert_eq!(
        contacts.len(),
        1,
        "duress mode should return decoy contacts"
    );
    assert_eq!(contacts[0].display_name(), "Decoy Contact");
}

// =============================================================================
// =============================================================================

// @scenario: duress_mode :: Duress mode is opt-in and disabled by default
#[test]
fn test_load_duress_settings_returns_none_initially() {
    let wb = create_vauchi_with_identity("Alice");

    let settings = wb.load_duress_settings().expect("load should succeed");
    assert!(
        settings.is_none(),
        "duress settings should be None before any configuration"
    );
}

// @scenario: duress_mode :: Configure trusted contacts for duress alerts
// @scenario: duress_mode :: App update preserves duress configuration
#[test]
fn test_save_load_duress_settings_roundtrip() {
    let wb = create_vauchi_with_identity("Alice");

    let settings = vauchi_core::types::DuressSettings {
        alert_contact_ids: vec!["contact-1".to_string(), "contact-2".to_string()],
        alert_message: "I need help".to_string(),
        include_location: true,
    };

    wb.save_duress_settings(&settings)
        .expect("save should succeed");

    let loaded = wb
        .load_duress_settings()
        .expect("load should succeed")
        .expect("should have duress settings");

    assert_eq!(loaded.alert_contact_ids, settings.alert_contact_ids);
    assert_eq!(loaded.alert_message, settings.alert_message);
    assert_eq!(loaded.include_location, settings.include_location);
}

// @scenario: duress_mode :: Disable duress mode from settings
#[test]
fn test_delete_duress_settings() {
    let wb = create_vauchi_with_identity("Alice");

    let settings = vauchi_core::types::DuressSettings {
        alert_contact_ids: vec!["contact-1".to_string()],
        alert_message: "Help".to_string(),
        include_location: false,
    };

    wb.save_duress_settings(&settings)
        .expect("save should succeed");

    wb.delete_duress_settings().expect("delete should succeed");

    let loaded = wb.load_duress_settings().expect("load should succeed");
    assert!(
        loaded.is_none(),
        "duress settings should be None after delete"
    );
}

// =============================================================================
// =============================================================================

// @internal
#[test]
fn test_duress_authenticate_queues_nothing() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");
    wb.setup_duress_password("duress-999")
        .expect("setup duress should succeed");

    // Create a trusted contact with a ratchet so the alert can be queued.
    let trusted_contact = Contact::from_exchange(
        *wb.identity().unwrap().signing_public_key(),
        ContactCard::new("Trusted Friend"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let trusted_id = trusted_contact.id().to_string();
    wb.add_contact(trusted_contact).unwrap();
    let (ratchet, is_initiator) = {
        let dh = vauchi_core::exchange::X3DHKeyPair::generate();
        let r = vauchi_core::crypto::ratchet::DoubleRatchetState::initialize_initiator(
            &vauchi_core::crypto::SymmetricKey::generate(),
            *dh.public_key(),
        )
        .unwrap();
        (r, true)
    };
    wb.storage()
        .ratchets()
        .save_ratchet_state(&trusted_id, &ratchet, is_initiator)
        .unwrap();

    // Configure duress settings with trusted contacts
    let settings = vauchi_core::types::DuressSettings {
        alert_contact_ids: vec![trusted_id.clone()],
        alert_message: "I'm in danger".to_string(),
        include_location: false,
    };
    wb.save_duress_settings(&settings)
        .expect("save settings should succeed");

    // Authenticate with duress PIN — silent by design: the alert queue left
    // the authentication path (74e1269d, 2026-07-08-simplify-duress-emergency;
    // the wipe fires at complete_lock). Even with settings + a reachable
    // trusted contact, NOTHING observable may be queued at auth time.
    let mode = wb.authenticate("duress-999").expect("auth should succeed");
    assert_eq!(mode, AuthMode::Duress);

    let pending = wb
        .storage()
        .pending()
        .count_all_pending_updates()
        .expect("count pending");
    assert_eq!(
        pending, 0,
        "duress authentication must queue nothing, even with alert settings configured"
    );
    let _ = trusted_id;
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
#[test]
fn test_queue_duress_alert_without_settings_is_noop() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");
    wb.setup_duress_password("duress-999")
        .expect("setup duress should succeed");

    // Do NOT configure duress settings

    // Authenticate with duress PIN — should NOT fail, just no alerts
    let mode = wb.authenticate("duress-999").expect("auth should succeed");
    assert_eq!(mode, AuthMode::Duress);

    // Verify no pending updates were queued (no settings = no recipients).
    let pending = wb
        .storage()
        .pending()
        .count_all_pending_updates()
        .expect("count pending");
    assert!(
        pending == 0,
        "duress authentication without settings should not queue pending updates"
    );
}

// The disguised safety-alert wire shape now ships only via the emergency
// broadcast path — duress auth queues nothing (74e1269d). The PendingUpdate
// must still be indistinguishable from a card delta (ADR-032).
// @scenario: emergency_broadcast :: Alert delivery to multiple contacts
#[test]
fn test_safety_alert_pending_update_is_disguised_as_card_delta() {
    let mut wb = create_vauchi_with_identity("Alice");

    // Create a trusted contact with a ratchet so the alert can be queued.
    let trusted_contact = Contact::from_exchange(
        *wb.identity().unwrap().signing_public_key(),
        ContactCard::new("Trusted Friend"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let trusted_id = trusted_contact.id().to_string();
    wb.add_contact(trusted_contact).unwrap();
    let (ratchet, is_initiator) = {
        let dh = vauchi_core::exchange::X3DHKeyPair::generate();
        let r = vauchi_core::crypto::ratchet::DoubleRatchetState::initialize_initiator(
            &vauchi_core::crypto::SymmetricKey::generate(),
            *dh.public_key(),
        )
        .unwrap();
        (r, true)
    };
    wb.storage()
        .ratchets()
        .save_ratchet_state(&trusted_id, &ratchet, is_initiator)
        .unwrap();

    wb.configure_emergency_broadcast(vec![trusted_id.clone()], "Help".to_string(), false)
        .expect("configure broadcast should succeed");

    let before = wb.clock().unix_seconds();
    let result = wb
        .send_emergency_broadcast()
        .expect("broadcast should succeed");
    assert_eq!(result.sent, 1);

    // Verify the alert was queued as a storage-backed PendingUpdate.
    let pending = wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .expect("load pending");
    assert!(!pending.is_empty(), "should have queued a duress alert");

    let update = &pending[0];
    assert!(
        update.created_at >= before,
        "pending update should have a timestamp >= auth time"
    );
    assert_eq!(update.update_type, "card_delta");
    assert_eq!(update.contact_id, trusted_id);
    // Payload is encrypted ratchet traffic — verify it is non-empty.
    assert!(!update.payload.is_empty(), "payload should not be empty");
}

// =============================================================================
// =============================================================================

// @scenario: duress_mode :: Duress credential shows decoy contacts
#[test]
fn test_full_duress_flow_shows_decoys_and_queues_nothing() {
    let mut wb = create_vauchi_with_identity("Alice");

    // 1. Set up app password
    wb.setup_app_password("my-pin-1234")
        .expect("setup app password should succeed");

    // 2. Set up duress password
    wb.setup_duress_password("duress-999")
        .expect("setup duress should succeed");

    // 3. Add decoy contacts
    let decoy_card = ContactCard::new("Fake Friend");
    wb.add_decoy_contact("decoy-1", "Fake Friend", &decoy_card)
        .expect("add decoy should succeed");

    // 4. Create a trusted contact with a ratchet so the alert can be queued.
    let trusted_contact = Contact::from_exchange(
        *wb.identity().unwrap().signing_public_key(),
        ContactCard::new("Trusted Friend"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let trusted_id = trusted_contact.id().to_string();
    wb.add_contact(trusted_contact).unwrap();
    let (ratchet, is_initiator) = {
        let dh = vauchi_core::exchange::X3DHKeyPair::generate();
        let r = vauchi_core::crypto::ratchet::DoubleRatchetState::initialize_initiator(
            &vauchi_core::crypto::SymmetricKey::generate(),
            *dh.public_key(),
        )
        .unwrap();
        (r, true)
    };
    wb.storage()
        .ratchets()
        .save_ratchet_state(&trusted_id, &ratchet, is_initiator)
        .unwrap();

    // 5. Configure duress alert settings
    let settings = vauchi_core::types::DuressSettings {
        alert_contact_ids: vec![trusted_id.clone()],
        alert_message: "Emergency - duress unlock".to_string(),
        include_location: true,
    };
    wb.save_duress_settings(&settings)
        .expect("save duress settings should succeed");

    // 6. Verify everything is configured
    assert!(wb.is_password_enabled().expect("check should succeed"));
    assert!(wb.is_duress_enabled().expect("check should succeed"));
    let loaded_settings = wb
        .load_duress_settings()
        .expect("load should succeed")
        .expect("should have settings");
    assert_eq!(loaded_settings.alert_contact_ids.len(), 1);

    // 7. Authenticate with duress PIN — triggers full flow
    let mode = wb.authenticate("duress-999").expect("auth should succeed");
    assert_eq!(mode, AuthMode::Duress);

    // 8a. Verify duress mode shows decoy contacts
    let contacts = wb.list_contacts().expect("list should succeed");
    assert_eq!(contacts.len(), 1, "should show decoy contacts only");
    assert_eq!(contacts[0].display_name(), "Fake Friend");
    // 8b. Duress auth is silent: the alert queue left the authentication
    // path (74e1269d — the wipe fires at complete_lock instead).
    let pending = wb
        .storage()
        .pending()
        .count_all_pending_updates()
        .expect("count pending");
    assert_eq!(pending, 0, "duress authentication must queue nothing");
    let _ = trusted_id;
}
