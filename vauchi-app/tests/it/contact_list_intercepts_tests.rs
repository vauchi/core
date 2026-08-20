// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `AppEngine` intercepts on the Contacts screen.
//!
//! `ContactListEngine` emits screen actions whose routing lives in
//! `AppEngine` (not the engine itself), so this file guards against
//! regressions where an action id on a Primary-styled button ends up
//! being a no-op because the intercept was removed or moved.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

const DISMISS_DEMO_CONTACT_ACTION_ID: &str = "dismiss_demo_contact";

fn has_demo_banner(screen: &vauchi_app::ui::ScreenModel) -> bool {
    screen.components.iter().any(|c| {
        matches!(
            c,
            Component::Banner { action_id, .. } if action_id == DISMISS_DEMO_CONTACT_ACTION_ID
        )
    })
}

fn new_vauchi_with_identity() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    vauchi
}

// @internal
#[test]
fn pressing_add_contact_on_contacts_navigates_to_exchange() {
    let vauchi = new_vauchi_with_identity();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let before = engine.current_screen();
    // Tier-0 (c) narrow collapse: the Contacts screen now reports the
    // canonical `AppScreen::screen_id()` (`contacts`) on the wire, not the
    // `ContactListEngine` sub-state id (`contact_list`).
    assert_eq!(before.screen_id, "contacts");

    // The Primary "Add Contact" button surfaces this action id.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_contact".into(),
    });

    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(
                screen.screen_id, "exchange",
                "add_contact must route to the Exchange tab root (canonical \
                 `exchange` id, so the nav bar renders), got screen_id={}",
                screen.screen_id
            );
        }
        other => panic!(
            "pressing add_contact on Contacts must return NavigateTo(Exchange), got: {other:?}"
        ),
    }
}

// @internal
#[test]
fn pressing_go_exchange_on_contacts_navigates_to_exchange() {
    // Companion coverage for the `go_exchange` intercept, which is
    // emitted on the empty Contacts state and shares the same target
    // as `add_contact`. Kept as a sibling test so a refactor that
    // removes one intercept without the other is caught immediately.
    let vauchi = new_vauchi_with_identity();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "go_exchange".into(),
    });

    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(screen.screen_id, "exchange");
        }
        other => panic!("go_exchange must route to Exchange, got: {other:?}"),
    }
}

// Regression for 2026-05-25-contact-tap-opens-own-card.
//
// Tapping a contact row must navigate to that contact's detail. The
// frontend forwards a generic `ListItemSelected` and renders whatever
// `ScreenModel` core returns — core must resolve the navigation to
// `NavigateTo(contact_detail)`, NOT ship a raw `open_contact` result that
// each frontend has to map to a "contact_detail" screen itself. That
// domain-leaking contract is what broke: `route_result` had no general
// `OpenContact` arm, so the result fell through raw and the mobile
// frontends wrongly navigated to My Card.
// @internal
#[test]
fn tapping_a_contact_navigates_to_contact_detail() {
    let vauchi = new_vauchi_with_identity();
    let vcf = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n";
    vauchi.import_contacts_from_vcf(vcf).unwrap();
    let bob_id = vauchi.list_contacts().unwrap()[0].id().to_string();

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "contacts".into(),
        item_id: bob_id.clone(),
    });

    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(
                screen.screen_id, "contact_detail",
                "tapping a contact must navigate to contact_detail (core-resolved, \
                 no domain leak to the frontend), got screen_id={}",
                screen.screen_id
            );
        }
        other => panic!(
            "tapping a contact must return NavigateTo(contact_detail); a raw \
             open_contact result forces frontends to know domain screen ids. Got: {other:?}"
        ),
    }
}

// @internal
#[test]
fn demo_banner_appears_on_contacts_screen_when_demo_active() {
    // The onboarding demo contact lives as a `DomainCommand`-driven
    // Vauchi state today. Per the shell-purity audit
    // (`_private/docs/investigations/2026-05-28-core-screen-composition-surface.md`),
    // core emits the demo affordance as a `Component::Banner` from the
    // Contacts screen so frontends render it as a generic Banner instead
    // of owning a custom `DemoContactCard` view (~90 LOC on iOS).
    let vauchi = new_vauchi_with_identity();
    vauchi.initialize_demo_contact().expect("init demo");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    assert!(
        has_demo_banner(&engine.current_screen()),
        "Contacts screen must carry a Banner with action_id={DISMISS_DEMO_CONTACT_ACTION_ID} when demo is active",
    );
}

// @internal
#[test]
fn demo_banner_absent_on_non_contacts_screens() {
    // The demo banner is scoped to the Contacts screen. On MyInfo or
    // any other root the banner must not appear — otherwise the
    // overlay leaks app-chrome semantics that don't belong on the
    // home tab.
    let vauchi = new_vauchi_with_identity();
    vauchi.initialize_demo_contact().expect("init demo");
    let engine = AppEngine::new(vauchi);
    // engine_with_identity defaults to MyInfo

    assert!(
        !has_demo_banner(&engine.current_screen()),
        "Demo banner must not appear on the default (MyInfo) screen",
    );
}

// @internal
#[test]
fn demo_banner_absent_when_demo_not_active() {
    // Without `initialize_demo_contact`, the demo state is inactive
    // and no banner should be emitted even on the Contacts screen.
    let vauchi = new_vauchi_with_identity();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    assert!(
        !has_demo_banner(&engine.current_screen()),
        "Demo banner must not appear when demo is not active",
    );
}

// @internal
#[test]
fn dismiss_demo_contact_action_removes_banner() {
    // Pressing the dismiss action surfaced on the demo banner clears
    // the demo state in Vauchi; subsequent renders of the Contacts
    // screen must not re-emit the banner.
    let vauchi = new_vauchi_with_identity();
    vauchi.initialize_demo_contact().expect("init demo");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);
    assert!(
        has_demo_banner(&engine.current_screen()),
        "precondition: demo banner is present",
    );

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: DISMISS_DEMO_CONTACT_ACTION_ID.into(),
    });

    assert!(
        !has_demo_banner(&engine.current_screen()),
        "Demo banner must disappear after dismiss action is dispatched",
    );
}

/// Importing contacts is offered on the Contacts screen.
///
/// The action lived only in the More menu, which is being retired, and
/// `contacts.import_contacts` was already in the locale file with no
/// caller — the affordance belongs here, next to `view_archived` and
/// `find_duplicates`.
// @internal
#[test]
fn contacts_screen_offers_importing_contacts() {
    let vauchi = new_vauchi_with_identity();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let screen = engine.current_screen();
    assert!(
        screen
            .contextual_actions
            .iter()
            .any(|a| a.id == "import_contacts"),
        "Contacts must offer import_contacts; actions: {:?}",
        screen
            .contextual_actions
            .iter()
            .map(|a| &a.id)
            .collect::<Vec<_>>()
    );
}

/// Pressing it asks the shell for a file rather than navigating.
// @internal
#[test]
fn importing_contacts_asks_the_shell_for_a_file() {
    let vauchi = new_vauchi_with_identity();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "import_contacts".into(),
    });

    match result {
        ActionResult::Commands { ref commands } => assert!(
            commands
                .iter()
                .any(|c| matches!(c, vauchi_core::Command::FilePickFromUser { .. })),
            "import_contacts must request a file pick, got: {commands:?}"
        ),
        other => panic!("import_contacts must return Commands, got: {other:?}"),
    }
}
