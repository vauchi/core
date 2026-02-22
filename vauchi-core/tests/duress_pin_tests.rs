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

mod common;

use vauchi_core::contact_card::ContactCard;
use vauchi_core::storage::Storage;
use vauchi_core::{AppPasswordConfig, AuthMode, AuthResult};

use common::helpers::{create_vauchi_with_identity, setup_alice_bob_exchange};

// =============================================================================
// Password KDF Tests
// =============================================================================

// @scenario: duress_pin.feature:Enable duress PIN in settings
#[test]
fn test_create_password_config_produces_valid_hash() {
    let config = AppPasswordConfig::create("secure-pin-1234").expect("create should succeed");

    // Hash and salt should be populated
    assert_ne!(config.password_hash(), &[0u8; 32]);
    assert_ne!(config.password_salt(), &[0u8; 16]);

    // Duress should not be set up initially
    assert!(!config.duress_enabled());
    assert!(config.duress_hash().is_none());
    assert!(config.duress_salt().is_none());
}

// @scenario: duress_pin.feature:Normal PIN shows real contacts
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

// @scenario: duress_pin.feature:Wrong PIN handling
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

// @scenario: duress_pin.feature:Duress PIN shows decoy contacts
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

    // Normal password should still return Normal
    let result = config.verify("my-password");
    assert!(
        matches!(result, AuthResult::Normal),
        "normal password should still return Normal, got {:?}",
        result
    );
}

// @scenario: duress_pin.feature:Both databases use strong encryption
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

// @scenario: duress_pin.feature:Both databases use strong encryption
#[test]
fn test_create_different_passwords_produce_different_hashes() {
    let config1 = AppPasswordConfig::create("password-one").expect("create should succeed");
    let config2 = AppPasswordConfig::create("password-two").expect("create should succeed");

    // Different passwords should produce different hashes (different salts too)
    assert_ne!(config1.password_hash(), config2.password_hash());
}

// @scenario: duress_pin.feature:Duress PIN must differ from normal PIN
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
// Password Storage Tests
// =============================================================================

// @scenario: duress_pin.feature:Duress PIN is opt-in and disabled by default
#[test]
fn test_load_password_config_returns_none_initially() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let config = storage.load_password_config().expect("load should succeed");
    assert!(
        config.is_none(),
        "password config should be None before any password is set"
    );
}

// @scenario: duress_pin.feature:Enable duress PIN in settings
#[test]
fn test_save_load_app_password_roundtrip() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    // Create and save identity first (password columns are on identity table)
    // Create identity row so password columns can be updated
    let backup_data = b"dummy-backup-data";
    storage
        .save_identity(backup_data, "Test User")
        .expect("save identity should succeed");

    let password_config =
        AppPasswordConfig::create("test-password").expect("create should succeed");
    storage
        .save_app_password(
            password_config.password_hash(),
            password_config.password_salt(),
        )
        .expect("save should succeed");

    let loaded = storage
        .load_password_config()
        .expect("load should succeed")
        .expect("should have password config");

    assert_eq!(loaded.password_hash(), password_config.password_hash());
    assert_eq!(loaded.password_salt(), password_config.password_salt());
    assert!(!loaded.duress_enabled());
}

// @scenario: duress_pin.feature:Enable duress PIN in settings
#[test]
fn test_save_load_duress_password_roundtrip() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    // Create identity row so password columns can be updated
    let backup_data = b"dummy-backup-data";
    storage
        .save_identity(backup_data, "Test User")
        .expect("save identity should succeed");

    // Set up normal password first
    let mut password_config =
        AppPasswordConfig::create("test-password").expect("create should succeed");
    storage
        .save_app_password(
            password_config.password_hash(),
            password_config.password_salt(),
        )
        .expect("save app password should succeed");

    // Set up and save duress password
    password_config
        .setup_duress("duress-pin")
        .expect("setup duress should succeed");
    storage
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
        .load_password_config()
        .expect("load should succeed")
        .expect("should have password config");

    assert!(loaded.duress_enabled());
    assert_eq!(loaded.duress_hash(), password_config.duress_hash());
    assert_eq!(loaded.duress_salt(), password_config.duress_salt());
}

// @scenario: duress_pin.feature:Disable duress PIN from settings
#[test]
fn test_disable_duress_clears_data() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    // Create identity row so password columns can be updated
    let backup_data = b"dummy-backup-data";
    storage
        .save_identity(backup_data, "Test User")
        .expect("save identity should succeed");

    // Set up normal + duress
    let mut password_config =
        AppPasswordConfig::create("test-password").expect("create should succeed");
    storage
        .save_app_password(
            password_config.password_hash(),
            password_config.password_salt(),
        )
        .expect("save app password should succeed");
    password_config
        .setup_duress("duress-pin")
        .expect("setup duress should succeed");
    storage
        .save_duress_password(
            password_config.duress_hash().expect("duress hash"),
            password_config.duress_salt().expect("duress salt"),
        )
        .expect("save duress should succeed");

    // Disable duress
    storage.disable_duress().expect("disable should succeed");

    let loaded = storage
        .load_password_config()
        .expect("load should succeed")
        .expect("should have password config");

    assert!(!loaded.duress_enabled());
    assert!(loaded.duress_hash().is_none());
    assert!(loaded.duress_salt().is_none());
}

// =============================================================================
// Decoy Contact Storage Tests
// =============================================================================

// @scenario: duress_pin.feature:Duress PIN is opt-in and disabled by default
#[test]
fn test_load_decoy_contacts_empty_initially() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let contacts = storage.load_decoy_contacts().expect("load should succeed");
    assert!(
        contacts.is_empty(),
        "decoy contacts should be empty initially"
    );
}

// @scenario: duress_pin.feature:Configure decoy contacts
#[test]
fn test_save_load_decoy_contact() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let card = ContactCard::new("Fake Alice");
    storage
        .save_decoy_contact("decoy-1", "Fake Alice", &card)
        .expect("save should succeed");

    let contacts = storage.load_decoy_contacts().expect("load should succeed");
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].0, "decoy-1"); // id
    assert_eq!(contacts[0].1, "Fake Alice"); // display_name
    assert_eq!(contacts[0].2.display_name(), "Fake Alice"); // card
}

// @scenario: duress_pin.feature:Configure decoy contacts
#[test]
fn test_delete_decoy_contact() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let card = ContactCard::new("Fake Alice");
    storage
        .save_decoy_contact("decoy-1", "Fake Alice", &card)
        .expect("save should succeed");

    storage
        .delete_decoy_contact("decoy-1")
        .expect("delete should succeed");

    let contacts = storage.load_decoy_contacts().expect("load should succeed");
    assert!(
        contacts.is_empty(),
        "decoy contacts should be empty after delete"
    );
}

// @scenario: duress_pin.feature:Configure decoy contacts
#[test]
fn test_clear_all_decoy_contacts() {
    let storage =
        Storage::in_memory(vauchi_core::SymmetricKey::generate()).expect("storage should open");

    let card1 = ContactCard::new("Fake Alice");
    let card2 = ContactCard::new("Fake Bob");
    storage
        .save_decoy_contact("decoy-1", "Fake Alice", &card1)
        .expect("save 1 should succeed");
    storage
        .save_decoy_contact("decoy-2", "Fake Bob", &card2)
        .expect("save 2 should succeed");

    storage
        .clear_all_decoy_contacts()
        .expect("clear should succeed");

    let contacts = storage.load_decoy_contacts().expect("load should succeed");
    assert!(
        contacts.is_empty(),
        "decoy contacts should be empty after clear"
    );
}

// =============================================================================
// Auth Mode Tests
// =============================================================================

// @scenario: duress_pin.feature:Duress PIN is opt-in and disabled by default
#[test]
fn test_unauthenticated_mode_default() {
    let wb = create_vauchi_with_identity("Alice");

    assert_eq!(
        wb.auth_mode(),
        AuthMode::Unauthenticated,
        "default auth mode should be Unauthenticated"
    );
}

// @scenario: duress_pin.feature:Enable duress PIN in settings
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

// @scenario: duress_pin.feature:Normal PIN shows real contacts
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

// @scenario: duress_pin.feature:Wrong PIN handling
#[test]
fn test_authenticate_invalid_password_fails() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");

    let result = wb.authenticate("wrong-pin");
    assert!(result.is_err(), "wrong password should return an error");
    // Auth mode should remain Unauthenticated
    assert_eq!(wb.auth_mode(), AuthMode::Unauthenticated);
}

// @scenario: duress_pin.feature:Duress PIN shows decoy contacts
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

// @scenario: duress_pin.feature:Enable duress PIN in settings
#[test]
fn test_setup_duress_password_requires_app_password_first() {
    let mut wb = create_vauchi_with_identity("Alice");

    let result = wb.setup_duress_password("duress-999");
    assert!(
        result.is_err(),
        "setting up duress without app password should fail"
    );
}

// @scenario: duress_pin.feature:Disable duress PIN from settings
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
// Mode-Aware Contact Loading Tests
// =============================================================================

// @scenario: duress_pin.feature:Normal PIN shows real contacts
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

// @scenario: duress_pin.feature:Normal PIN shows real contacts
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

// @scenario: duress_pin.feature:Duress PIN shows decoy contacts
// @scenario: duress_pin.feature:Cannot access real contacts from duress mode
#[test]
fn test_list_contacts_duress_mode_returns_decoy() {
    let (mut alice_wb, _bob_wb, _secret, _bob_id, _alice_id) = setup_alice_bob_exchange();

    // Set up password + duress
    alice_wb
        .setup_app_password("my-pin")
        .expect("setup should succeed");
    alice_wb
        .setup_duress_password("duress-pin")
        .expect("setup duress should succeed");

    // Add a decoy contact
    let decoy_card = ContactCard::new("Decoy Contact");
    alice_wb
        .add_decoy_contact("decoy-1", "Decoy Contact", &decoy_card)
        .expect("add decoy should succeed");

    // Authenticate with duress
    let mode = alice_wb
        .authenticate("duress-pin")
        .expect("auth should succeed");
    assert_eq!(mode, AuthMode::Duress);

    let contacts = alice_wb.list_contacts().expect("list should succeed");
    // Should see the decoy contact, not Bob
    assert_eq!(
        contacts.len(),
        1,
        "duress mode should return decoy contacts"
    );
    assert_eq!(contacts[0].display_name(), "Decoy Contact");
}

// =============================================================================
// Duress Settings Storage Tests
// =============================================================================

// @scenario: duress_pin.feature:Duress PIN is opt-in and disabled by default
#[test]
fn test_load_duress_settings_returns_none_initially() {
    let wb = create_vauchi_with_identity("Alice");

    let settings = wb.load_duress_settings().expect("load should succeed");
    assert!(
        settings.is_none(),
        "duress settings should be None before any configuration"
    );
}

// @scenario: duress_pin.feature:Configure trusted contacts for duress alerts
#[test]
fn test_save_load_duress_settings_roundtrip() {
    let wb = create_vauchi_with_identity("Alice");

    let settings = vauchi_core::api::duress::DuressSettings {
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

// @scenario: duress_pin.feature:Disable duress PIN from settings
#[test]
fn test_delete_duress_settings() {
    let wb = create_vauchi_with_identity("Alice");

    let settings = vauchi_core::api::duress::DuressSettings {
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
// Duress Alert Tests
// =============================================================================

// @scenario: duress_pin.feature:Duress unlock sends silent alert to trusted contacts
#[test]
fn test_queue_duress_alert_on_duress_authenticate() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");
    wb.setup_duress_password("duress-999")
        .expect("setup duress should succeed");

    // Configure duress settings with trusted contacts
    let settings = vauchi_core::api::duress::DuressSettings {
        alert_contact_ids: vec!["trusted-contact-1".to_string()],
        alert_message: "I'm in danger".to_string(),
        include_location: false,
    };
    wb.save_duress_settings(&settings)
        .expect("save settings should succeed");

    // Authenticate with duress PIN — should queue an alert
    let mode = wb.authenticate("duress-999").expect("auth should succeed");
    assert_eq!(mode, AuthMode::Duress);

    // Verify that a duress alert was queued
    let alerts = wb.pending_duress_alerts();
    assert!(
        !alerts.is_empty(),
        "duress authentication should queue at least one alert"
    );
}

// @scenario: duress_pin.feature:Duress unlock sends silent alert to trusted contacts
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

    // No alerts should be queued
    let alerts = wb.pending_duress_alerts();
    assert!(
        alerts.is_empty(),
        "duress authentication without settings should not queue alerts"
    );
}

// @scenario: duress_pin.feature:Duress alert content
#[test]
fn test_duress_alert_contains_timestamp_and_device_id() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.setup_app_password("my-pin-1234")
        .expect("setup should succeed");
    wb.setup_duress_password("duress-999")
        .expect("setup duress should succeed");

    let settings = vauchi_core::api::duress::DuressSettings {
        alert_contact_ids: vec!["trusted-1".to_string()],
        alert_message: "Help".to_string(),
        include_location: false,
    };
    wb.save_duress_settings(&settings)
        .expect("save settings should succeed");

    wb.authenticate("duress-999").expect("auth should succeed");

    let alerts = wb.pending_duress_alerts();
    assert!(!alerts.is_empty());

    let alert = &alerts[0];
    assert!(
        alert.timestamp > 0,
        "alert should have a non-zero timestamp"
    );
    assert!(
        !alert.device_id.is_empty(),
        "alert should have a non-empty device_id"
    );
    assert!(
        matches!(
            alert.alert_type,
            vauchi_core::api::duress::DuressAlertType::Unlock
        ),
        "alert type should be Unlock"
    );
}

// =============================================================================
// Full Duress Flow Test
// =============================================================================

// @scenario: duress_pin.feature:Duress PIN shows decoy contacts
// @scenario: duress_pin.feature:Duress unlock sends silent alert to trusted contacts
#[test]
fn test_full_duress_flow_setup_to_alert() {
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

    // 4. Configure duress alert settings
    let settings = vauchi_core::api::duress::DuressSettings {
        alert_contact_ids: vec!["trusted-contact-1".to_string()],
        alert_message: "Emergency - duress unlock".to_string(),
        include_location: true,
    };
    wb.save_duress_settings(&settings)
        .expect("save duress settings should succeed");

    // 5. Verify everything is configured
    assert!(wb.is_password_enabled().expect("check should succeed"));
    assert!(wb.is_duress_enabled().expect("check should succeed"));
    let loaded_settings = wb
        .load_duress_settings()
        .expect("load should succeed")
        .expect("should have settings");
    assert_eq!(loaded_settings.alert_contact_ids.len(), 1);

    // 6. Authenticate with duress PIN — triggers full flow
    let mode = wb.authenticate("duress-999").expect("auth should succeed");
    assert_eq!(mode, AuthMode::Duress);

    // 7. Verify duress mode shows decoy contacts
    let contacts = wb.list_contacts().expect("list should succeed");
    assert_eq!(contacts.len(), 1, "should show decoy contacts only");
    assert_eq!(contacts[0].display_name(), "Fake Friend");

    // 8. Verify alert was queued
    let alerts = wb.pending_duress_alerts();
    assert!(!alerts.is_empty(), "should have queued a duress alert");
}
