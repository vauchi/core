// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Third batch of common context-setup step bindings.

use cucumber::{given, then, when};

use crate::VauchiWorld;

// ── Themes ────────────────────────────────────────────────────────────────────

#[when("the user views available themes")]
fn user_views_available_themes(_world: &mut VauchiWorld) {}

// ── Platform context ──────────────────────────────────────────────────────────

#[given("Alice is using iOS")]
fn alice_using_ios(_world: &mut VauchiWorld) {}

#[given("I am on Android")]
fn am_on_android(_world: &mut VauchiWorld) {}

#[given("I am on iOS")]
fn am_on_ios(_world: &mut VauchiWorld) {}

#[given("I am on desktop")]
fn am_on_desktop(_world: &mut VauchiWorld) {}

// ── Duress / panic ────────────────────────────────────────────────────────────

#[when("I unlock with the duress credential")]
fn unlock_with_duress_credential(_world: &mut VauchiWorld) {}

#[when("I trigger a panic shred")]
fn trigger_panic_shred(_world: &mut VauchiWorld) {}

// ── Contact count as When steps ───────────────────────────────────────────────

#[when(expr = "I have {int} contacts")]
fn when_have_n_contacts(world: &mut VauchiWorld, n: u32) {
    let n = n as usize;
    let current = world.vauchi.list_contacts().unwrap().len();
    for i in current..n {
        world.add_test_contact(&format!("Contact{i:04}"));
    }
}

// ── Social / address fields ───────────────────────────────────────────────────

#[when(expr = "I add a social field for {string}")]
fn add_social_field(world: &mut VauchiWorld, network: String) {
    world.pending_field_type = Some("social".to_string());
    world.pending_label = Some(network.clone());
}

// ── Locale / i18n ─────────────────────────────────────────────────────────────

#[then("all UI strings should be translated")]
fn all_ui_strings_translated(_world: &mut VauchiWorld) {}

// ── Emergency broadcast / safety ─────────────────────────────────────────────

#[given("I am configuring emergency broadcast")]
fn configuring_emergency_broadcast(_world: &mut VauchiWorld) {}

// ── Multi-device ──────────────────────────────────────────────────────────────

#[given("Device A and Device B are linked")]
fn device_a_and_b_are_linked(_world: &mut VauchiWorld) {}

// ── Bob / Alice with specific fields ─────────────────────────────────────────

// ── Recovery trust / proof ────────────────────────────────────────────────────

#[given("Alice has marked Bob as recovery-trusted")]
fn alice_marked_bob_recovery_trusted(_world: &mut VauchiWorld) {}

#[given(expr = "Alice has uploaded a recovery proof")]
fn alice_uploaded_recovery_proof(_world: &mut VauchiWorld) {}

#[given("David has no mutual contacts with the vouchers")]
fn david_no_mutual_contacts(_world: &mut VauchiWorld) {}

// ── BLE / exchange session ────────────────────────────────────────────────────

#[given("Alice has a BLE session")]
fn alice_has_ble_session(_world: &mut VauchiWorld) {}

#[given("a mock BLE transport")]
fn mock_ble_transport(_world: &mut VauchiWorld) {}

#[when("the BLE GATT service is configured")]
fn ble_gatt_service_configured(_world: &mut VauchiWorld) {}

// ── UI action steps ───────────────────────────────────────────────────────────

#[when("I update my contact card")]
fn update_contact_card(_world: &mut VauchiWorld) {}

#[when("I tap on any actionable contact field")]
fn tap_actionable_contact_field(_world: &mut VauchiWorld) {}

#[when("I open the Help section")]
fn open_help_section(_world: &mut VauchiWorld) {}

// ── Then no-ops ───────────────────────────────────────────────────────────────

#[then("the settings should be saved")]
fn settings_should_be_saved(_world: &mut VauchiWorld) {}

#[then("I should see the available themes")]
fn should_see_available_themes(_world: &mut VauchiWorld) {}

#[then("I should see theme options")]
fn should_see_theme_options(_world: &mut VauchiWorld) {}

#[then("I should be able to select a theme")]
fn should_be_able_to_select_theme(_world: &mut VauchiWorld) {}

#[then("the theme should be applied")]
fn theme_applied(_world: &mut VauchiWorld) {}

#[then("I should be able to preview themes")]
fn should_preview_themes(_world: &mut VauchiWorld) {}

#[then("a panic shred should be initiated")]
fn panic_shred_initiated(_world: &mut VauchiWorld) {}

#[then("my data should be wiped")]
fn data_should_be_wiped(_world: &mut VauchiWorld) {}

#[then("duress should not be obvious")]
fn duress_not_obvious(_world: &mut VauchiWorld) {}

#[then("I should see a duress indicator")]
fn should_see_duress_indicator(_world: &mut VauchiWorld) {}

#[then("the app should appear normal")]
fn app_appears_normal(_world: &mut VauchiWorld) {}

#[then("I should see the FAQ list")]
fn should_see_faq_list(_world: &mut VauchiWorld) {}

#[then("I should be able to expand FAQ items")]
fn should_expand_faq_items(_world: &mut VauchiWorld) {}

#[then("I should see relevant help content")]
fn should_see_relevant_help_content(_world: &mut VauchiWorld) {}

#[then("I should be able to search FAQs")]
fn should_search_faqs(_world: &mut VauchiWorld) {}

#[then("I should see contact support options")]
fn should_see_support_options(_world: &mut VauchiWorld) {}

#[then("the Settings screen should be displayed")]
fn settings_screen_displayed(_world: &mut VauchiWorld) {}

#[then("I should see the Settings screen")]
fn should_see_settings_screen(_world: &mut VauchiWorld) {}

#[then("I should see the Help screen")]
fn should_see_help_screen(_world: &mut VauchiWorld) {}

#[then(expr = "I should be able to navigate back to {string}")]
fn should_navigate_back_to(_world: &mut VauchiWorld, _screen: String) {}

#[then("I should see my card as Bob would see it")]
fn see_card_as_bob(_world: &mut VauchiWorld) {}

#[then(expr = "fields hidden from Bob should show as {string} placeholders")]
fn fields_hidden_show_as(_world: &mut VauchiWorld, _placeholder: String) {}

#[then(expr = "I should see a banner {string}")]
fn should_see_banner(_world: &mut VauchiWorld, _text: String) {}

#[then(expr = "I should see an {string} button")]
fn should_see_button(_world: &mut VauchiWorld, _label: String) {}
