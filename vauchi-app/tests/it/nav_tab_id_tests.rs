// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `AppScreen::nav_tab_id()` — the owning bottom-nav tab for each screen,
//! surfaced on `ScreenModel.nav_tab_id` so the mobile/TUI 5-tab shells
//! stop hand-maintaining a `match AppScreen -> tab index` to highlight the
//! active tab. Unlike `parent_screen_id()` (which stops at the immediate
//! parent — `settings`, `recovery`, `tags`, … — for the desktop sidebar),
//! this resolves transitively to one of the five bottom-nav roots
//! (`my_info` / `contacts` / `exchange` / `groups` / `more`), or `None`
//! for pre-auth / transient screens that show no bottom nav.

use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The five bottom-nav tab roots each own themselves.
// @internal
#[test]
fn tab_roots_own_themselves() {
    assert_eq!(AppScreen::MyInfo.nav_tab_id().as_deref(), Some("my_info"));
    assert_eq!(
        AppScreen::Contacts.nav_tab_id().as_deref(),
        Some("contacts")
    );
    assert_eq!(
        AppScreen::Exchange.nav_tab_id().as_deref(),
        Some("exchange")
    );
    assert_eq!(AppScreen::Groups.nav_tab_id().as_deref(), Some("groups"));
    assert_eq!(AppScreen::More.nav_tab_id().as_deref(), Some("more"));
}

/// A sub-screen resolves to the bottom-nav tab that owns it — the same
/// tab `parent_screen_id()` points at when it names a tab directly.
// @internal
#[test]
fn sub_screens_resolve_to_their_owning_tab() {
    assert_eq!(
        AppScreen::ContactDetail {
            contact_id: "c1".into(),
        }
        .nav_tab_id()
        .as_deref(),
        Some("contacts"),
    );
    assert_eq!(
        AppScreen::MyInfoEntryDetail {
            field_id: "f1".into(),
        }
        .nav_tab_id()
        .as_deref(),
        Some("my_info"),
    );
    assert_eq!(
        AppScreen::GroupDetail {
            group_id: "g1".into(),
        }
        .nav_tab_id()
        .as_deref(),
        Some("groups"),
    );
    // Archived contacts is a contacts screen even though the More menu
    // links to it — the owning tab is contacts, not more.
    assert_eq!(
        AppScreen::ArchivedContacts.nav_tab_id().as_deref(),
        Some("contacts"),
    );
}

/// The top-level-non-tab screens the 5-tab shells bucket under **More**,
/// plus their deeper sub-screens which `parent_screen_id()` routes through
/// an intermediate (`settings` / `recovery` / `tags`) that this method
/// resolves the rest of the way to `more`.
// @internal
#[test]
fn more_bucket_screens_resolve_to_more() {
    for screen in [
        AppScreen::Settings,
        AppScreen::Help,
        AppScreen::Backup,
        AppScreen::DeviceManagement,
        AppScreen::Recovery,
        AppScreen::DeliveryStatus,
        AppScreen::ActivityLog,
        AppScreen::Privacy,
        AppScreen::Support,
        AppScreen::DuressPin,
        AppScreen::DeviceReplacement,
        AppScreen::DeviceLinking,
        // Resolve through an intermediate parent (settings / recovery / tags).
        AppScreen::SettingsAdvanced,
        AppScreen::ChangePassword,
        AppScreen::DecoyContacts,
        AppScreen::EmergencyShred,
        AppScreen::RecoveryHelp,
        AppScreen::RecoveryClaimReview,
        AppScreen::Tags,
        AppScreen::Places,
    ] {
        assert_eq!(
            screen.nav_tab_id().as_deref(),
            Some("more"),
            "{screen:?} must resolve to the More tab",
        );
    }
}

/// Ceremony flows launched from the Exchange tab keep it highlighted.
// @internal
#[test]
fn exchange_ceremonies_resolve_to_exchange() {
    for screen in [
        AppScreen::LinkExchange,
        AppScreen::NfcExchange,
        AppScreen::DirectTransport,
    ] {
        assert_eq!(
            screen.nav_tab_id().as_deref(),
            Some("exchange"),
            "{screen:?} must resolve to the Exchange tab",
        );
    }
}

/// Pre-auth and transient overlay screens show no bottom nav, so they own
/// no tab. `None` tells the shell to leave the current highlight alone
/// rather than force a tab.
// @internal
#[test]
fn pre_auth_and_transient_screens_own_no_tab() {
    assert_eq!(AppScreen::Lock.nav_tab_id(), None);
    assert_eq!(AppScreen::Onboarding.nav_tab_id(), None);
}

/// C4 bootstrap-chrome seam: a fresh engine with no identity boots to the
/// bootstrap screen, and its rendered `ScreenModel` carries
/// `nav_tab_id == None` — the generic "no bottom nav" consequence the 5-tab
/// shells render. The mobile TabNav gate hides the bar off the absent
/// `nav_tab_id`, so the F.home frontend migration can drop its `is_bootstrap`
/// role-boolean branch (`2026-07-06-mobile-domain-shell-violations` C4/F.home;
/// upstream-coverage seam per CC-24 — green here before the frontend branch
/// is deleted).
// @internal
#[test]
fn rendered_bootstrap_screen_shows_no_bottom_nav() {
    let engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let screen = engine.current_screen();
    assert_eq!(
        screen.screen_id, "identity_check",
        "a fresh engine with no identity boots to the bootstrap screen",
    );
    assert_eq!(
        screen.nav_tab_id, None,
        "the bootstrap screen owns no tab, so the shell hides the bottom nav",
    );
}

/// Wiring: a screen built through `AppEngine` carries the stamped
/// `nav_tab_id`, so frontends read it off the rendered `ScreenModel`
/// instead of re-deriving it from the screen id.
// @internal
#[test]
fn app_engine_stamps_nav_tab_id_on_rendered_screen() {
    let mut engine = engine_with_identity();

    engine.navigate_to(AppScreen::Settings);
    assert_eq!(
        engine.current_screen().nav_tab_id.as_deref(),
        Some("more"),
        "Settings is a More-bucket screen — the rendered ScreenModel must say so",
    );

    engine.navigate_to(AppScreen::Contacts);
    assert_eq!(
        engine.current_screen().nav_tab_id.as_deref(),
        Some("contacts"),
        "the Contacts tab root owns itself",
    );

    engine.navigate_to(AppScreen::Lock);
    assert_eq!(
        engine.current_screen().nav_tab_id,
        None,
        "the Lock screen shows no bottom nav",
    );
}
