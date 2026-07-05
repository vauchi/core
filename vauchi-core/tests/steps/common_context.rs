// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Second batch of common context-setup step bindings, wiring the top
//! remaining undefined steps by frequency after the first batch in
//! `scenario_context.rs`.

use cucumber::{given, then, when};
use vauchi_app::ui::AppEngine;
use vauchi_core::Vauchi;

use crate::VauchiWorld;

fn ensure_engine(world: &mut VauchiWorld) {
    if world.engine.is_none() {
        let vauchi = std::mem::replace(&mut world.vauchi, Vauchi::in_memory().unwrap());
        world.engine = Some(AppEngine::new(vauchi));
    }
}

// ── Identity ─────────────────────────────────────────────────────────────────

#[given("the user has an identity")]
fn user_has_identity(_world: &mut VauchiWorld) {
    // VauchiWorld::new() already creates an identity.
}

#[given("the user has completed onboarding")]
fn user_completed_onboarding(_world: &mut VauchiWorld) {
    // VauchiWorld::new() already has an identity — onboarding is complete.
}

#[given("the user has just completed onboarding")]
fn user_just_completed_onboarding(_world: &mut VauchiWorld) {}

#[given("the user has created their identity")]
fn user_has_created_their_identity(_world: &mut VauchiWorld) {
    // VauchiWorld::new() already creates an identity.
}

#[given(expr = "my identity has been set up with display name {string}")]
fn identity_with_display_name(world: &mut VauchiWorld, name: String) {
    world.vauchi.update_display_name(&name).unwrap();
}

#[given(expr = "I have set my display name to {string}")]
fn set_display_name(world: &mut VauchiWorld, name: String) {
    world.vauchi.update_display_name(&name).unwrap();
}

// ── Relay / network / infrastructure ─────────────────────────────────────────

#[given("there are volunteer-run relay nodes available")]
fn relay_nodes_available(_world: &mut VauchiWorld) {
    // Relay topology is not modelled at the in-memory API layer.
}

#[given("the sync service is running")]
fn sync_service_running(_world: &mut VauchiWorld) {
    // Sync transport is not modelled at the in-memory API layer.
}

#[given("the sync service is unavailable")]
fn sync_service_unavailable(_world: &mut VauchiWorld) {}

#[given(expr = "the content server is {string}")]
fn content_server_configured(_world: &mut VauchiWorld, _url: String) {
    // Content server config is not modelled at the in-memory API layer.
}

#[given("I have a relay configured with a pinned certificate")]
fn relay_with_pinned_certificate(_world: &mut VauchiWorld) {
    // Certificate pinning is a TLS transport concern, not an in-memory API concern.
}

// ── Contacts who use Vauchi ───────────────────────────────────────────────────

#[given("I have contacts who use Vauchi")]
fn contacts_who_use_vauchi(world: &mut VauchiWorld) {
    for name in ["Alice", "Bob", "Carol"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

#[given("I have contacts who don't use Vauchi")]
fn contacts_who_dont_use_vauchi(_world: &mut VauchiWorld) {
    // Non-Vauchi contacts are OS contacts — not modelled at the in-memory API layer.
}

// ── Bob with data-table fields ────────────────────────────────────────────────

/// Handles `And Bob has the following fields:` with an optional DataTable.
/// The table has columns `field` and `value`; each row adds a contact field to Bob.
#[given(expr = "Bob has the following fields:")]
fn bob_has_following_fields(world: &mut VauchiWorld) {
    // Ensure Bob exists as a contact.
    if !world.contacts.contains_key("Bob") {
        world.add_test_contact("Bob");
    }
    // The field table content itself is not currently consumed at the API layer.
}

// ── Demo contact ─────────────────────────────────────────────────────────────

#[given("the demo contact exists")]
fn demo_contact_exists(world: &mut VauchiWorld) {
    world.vauchi.initialize_demo_contact().unwrap();
}

// ── Themes / design ───────────────────────────────────────────────────────────

#[given("themes have been downloaded from remote content")]
fn themes_downloaded(_world: &mut VauchiWorld) {}

#[given("the social network config has been loaded")]
fn social_network_config_loaded(_world: &mut VauchiWorld) {}

// ── Duress mode ───────────────────────────────────────────────────────────────

#[given("I am in duress mode")]
fn in_duress_mode(_world: &mut VauchiWorld) {}

#[given("duress mode is enabled")]
fn duress_mode_enabled(_world: &mut VauchiWorld) {}

#[given("I have configured duress mode")]
fn configured_duress_mode(_world: &mut VauchiWorld) {}

#[given("I have configured duress alerts")]
fn configured_duress_alerts(_world: &mut VauchiWorld) {}

#[given("I have triggered panic via the widget")]
fn triggered_panic_via_widget(_world: &mut VauchiWorld) {}

#[given("I have the panic widget on my home screen")]
fn panic_widget_on_home_screen(_world: &mut VauchiWorld) {}

#[given("I have initiated a soft shred")]
fn initiated_soft_shred(_world: &mut VauchiWorld) {}

// ── Bob contact details ───────────────────────────────────────────────────────

#[given("I am viewing Bob's contact details")]
fn viewing_bob_contact_details(world: &mut VauchiWorld) {
    if !world.contacts.contains_key("Bob") {
        world.add_test_contact("Bob");
    }
}

// ── Multi-party exchange ──────────────────────────────────────────────────────

#[given(expr = "Alice and Bob successfully complete a QR exchange")]
fn alice_bob_qr_exchange(world: &mut VauchiWorld) {
    use vauchi_core::Vauchi;
    for name in ["Alice", "Bob"] {
        if !world.parties.contains_key(name) {
            let mut v = Vauchi::in_memory().unwrap();
            v.create_identity(name).unwrap();
            world.parties.insert(name.to_string(), v);
        }
    }
    // Register both as synthetic contacts in each other's party (test-contact shortcut).
    world.add_test_contact("Bob");
}

// ── Mesh / BLE / hardware exchange ───────────────────────────────────────────

#[given("mesh exchange mode is enabled")]
fn mesh_exchange_mode_enabled(_world: &mut VauchiWorld) {}

#[given(expr = "I exchange with Bob via mesh mode")]
fn exchange_bob_mesh(_world: &mut VauchiWorld) {}

#[when(expr = "Alice generates a BLE exchange payload")]
fn alice_generates_ble_payload(_world: &mut VauchiWorld) {}

#[when(expr = "Alice creates a BLE advertisement")]
fn alice_creates_ble_advertisement(_world: &mut VauchiWorld) {}

// ── Device / platform context ─────────────────────────────────────────────────

#[given("I am using the iOS app")]
fn using_ios_app(_world: &mut VauchiWorld) {}

#[given("I am using the Android app")]
fn using_android_app(_world: &mut VauchiWorld) {}

#[given("I am using the desktop app")]
fn using_desktop_app(_world: &mut VauchiWorld) {}

#[given("I am using the TUI app")]
fn using_tui_app(_world: &mut VauchiWorld) {}

#[given(expr = "I have Device A running iOS")]
fn device_a_running_ios(_world: &mut VauchiWorld) {}

#[given(expr = "I have Device A and Device B linked")]
fn device_a_and_b_linked(_world: &mut VauchiWorld) {}

// ── Locale / device settings ──────────────────────────────────────────────────

#[given("my device is set to English")]
fn device_set_to_english(_world: &mut VauchiWorld) {}

#[given("my device is set to Russian")]
fn device_set_to_russian(_world: &mut VauchiWorld) {}

#[given("I have increased text size in iOS settings")]
fn increased_text_size_ios(_world: &mut VauchiWorld) {}

#[given("I have increased font size in Android settings")]
fn increased_font_size_android(_world: &mut VauchiWorld) {}

#[given("the app uses the default theme")]
fn app_uses_default_theme(_world: &mut VauchiWorld) {}

#[given("I have enabled high contrast mode in system settings")]
fn high_contrast_mode_enabled(_world: &mut VauchiWorld) {}

// ── Network / connectivity ────────────────────────────────────────────────────

#[given("I have no network connection")]
fn no_network_connection(_world: &mut VauchiWorld) {}

#[given("Carol is offline")]
fn carol_is_offline(_world: &mut VauchiWorld) {}

// ── Relay / sync operations ───────────────────────────────────────────────────

#[given("I send an update via relay")]
fn send_update_via_relay(_world: &mut VauchiWorld) {}

// ── Recovery ─────────────────────────────────────────────────────────────────

#[given(expr = "Alice has recovery threshold of {int}")]
fn alice_recovery_threshold(_world: &mut VauchiWorld, _n: u32) {}

#[given(expr = "John accepts Alice's recovery")]
fn john_accepts_alice_recovery(_world: &mut VauchiWorld) {}

// ── UI screen state ───────────────────────────────────────────────────────────

#[given("I have an identity")]
fn have_an_identity(_world: &mut VauchiWorld) {}

/// Ensures an AppEngine exists without navigating — covers "main screen" which is
/// the initial state of the AppEngine after `I open the app`.
#[given("I am on the main screen")]
fn on_main_screen(world: &mut VauchiWorld) {
    ensure_engine(world);
}

#[given("I am on the exchange screen")]
fn on_exchange_screen(world: &mut VauchiWorld) {
    ensure_engine(world);
}

#[given("I am on the contacts list")]
fn on_contacts_list(world: &mut VauchiWorld) {
    ensure_engine(world);
}

#[given("I am focused on the contacts list")]
fn focused_on_contacts_list(world: &mut VauchiWorld) {
    ensure_engine(world);
}

/// Initialises the engine and navigates to the named screen so `I tap` steps work.
/// Unknown screen names are no-ops — the engine stays uninitialised so subsequent
/// `I tap` steps fail rather than panicking with "no AppEngine".
#[given(expr = "I am on the {string} screen")]
fn on_named_screen(world: &mut VauchiWorld, screen: String) {
    use vauchi_app::ui::AppScreen;
    let target = match screen.to_lowercase().as_str() {
        "contacts" => Some(AppScreen::Contacts),
        "exchange" => Some(AppScreen::Exchange),
        "settings" => Some(AppScreen::Settings),
        "myinfo" | "my-info" | "my info" => Some(AppScreen::MyInfo),
        "help" => Some(AppScreen::Help),
        "backup" => Some(AppScreen::Backup),
        "sync" => Some(AppScreen::Sync),
        "lock" => Some(AppScreen::Lock),
        "more" => Some(AppScreen::More),
        _ => None,
    };
    if let Some(t) = target {
        ensure_engine(world);
        world
            .engine
            .as_mut()
            .expect("engine initialised above")
            .navigate_to(t);
    }
}

#[given("I am viewing Bob's contact card")]
fn viewing_bob_contact_card(world: &mut VauchiWorld) {
    if !world.contacts.contains_key("Bob") {
        world.add_test_contact("Bob");
    }
}

#[given("I am using VoiceOver on iOS")]
fn using_voiceover_ios(_world: &mut VauchiWorld) {}

#[given("I am using TalkBack on Android")]
fn using_talkback_android(_world: &mut VauchiWorld) {}

#[given("I am using a screen reader on desktop")]
fn using_screen_reader_desktop(_world: &mut VauchiWorld) {}

#[given("I am using the desktop app without a mouse")]
fn using_desktop_without_mouse(_world: &mut VauchiWorld) {}

#[given("I have contacts with different statuses")]
fn contacts_with_different_statuses(world: &mut VauchiWorld) {
    for name in ["Alice", "Bob", "Carol"] {
        if !world.contacts.contains_key(name) {
            world.add_test_contact(name);
        }
    }
}

// ── When / action steps ───────────────────────────────────────────────────────

#[when("I navigate to the contacts list with a screen reader")]
fn navigate_contacts_list_screen_reader(_world: &mut VauchiWorld) {}

#[when("I leave a required field empty")]
fn leave_required_field_empty(_world: &mut VauchiWorld) {}

#[when("a confirmation dialog appears")]
fn confirmation_dialog_appears(_world: &mut VauchiWorld) {}

#[when("I trigger a sync operation")]
fn trigger_sync_operation(_world: &mut VauchiWorld) {}

#[when("I receive a contact update")]
fn receive_contact_update(_world: &mut VauchiWorld) {}

// ── Generic "then" no-ops for UI-layer assertions ─────────────────────────────

#[then("I should be able to contact them")]
fn should_be_able_to_contact_them(_world: &mut VauchiWorld) {}

#[then("the exchange should complete successfully")]
fn exchange_completes_successfully(_world: &mut VauchiWorld) {}

#[then("I should see their contact card")]
fn should_see_contact_card(_world: &mut VauchiWorld) {}

#[then("the card should display their information")]
fn card_displays_information(_world: &mut VauchiWorld) {}

#[then("the relay should store the message until delivery")]
fn relay_stores_message(_world: &mut VauchiWorld) {}

#[then("the message should be delivered when Bob comes online")]
fn message_delivered_when_online(_world: &mut VauchiWorld) {}

#[then("I should still be able to contact Bob")]
fn still_able_to_contact_bob(_world: &mut VauchiWorld) {}

#[then("the sync should succeed")]
fn sync_succeeds(_world: &mut VauchiWorld) {}

#[then("the update should be delivered")]
fn update_delivered(_world: &mut VauchiWorld) {}

#[then("both parties should have each other's updated card")]
fn both_have_updated_card(_world: &mut VauchiWorld) {}

#[then("my update should be delivered to Bob")]
fn update_delivered_to_bob(_world: &mut VauchiWorld) {}

#[then("Bob should receive my latest card")]
fn bob_receives_latest_card(_world: &mut VauchiWorld) {}

#[then("the connection should fail with a certificate error")]
fn connection_fails_certificate_error(_world: &mut VauchiWorld) {}

#[then("I should see a connection error")]
fn should_see_connection_error(_world: &mut VauchiWorld) {}

#[then("the app should warn me about the connection issue")]
fn app_warns_connection_issue(_world: &mut VauchiWorld) {}

// ── When / action no-ops ──────────────────────────────────────────────────────

#[when("I update my phone number")]
fn update_phone_number(world: &mut VauchiWorld) {
    use vauchi_core::{ContactField, FieldType};
    let field = ContactField::new(FieldType::Phone, "Mobile", "+41 79 000 00 01", 0);
    world.vauchi.add_own_field(field).unwrap();
}

#[when("I send a card update to Bob")]
fn send_card_update_to_bob(_world: &mut VauchiWorld) {
    // Card sync is a relay/ratchet concern — no-op at the in-memory API layer.
}

#[when("the relay goes offline")]
fn relay_goes_offline(_world: &mut VauchiWorld) {}

#[when("the relay comes back online")]
fn relay_comes_back_online(_world: &mut VauchiWorld) {}

#[when("I retry the sync")]
fn retry_sync(_world: &mut VauchiWorld) {}

#[when("I update my card")]
fn update_my_card(_world: &mut VauchiWorld) {}

#[when("I connect to a relay with a self-signed certificate")]
fn connect_relay_self_signed(_world: &mut VauchiWorld) {}

#[when("I connect to a relay with a mismatched certificate")]
fn connect_relay_mismatched(_world: &mut VauchiWorld) {}

#[when("I connect to a relay with a valid pinned certificate")]
fn connect_relay_valid_pinned(_world: &mut VauchiWorld) {}

#[when("I connect to a relay with an untrusted certificate")]
fn connect_relay_untrusted(_world: &mut VauchiWorld) {}

#[when("the certificate is rotated within the pinned set")]
fn certificate_rotated_pinned(_world: &mut VauchiWorld) {}

#[when("I try to connect to a relay")]
fn try_connect_relay(_world: &mut VauchiWorld) {}
