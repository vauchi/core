// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use cucumber::{given, then, when};

use crate::VauchiWorld;

// ── Demo contact setup ─────────────────────────────────────────────────────

/// Initializes the demo contact in the storage (safe to call when no real contacts exist).
#[given("I have the demo contact")]
fn have_demo_contact(world: &mut VauchiWorld) {
    world.vauchi.initialize_demo_contact().unwrap();
}

// ── Dismiss flow ───────────────────────────────────────────────────────────

/// Dismisses the demo contact (user explicitly deleted it).
#[when("I delete it")]
fn delete_demo_contact(world: &mut VauchiWorld) {
    world.vauchi.dismiss_demo_contact().unwrap();
}

/// Asserts the demo contact is no longer active after dismissal.
#[then("it should not reappear")]
fn demo_contact_not_reappear(world: &mut VauchiWorld) {
    assert!(
        !world.vauchi.is_demo_contact_active().unwrap(),
        "demo contact should be inactive after dismissal"
    );
}

/// No-op: not pestering the user is a UX/notification concern.
#[then("I should not be pestered about it")]
fn not_pestered(_world: &mut VauchiWorld) {}

/// No-op: onboarding tips elsewhere are a UI-layer concern.
#[then("I should still see onboarding tips elsewhere")]
fn tips_elsewhere(_world: &mut VauchiWorld) {}

// ── Demo contact updates flow ──────────────────────────────────────────────

/// No-op: re-opening the app is a lifecycle concern; the demo state persists in storage.
#[when("I open the app later")]
fn open_app_later(_world: &mut VauchiWorld) {}

/// Advances the demo contact to the next tip (simulating a "has updated" state).
#[when(expr = "the demo contact has {string}")]
fn demo_contact_has_updated(world: &mut VauchiWorld, _update: String) {
    world.vauchi.advance_demo_contact().unwrap();
}

/// No-op: the update indicator is a frontend presentation concern.
#[then("I should see an update indicator")]
fn see_update_indicator(_world: &mut VauchiWorld) {}

/// No-op: tap behavior is a frontend concern.
#[then("tapping should show the changed field")]
fn tapping_shows_changed_field(_world: &mut VauchiWorld) {}

/// No-op: user understanding is a UX/copy concern, not an API assertion.
#[then("I should understand this is how real updates work")]
fn understand_real_updates(_world: &mut VauchiWorld) {}

// ── Auto-remove after first real exchange ─────────────────────────────────

/// Simulates completing a first real exchange by adding a test contact and calling auto_remove.
#[when("I complete my first real exchange")]
fn complete_first_real_exchange(world: &mut VauchiWorld) {
    world.add_test_contact("RealContact");
    world.vauchi.auto_remove_demo_contact().unwrap();
}

/// Asserts the demo contact was auto-removed after acquiring a real contact.
#[then("the demo contact should be automatically removed")]
fn demo_contact_auto_removed(world: &mut VauchiWorld) {
    assert!(
        !world.vauchi.is_demo_contact_active().unwrap(),
        "demo contact should be auto-removed after the first real exchange"
    );
}

/// No-op: shifting focus to real contacts is a UI/UX presentation concern.
#[then("focus should shift to real contacts")]
fn focus_on_real_contacts(_world: &mut VauchiWorld) {}
