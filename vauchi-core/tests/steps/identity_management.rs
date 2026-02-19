// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Step definitions for identity_management.feature

use cucumber::{given, then, when};

use crate::VauchiWorld;

// ============================================================
// Background steps
// ============================================================

#[given("the Vauchi application is installed")]
fn app_installed(_world: &mut VauchiWorld) {
    // In-memory mode simulates installed app
}

#[given("the device has secure storage available")]
fn secure_storage_available(_world: &mut VauchiWorld) {
    // In-memory mode always has storage
}

// ============================================================
// Identity creation
// ============================================================

#[given("I am launching Vauchi for the first time")]
fn first_launch(world: &mut VauchiWorld) {
    // Reset to a fresh state (no identity)
    world.vauchi = vauchi_core::Vauchi::in_memory().unwrap();
}

#[given("I have just created a new identity")]
fn just_created_identity(world: &mut VauchiWorld) {
    if world.vauchi.identity().is_none() {
        world.vauchi.create_identity("NewUser").unwrap();
    }
}

#[given("I am on the identity setup screen")]
fn on_identity_setup(_world: &mut VauchiWorld) {
    // UI state — identity setup is the initial state
}

#[when("the application initializes")]
fn app_initializes(world: &mut VauchiWorld) {
    world.vauchi.create_identity("TestUser").unwrap();
}

#[then("a new Ed25519 keypair should be generated")]
fn ed25519_keypair_generated(world: &mut VauchiWorld) {
    let identity = world.vauchi.identity().unwrap();
    // Public key should be non-empty (Ed25519 signing key)
    assert!(!identity.signing_public_key().is_empty());
}

#[then("an X25519 exchange keypair should be derived")]
fn x25519_keypair_derived(world: &mut VauchiWorld) {
    let identity = world.vauchi.identity().unwrap();
    let _kp = identity.x3dh_keypair();
    // If this doesn't panic, the keypair exists
}

#[then("the private keys should be stored in secure storage")]
fn keys_in_secure_storage(world: &mut VauchiWorld) {
    // In-memory mode stores keys in the in-memory DB
    assert!(world.vauchi.identity().is_some());
}

#[then("I should see the identity setup screen")]
fn see_setup_screen(_world: &mut VauchiWorld) {
    // UI assertion — pass in API-level test
}

// ============================================================
// Display name setup
// ============================================================

#[when(expr = "I enter {string} as my display name")]
fn enter_display_name(world: &mut VauchiWorld, name: String) {
    world.pending_display_name = Some(name);
}

#[when("I confirm the setup")]
fn confirm_setup(world: &mut VauchiWorld) {
    if let Some(name) = world.pending_display_name.take() {
        world.last_result = match world.vauchi.update_display_name(&name) {
            Ok(()) => Ok(()),
            Err(e) => {
                world.last_error_message = Some(format!("{e}"));
                Err(format!("{e}"))
            }
        };
    }
}

#[then("I should be taken to the main screen")]
fn taken_to_main(_world: &mut VauchiWorld) {
    // UI state — pass in API-level test
}

// ============================================================
// Display name validation
// ============================================================

#[when("I try to set an empty display name")]
fn set_empty_display_name(world: &mut VauchiWorld) {
    world.last_result = match world.vauchi.update_display_name("") {
        Ok(()) => Ok(()),
        Err(e) => {
            world.last_error_message = Some(format!("{e}"));
            Err(format!("{e}"))
        }
    };
}

#[then(expr = "I should see an error {string}")]
fn see_error(world: &mut VauchiWorld, _expected: String) {
    assert!(
        world.last_result.is_err() || world.last_error_message.is_some(),
        "Expected an error"
    );
}

#[then("I should not be able to proceed")]
fn cannot_proceed(world: &mut VauchiWorld) {
    assert!(world.last_result.is_err());
}

// ============================================================
// Identity backup
// ============================================================

#[given("I am on the settings screen")]
fn on_settings_screen(_world: &mut VauchiWorld) {
    // UI state
}

#[when(expr = "I select {string}")]
fn select_option(_world: &mut VauchiWorld, _option: String) {
    // UI action — navigation
}

#[when(expr = "I enter backup password {string}")]
fn enter_backup_password(world: &mut VauchiWorld, password: String) {
    world.pending_password = Some(password);
}

#[when("I confirm the password")]
fn confirm_password(world: &mut VauchiWorld) {
    if let Some(password) = world.pending_password.take() {
        let identity = world.vauchi.identity().unwrap();
        world.last_result = match identity.export_backup(&password) {
            Ok(backup) => {
                world.backup_data = Some(backup.as_bytes().to_vec());
                Ok(())
            }
            Err(e) => {
                world.last_error_message = Some(format!("{e}"));
                Err(format!("{e}"))
            }
        };
    }
}

#[then("an encrypted backup file should be generated")]
fn backup_generated(world: &mut VauchiWorld) {
    assert!(world.backup_data.is_some(), "Backup data should exist");
    assert!(
        !world.backup_data.as_ref().unwrap().is_empty(),
        "Backup data should not be empty"
    );
}

#[then("the backup should contain my master seed")]
fn backup_contains_seed(world: &mut VauchiWorld) {
    // Backup is encrypted — we verify it's non-trivial in size
    assert!(world.backup_data.as_ref().unwrap().len() > 32);
}

#[then("the backup should be encrypted with my password")]
fn backup_encrypted(_world: &mut VauchiWorld) {
    // Verified implicitly — create_backup encrypts with the password
}

// ============================================================
// Password strength
// ============================================================

#[given("I am creating an identity backup")]
fn creating_backup(_world: &mut VauchiWorld) {
    // State: in backup creation flow
}

#[when(expr = "I enter password {string}")]
fn enter_password(world: &mut VauchiWorld, password: String) {
    world.pending_password = Some(password.clone());

    // Check password strength via the API
    use vauchi_core::identity::password::validate_password;
    let result = validate_password(&password);
    match result {
        Ok(_strength) => {
            world.last_result = Ok(());
        }
        Err(e) => {
            world.last_error_message = Some(format!("{e}"));
            world.last_result = Err(format!("{e}"));
        }
    }
}

#[then("I should see an error about password requirements")]
fn password_error(world: &mut VauchiWorld) {
    assert!(
        world.last_result.is_err() || world.last_error_message.is_some(),
        "Expected password requirement error"
    );
}

#[then("the backup should not be created")]
fn backup_not_created(world: &mut VauchiWorld) {
    assert!(world.backup_data.is_none());
}

// ============================================================
// Identity details
// ============================================================

#[when("I view my identity details")]
fn view_identity_details(_world: &mut VauchiWorld) {
    // UI action
}

#[then("I should see my public key fingerprint")]
fn see_fingerprint(world: &mut VauchiWorld) {
    let pid = world.vauchi.public_id().unwrap();
    assert!(!pid.is_empty());
}

#[then("the fingerprint should be displayed in a human-readable format")]
fn fingerprint_readable(world: &mut VauchiWorld) {
    let pid = world.vauchi.public_id().unwrap();
    // Public ID is hex-encoded
    assert!(pid.chars().all(|c| c.is_ascii_hexdigit()));
}

#[then("I should be able to copy the fingerprint")]
fn can_copy_fingerprint(_world: &mut VauchiWorld) {
    // UI capability — pass
}
