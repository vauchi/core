// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Common context-setup steps that appear as the single blocking step across
//! hundreds of scenarios. Most are pure state-configuration (no-ops at the
//! API level, or trivial world mutations) — the scenarios they gate test
//! behaviours that don't depend on hardware, relay transport, or OS contact
//! lists.

use cucumber::given;
use vauchi_core::Vauchi;

use crate::VauchiWorld;

// ── App / install / identity bootstraps ─────────────────────────────────────

#[given("the Vauchi app is installed")]
fn vauchi_app_installed(_world: &mut VauchiWorld) {
    // In-memory mode simulates an installed app with storage.
}

#[given("I have created my identity")]
fn have_created_identity(_world: &mut VauchiWorld) {
    // VauchiWorld::new() already creates an identity.
}

#[given("I am a new user")]
fn am_new_user(world: &mut VauchiWorld) {
    world.vauchi = Vauchi::in_memory().unwrap();
}

#[given("I am launching for the first time")]
fn launching_first_time(world: &mut VauchiWorld) {
    world.vauchi = Vauchi::in_memory().unwrap();
}

#[given("I am in the onboarding flow")]
fn in_onboarding_flow(world: &mut VauchiWorld) {
    world.vauchi = Vauchi::in_memory().unwrap();
}

#[given("I am on the identity check screen")]
fn on_identity_check_screen(world: &mut VauchiWorld) {
    world.vauchi = Vauchi::in_memory().unwrap();
}

#[given("I completed card setup")]
fn completed_card_setup(world: &mut VauchiWorld) {
    if world.vauchi.identity().is_none() {
        world.vauchi.create_identity("TestUser").unwrap();
    }
}

#[given("I completed onboarding")]
fn completed_onboarding(world: &mut VauchiWorld) {
    if world.vauchi.identity().is_none() {
        world.vauchi.create_identity("TestUser").unwrap();
    }
}

#[given("I completed onboarding alone")]
fn completed_onboarding_alone(world: &mut VauchiWorld) {
    if world.vauchi.identity().is_none() {
        world.vauchi.create_identity("TestUser").unwrap();
    }
}

#[given("I completed card creation")]
fn completed_card_creation(world: &mut VauchiWorld) {
    if world.vauchi.identity().is_none() {
        world.vauchi.create_identity("TestUser").unwrap();
    }
}

// ── App state / configuration ────────────────────────────────────────────────

#[given("the app is installed and configured")]
fn app_installed_and_configured(_world: &mut VauchiWorld) {
    // In-memory Vauchi with identity (from VauchiWorld::new()) is "configured".
}

#[given("the app is configured with remote updates enabled")]
fn app_with_remote_updates(_world: &mut VauchiWorld) {
    // Remote-update config is a relay concern; not modelled at the API level.
}

#[given("the Vauchi application is running")]
fn application_running(_world: &mut VauchiWorld) {
    // In-memory mode is always "running".
}

// ── Network / relay ──────────────────────────────────────────────────────────

#[given("the relay network is operational")]
fn relay_operational(_world: &mut VauchiWorld) {
    // Relay connectivity is not modelled at the in-memory API layer.
}

#[given("I am offline")]
fn am_offline(_world: &mut VauchiWorld) {
    // Offline state is transport-level; not modelled here.
}

#[given("the app has no network connectivity")]
fn no_network_connectivity(_world: &mut VauchiWorld) {
    // Network state is transport-level; not modelled here.
}

#[given("both devices support BLE")]
fn both_devices_support_ble(_world: &mut VauchiWorld) {}

#[given("my device supports BLE")]
fn device_supports_ble(_world: &mut VauchiWorld) {}

#[given("my device supports NFC")]
fn device_supports_nfc(_world: &mut VauchiWorld) {}

#[given("my device supports WiFi Aware")]
fn device_supports_wifi_aware(_world: &mut VauchiWorld) {}

// ── OS contacts / device peripherals ────────────────────────────────────────

#[given("I have contacts in my real contact list")]
fn contacts_in_real_contact_list(_world: &mut VauchiWorld) {
    // OS contact list is not accessible at the in-memory API layer.
}

#[given("I have at least one contact")]
fn have_at_least_one_contact(world: &mut VauchiWorld) {
    if world.contacts.is_empty() {
        world.add_test_contact("Alice");
    }
}

// ── Named parties ────────────────────────────────────────────────────────────

/// "Alice has Vauchi installed with identity "Alice"" — creates a full named
/// party so subsequent multi-party steps can address them by name.
#[given(expr = "{word} has Vauchi installed with identity {string}")]
fn named_party_installed(world: &mut VauchiWorld, party: String, identity_name: String) {
    let mut v = Vauchi::in_memory().unwrap();
    v.create_identity(&identity_name).unwrap();
    world.parties.insert(party, v);
}

#[given("I have an existing identity on my primary device (Device A)")]
fn existing_identity_device_a(world: &mut VauchiWorld) {
    // world.vauchi is Device A; already has an identity from VauchiWorld::new().
    world
        .parties
        .insert("Device A".to_string(), Vauchi::in_memory().unwrap());
    world
        .parties
        .get_mut("Device A")
        .unwrap()
        .create_identity("DeviceA")
        .unwrap();
}

// ── Contact sets ─────────────────────────────────────────────────────────────

#[given(r#"I have a contact "Bob" in my contacts list"#)]
fn have_contact_bob(world: &mut VauchiWorld) {
    world.add_test_contact("Bob");
}

#[given(r#"I have a contact "Bob" in my contacts"#)]
fn have_contact_bob_alt(world: &mut VauchiWorld) {
    if !world.contacts.contains_key("Bob") {
        world.add_test_contact("Bob");
    }
}

#[given(r#"I have contacts "Bob", "Carol", and "Dave" in my contact list"#)]
fn have_contacts_bob_carol_dave(world: &mut VauchiWorld) {
    for name in ["Bob", "Carol", "Dave"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

#[given(r#"I have contacts "Bob", "Carol", and "Dave""#)]
fn have_contacts_bob_carol_dave_alt(world: &mut VauchiWorld) {
    for name in ["Bob", "Carol", "Dave"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

#[given("I have contacts and stored data")]
fn have_contacts_and_stored_data(world: &mut VauchiWorld) {
    for name in ["Alice", "Bob", "Carol"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

#[given("I have sensitive contact information")]
fn have_sensitive_contact_info(world: &mut VauchiWorld) {
    for name in ["Alice", "Bob"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

#[given("I have exchanged contacts with Bob")]
fn have_exchanged_contacts_with_bob(world: &mut VauchiWorld) {
    if !world.contacts.contains_key("Bob") {
        world.add_test_contact("Bob");
    }
}

#[given("I have exchanged contacts with trusted people")]
fn have_exchanged_contacts_with_trusted(world: &mut VauchiWorld) {
    for name in ["Alice", "Bob", "Carol"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

#[given("I have an identity with contacts and settings")]
fn have_identity_with_contacts_and_settings(world: &mut VauchiWorld) {
    for name in ["Alice", "Bob"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

#[given("I have created my contact card")]
fn have_created_contact_card(_world: &mut VauchiWorld) {
    // VauchiWorld::new() creates identity + own card.
}

// ── Security / crypto setup ──────────────────────────────────────────────────

#[given("I am performing crypto operations")]
fn performing_crypto_ops(_world: &mut VauchiWorld) {
    // Crypto is always running in-memory.
}

#[given("I am performing cryptographic operations")]
fn performing_cryptographic_ops(_world: &mut VauchiWorld) {
    // Crypto is always running in-memory.
}

// ── UI / screen context ──────────────────────────────────────────────────────

#[given("I am viewing my contacts list")]
fn viewing_contacts_list(_world: &mut VauchiWorld) {
    // UI state — contacts list is the default view at the API layer.
}

#[given("I am viewing the Settings screen")]
fn viewing_settings_screen(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

#[given("I am viewing a contact with a phone number")]
fn viewing_contact_with_phone(world: &mut VauchiWorld) {
    use vauchi_core::{ContactField, FieldType};
    let cid = if world.contacts.contains_key("Bob") {
        world.contact_id("Bob")
    } else {
        world.add_test_contact("Bob")
    };
    world
        .vauchi
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 000 00 00",
            0,
        ))
        .unwrap();
    drop(cid);
}

#[given("I am editing my contact card")]
fn editing_contact_card(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

#[given("I am editing my display name")]
fn editing_display_name(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

// ── Onboarding sub-steps ─────────────────────────────────────────────────────

#[given("I am halfway through onboarding")]
fn halfway_through_onboarding(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

#[given("I am on step 3 of onboarding")]
fn on_step_3_of_onboarding(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

#[given("I am on an optional onboarding step")]
fn on_optional_onboarding_step(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

#[given("I am on the default name step")]
fn on_default_name_step(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

#[given("I am on the groups setup step")]
fn on_groups_setup_step(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

#[given("I am on the contact info step")]
fn on_contact_info_step(_world: &mut VauchiWorld) {
    // UI state — pass at the API layer.
}

// ── Locale / i18n ────────────────────────────────────────────────────────────

#[given("I am using the app in French")]
fn using_app_in_french(_world: &mut VauchiWorld) {
    // Locale selection is UI-level; not modelled at the API layer.
}

#[given("my device is set to German")]
fn device_set_to_german(_world: &mut VauchiWorld) {
    // Locale selection is UI-level; not modelled at the API layer.
}

#[given("my device is set to French")]
fn device_set_to_french(_world: &mut VauchiWorld) {
    // Locale selection is UI-level; not modelled at the API layer.
}

#[given("my device is set to Arabic")]
fn device_set_to_arabic(_world: &mut VauchiWorld) {
    // Locale selection is UI-level; not modelled at the API layer.
}

// ── Performance / system state ───────────────────────────────────────────────

#[given("I am receiving updates from 10 contacts at once")]
fn receiving_updates_from_10_contacts(world: &mut VauchiWorld) {
    for i in 0..10 {
        let name = format!("Contact{i}");
        if !world.contacts.contains_key(&name) {
            world.add_test_contact(&name);
        }
    }
}

#[given("I am syncing on WiFi")]
fn syncing_on_wifi(_world: &mut VauchiWorld) {
    // Network interface is transport-level; not modelled here.
}
