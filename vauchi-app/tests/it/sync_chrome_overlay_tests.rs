// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AppEngine sync-chrome overlay
//! (`apply_sync_chrome_overlay`).
//!
//! Design: `_private/docs/designs/2026-05-28-sync-chrome-overlay-design.md`.
//! Replaces iOS `HomeView.SyncStatusIndicator` per G1 of
//! `2026-05-02-ios-humble-ui-deep-retirement`.

use vauchi_app::ui::{
    ActionResult, AppEngine, Component, IndicatorKind, UserAction, WorkflowEngine,
};
use vauchi_core::ImportSource;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;

fn test_engine() -> AppEngine {
    let vauchi = Vauchi::in_memory().unwrap();
    AppEngine::new(vauchi)
}

fn add_contact(engine: &AppEngine, name: &str) {
    let contact = Contact::from_import(
        format!("contact-{name}"),
        ContactCard::new(name),
        ImportSource::VcardFile,
        None,
        0,
    );
    engine.vauchi().add_contact(contact).unwrap();
}

/// Engine whose user has completed onboarding far enough to sync:
/// an identity and at least one contact.
fn test_engine_with_contact() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    add_contact(&engine, "Bob");
    engine
}

fn find_sync_indicator(
    components: &[Component],
) -> Option<(&str, &IndicatorKind, &Option<String>)> {
    components.iter().find_map(|c| match c {
        Component::Indicator {
            id,
            label,
            kind,
            action_id,
            ..
        } if id == "sync" => Some((label.as_str(), kind, action_id)),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Contact gate: no contacts → no sync chip (nobody to sync with)
// ---------------------------------------------------------------------------

// @internal
#[test]
fn no_contacts_emit_no_sync_indicator() {
    // A fresh install (identity, but zero contacts) must not offer
    // sync: there is nobody to sync with. Owner rule 2026-07-31:
    // sync only becomes available once at least one contact exists.
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    let screen = engine.current_screen();
    assert!(
        find_sync_indicator(&screen.components).is_none(),
        "sync indicator must not be emitted while the contact list is empty"
    );
}

// @internal
#[test]
fn sync_indicator_appears_once_first_contact_exists() {
    // The gate is evaluated lazily on every emit: adding the first
    // contact must surface the chip on the next current_screen().
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    assert!(find_sync_indicator(&engine.current_screen().components).is_none());

    add_contact(&engine, "Bob");
    assert!(
        find_sync_indicator(&engine.current_screen().components).is_some(),
        "sync indicator should appear once a contact exists"
    );
}

// ---------------------------------------------------------------------------
// Default Idle state → Neutral sync chip on every emitted screen
// ---------------------------------------------------------------------------

// @internal
#[test]
fn idle_status_emits_neutral_sync_indicator_with_tap_action() {
    // On engine boot, sync_chrome_status defaults to Idle, so the
    // overlay should inject a Component::Indicator with kind=Neutral,
    // label="Sync", action_id=Some("sync_now") on every emitted
    // top-level screen (given a contact exists — see the gate tests).
    let engine = test_engine_with_contact();
    let screen = engine.current_screen();
    let (label, kind, action_id) =
        find_sync_indicator(&screen.components).expect("sync indicator missing on idle screen");
    assert_eq!(label, "Sync");
    assert_eq!(*kind, IndicatorKind::Neutral);
    assert_eq!(action_id.as_deref(), Some("sync_now"));
}

// @internal
#[test]
fn sync_indicator_appears_first_in_components() {
    // The overlay inserts at index 0 so chrome stays at the top of
    // the screen body — frontends with a chrome region (toolbar)
    // can render it there, frontends without render it inline.
    let engine = test_engine_with_contact();
    let screen = engine.current_screen();
    let first = screen.components.first().expect("no components");
    assert!(
        matches!(first, Component::Indicator { id, .. } if id == "sync"),
        "expected Indicator id=\"sync\" at index 0, got {first:?}"
    );
}

// @internal
#[test]
fn fixed_layout_screen_has_no_sync_indicator() {
    // The QR exchange screen is `ScreenLayout::Fixed` and must not
    // reflow — the sync chrome's state changes would shift the QR the
    // peer is scanning and break the camera lock. The overlay must skip
    // fixed-layout screens (`2026-06-03-exchange-qr-scan-stability`).
    use vauchi_app::ui::{AppScreen, ScreenLayout};
    use vauchi_core::exchange::mode::ExchangeMode;

    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let screen = engine.current_screen();
    assert_eq!(
        screen.layout,
        ScreenLayout::Fixed,
        "the exchange screen must be Fixed layout"
    );
    assert!(
        find_sync_indicator(&screen.components).is_none(),
        "a Fixed-layout screen must NOT carry the reflowing sync indicator"
    );
}

// @internal
#[test]
fn pinned_layout_screen_keeps_sync_indicator() {
    // Unlike Fixed (QR no-reflow contract), Pinned screens delegate
    // scrolling to their list component but may still reflow — the sync
    // chrome must render (`2026-06-11-contacts-list-windowing-design`).
    use vauchi_app::ui::{AppScreen, ScreenLayout};

    let mut engine = test_engine_with_contact();
    engine.navigate_to(AppScreen::Contacts);
    let screen = engine.current_screen();
    assert_eq!(
        screen.layout,
        ScreenLayout::Pinned,
        "the contacts screen must be Pinned layout"
    );
    assert!(
        find_sync_indicator(&screen.components).is_some(),
        "a Pinned-layout screen must keep the sync indicator"
    );
}

// ---------------------------------------------------------------------------
// Offline → overlay skipped (apply_offline_overlay Banner handles it)
// ---------------------------------------------------------------------------

// @internal
#[test]
fn offline_skips_sync_chrome_overlay() {
    // The offline overlay already injects a Component::Banner with
    // "You're offline. Changes will sync when you reconnect." — adding
    // a sync chip on top would be redundant and visually noisy.
    // apply_sync_chrome_overlay early-returns when network_online is
    // false.
    let mut engine = test_engine();
    engine.set_network_online(false);
    let screen = engine.current_screen();
    assert!(
        find_sync_indicator(&screen.components).is_none(),
        "sync indicator should not be emitted while offline"
    );
}

// @internal
#[test]
fn going_back_online_re_enables_sync_chrome_overlay() {
    // Flipping back to online restores the chip — the state read
    // is lazy, so toggling network_online flips the emission on the
    // next current_screen() call without any explicit re-init.
    let mut engine = test_engine_with_contact();
    engine.set_network_online(false);
    assert!(find_sync_indicator(&engine.current_screen().components).is_none());

    engine.set_network_online(true);
    assert!(
        find_sync_indicator(&engine.current_screen().components).is_some(),
        "sync indicator should re-emit after network_online flips back to true"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

// @internal
#[test]
fn apply_sync_chrome_overlay_is_idempotent_across_renders() {
    // Calling current_screen() multiple times in a row must not
    // accumulate sync indicators. Each emission walks the existing
    // components and skips if any Indicator with id="sync" is
    // already present — mirroring the four sibling overlays.
    let engine = test_engine_with_contact();
    let screen_a = engine.current_screen();
    let screen_b = engine.current_screen();
    let count_a = screen_a
        .components
        .iter()
        .filter(|c| matches!(c, Component::Indicator { id, .. } if id == "sync"))
        .count();
    let count_b = screen_b
        .components
        .iter()
        .filter(|c| matches!(c, Component::Indicator { id, .. } if id == "sync"))
        .count();
    assert_eq!(count_a, 1, "first render emits exactly one sync indicator");
    assert_eq!(
        count_b, 1,
        "second render also emits exactly one (idempotent)"
    );
}

// ---------------------------------------------------------------------------
// sync_now action — handler responds, returns UpdateScreen
// ---------------------------------------------------------------------------

// @internal
#[test]
fn sync_now_action_returns_update_screen() {
    // The chrome chip's action_id="sync_now" must reach a real
    // AppEngine handler arm (no orphan affordance per CC-22).
    // In non-network builds the arm is a no-op other than refreshing
    // the screen; in network builds it calls Vauchi::sync() and
    // updates sync_chrome_status. Either way, the result is
    // UpdateScreen — never NoHandler or panicking.
    let mut engine = test_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "sync_now must return ActionResult::UpdateScreen, got {result:?}"
    );
}

// @internal
#[test]
fn sync_now_action_pressed_does_not_panic() {
    // Smoke test: the sync_now handler arm must not panic, even
    // when there is no identity and no relay key cached. In test
    // builds without network-http, the body is a no-op. In network
    // builds, Vauchi::sync() returns NoIdentity / NotConnected
    // outcomes which the arm leaves the state unchanged for. Either
    // way the call completes and the engine emits a fresh ScreenModel.
    let mut engine = test_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });
    // Second call to confirm we can still emit a screen afterward.
    let screen = engine.current_screen();
    assert!(
        !screen.components.is_empty(),
        "engine still emits a populated screen after sync_now"
    );
}

// ---------------------------------------------------------------------------
// sync_now must establish the OHTTP session before syncing
// ---------------------------------------------------------------------------

// A fresh process has no OHTTP key; `Vauchi::sync()` then returns
// `NotConnected` without doing anything. The Sync chip is the only sync
// trigger the shells have, so it must connect first — and report the
// attempt's failure instead of leaving the chip on "Sync" as if nothing
// had been asked. Seen on two Android phones 2026-09-09: every tap on
// Sync after an app start was a silent no-op.
// @scenario: sync_chrome :: Sync now connects before syncing
#[cfg(feature = "network-http")]
#[test]
fn sync_now_without_a_session_attempts_to_connect_and_reports_failure() {
    use vauchi_core::api::VauchiConfig;
    use vauchi_core::crypto::SymmetricKey;

    let dir = tempfile::tempdir().expect("tempdir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url("http://127.0.0.1:1")
        .with_ohttp_relay_url("http://127.0.0.1:2");
    let mut vauchi = Vauchi::new(config).expect("vauchi");
    vauchi.create_identity("Alice").expect("identity");
    assert!(!vauchi.has_ohttp_key(), "test premise: no session yet");
    let mut engine = AppEngine::new(vauchi);
    add_contact(&engine, "Bob");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "sync_now".into(),
    });

    let screen = engine.current_screen();
    let (label, kind, _) = find_sync_indicator(&screen.components).expect("sync indicator present");
    assert_eq!(
        (label, kind),
        ("Sync failed", &IndicatorKind::Error),
        "sync_now must try to connect (which fails against an unreachable relay) \
         rather than silently skipping the sync"
    );
}

// ---------------------------------------------------------------------------
// Throttled syncs tell the user when the next one may run
// ---------------------------------------------------------------------------

// Core throttles syncs (60 s jitter, 30 s–5 min after an exchange) and
// answers `TooSoon` with the remaining wait. The chip must show that
// wait instead of pretending nothing was asked
// (backlog 2026-09-09-manual-sync-within-throttle-gives-no-feedback).
// @scenario: sync_chrome :: Throttled sync shows the remaining wait
#[test]
fn throttled_sync_outcome_puts_the_remaining_wait_on_the_chip() {
    use vauchi_app::ui::SyncChromeStatus;
    use vauchi_core::api::VauchiSyncOutcome;

    let status = SyncChromeStatus::after_outcome(
        &VauchiSyncOutcome::TooSoon {
            retry_after_secs: 42,
        },
        SyncChromeStatus::Idle,
        1_000,
    );
    assert_eq!(
        status,
        SyncChromeStatus::Throttled {
            retry_after_secs: 42
        }
    );
    let (label, kind) = status.chip();
    assert_eq!(
        (label.as_str(), kind),
        ("Sync in 42 s", IndicatorKind::Neutral)
    );

    // A successful sync stamps the completion time; other outcomes keep
    // the previous status.
    assert!(matches!(
        SyncChromeStatus::after_outcome(
            &VauchiSyncOutcome::NotConnected,
            SyncChromeStatus::Failed,
            1_000
        ),
        SyncChromeStatus::Failed
    ));
}
