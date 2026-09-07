// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The primary navigation offers five destinations, and everything it
//! stopped offering is still reachable.
//!
//! Both `tab_info` (mobile bottom bar) and `sidebar_items` (desktop
//! sidebar, and the destination list inside the navigation overlay) used
//! to map over `available_screens`, so every shell rendered the same flat
//! fourteen-entry menu — ten-plus identical pills in which a daily
//! destination looked exactly like a rare and destructive one.
//!
//! Cutting a menu is only safe if nothing falls off the edge of the app
//! with it. `the_demoted_set_is_exactly_the_screens_with_declared_routes`
//! is the half that makes the cut safe: it derives the demoted set from
//! the code (`available_screens` minus `primary_destinations`) rather than
//! from a hand-written list, so a screen added to `available_screens`
//! without a replacement route fails here instead of shipping orphaned.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, PreparedSurface, TabLayout, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event, InteractionId, PresentationNode, SurfaceId};

/// Option A: the five destinations the nav offers, in the order it offers
/// them. Contacts and My Card are the daily pair, Exchange is the primary
/// action between them, Devices and Settings close the list.
const PRIMARY_DESTINATIONS: &[&str] = &[
    "contacts",
    "my_info",
    "exchange",
    "device_management",
    "settings",
];

/// The affordance a demoted screen is reached by.
#[derive(Clone, Copy, Debug)]
enum Route {
    /// A secondary action on the Contacts screen — where the three
    /// contact-book vocabularies belong, alongside `view_archived` and
    /// `find_duplicates`.
    ContactsAction(&'static str),
    /// A row on the Settings screen.
    SettingsRow(&'static str),
}

/// Every screen `available_screens` reaches that the nav no longer offers,
/// paired with the affordance that must reach it instead.
///
/// A pair, not a bare list: "still listed somewhere" is not reachability.
/// The test activates the affordance and asserts where the engine lands,
/// so a row that is rendered but routes nowhere — the failure mode behind
/// `2026-07-03-sync-surface-placebo` — fails exactly like a missing row.
const DEMOTED_SCREEN_ROUTES: &[(&str, Route)] = &[
    ("groups", Route::ContactsAction("groups")),
    ("tags", Route::ContactsAction("tags")),
    ("places", Route::ContactsAction("places")),
    ("recovery", Route::SettingsRow("recovery")),
    ("backup", Route::SettingsRow("backup_export")),
    ("privacy", Route::SettingsRow("privacy")),
    ("support", Route::SettingsRow("support")),
    ("help", Route::SettingsRow("help_center")),
    ("activity_log", Route::SettingsRow("activity_log")),
];

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn engine_on(screen: AppScreen) -> AppEngine {
    let mut engine = engine_with_identity();
    engine.navigate_to(screen.clone());
    assert_eq!(*engine.current_app_screen(), screen);
    engine
}

/// Activate `route` from a fresh engine and report both what the engine
/// returned and where it ended up.
#[track_caller]
fn follow(route: Route) -> (ActionResult, AppScreen) {
    let (mut engine, action) = match route {
        Route::SettingsRow(item_id) => {
            let mut engine = engine_on(AppScreen::Settings);
            let action = settings_row_action(&mut engine, item_id);
            (engine, action)
        }
        Route::ContactsAction(action_id) => {
            let engine = engine_on(AppScreen::Contacts);
            let offered = engine
                .current_screen()
                .contextual_actions
                .iter()
                .any(|action| action.id == action_id && action.enabled);
            assert!(
                offered,
                "the contacts screen offers no enabled `{action_id}` action"
            );
            let action = UserAction::ActionPressed {
                action_id: action_id.to_string(),
            };
            (engine, action)
        }
    };
    let result = engine.handle_action(action);
    let landed = engine.current_app_screen().clone();
    (result, landed)
}

fn screen_ids(screens: Vec<AppScreen>) -> Vec<String> {
    screens
        .into_iter()
        .map(|s| s.screen_id().to_string())
        .collect()
}

fn row_activations(prepared: &PreparedSurface) -> Vec<InteractionId> {
    fn walk(nodes: &[PresentationNode], out: &mut Vec<InteractionId>) {
        for node in nodes {
            match node {
                PresentationNode::List { rows, .. } => out.extend(
                    rows.iter()
                        .filter_map(|row| row.activation.as_ref())
                        .map(|action| action.interaction_id.clone()),
                ),
                PresentationNode::Group { children, .. } => walk(children, out),
                _ => {}
            }
        }
    }
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("the settings screen projects atomically");
    };
    let mut ids = Vec::new();
    walk(&surface.nodes, &mut ids);
    ids
}

/// The action a shell delivers when the user taps the settings row
/// carrying `item_id`: project the screen, reduce the opaque activation
/// the shell sends back. Going through the projection rather than
/// hand-building the `UserAction` is what makes a row that renders but
/// carries no activation fail here.
#[track_caller]
fn settings_row_action(engine: &mut AppEngine, item_id: &str) -> UserAction {
    let surface_id = SurfaceId::new("settings.test").unwrap();
    let prepared = PreparedSurface::from_screen(surface_id.clone(), 1, &engine.current_screen())
        .expect("the settings screen projects to a generic surface");
    row_activations(&prepared)
        .into_iter()
        .filter_map(|interaction_id| {
            prepared
                .reduce(Event::ActionActivated {
                    surface_id: surface_id.clone(),
                    interaction_id,
                })
                .ok()
        })
        .find(|action| {
            matches!(action, UserAction::ListItemSelected { item_id: id, .. } if id == item_id)
        })
        .unwrap_or_else(|| panic!("no settings row routes item id `{item_id}`"))
}

// @internal
#[test]
fn primary_destinations_are_the_five_option_a_screens() {
    let engine = engine_with_identity();
    assert_eq!(
        screen_ids(engine.primary_destinations()),
        PRIMARY_DESTINATIONS,
        "the nav must offer exactly the five Option A destinations, in order"
    );
}

// @internal
#[test]
fn both_shell_shapes_render_the_primary_destinations() {
    let engine = engine_with_identity();
    let tabs: Vec<String> = engine
        .tab_info(Locale::English)
        .into_iter()
        .map(|t| t.id)
        .collect();
    let sidebar: Vec<String> = engine
        .sidebar_items(Locale::English)
        .into_iter()
        .map(|t| t.id)
        .collect();

    assert_eq!(
        tabs, PRIMARY_DESTINATIONS,
        "the mobile bottom bar must render the primary destinations"
    );
    assert_eq!(
        sidebar, PRIMARY_DESTINATIONS,
        "the desktop sidebar must render the primary destinations"
    );
}

/// Before an identity exists there is nowhere to go but onboarding — the
/// pre-identity shape `available_screens` already had, kept so the cut
/// does not leak a settings pill onto the welcome screen.
// @internal
#[test]
fn before_identity_the_only_destination_is_onboarding() {
    let engine = AppEngine::new(Vauchi::in_memory().unwrap());
    assert_eq!(screen_ids(engine.primary_destinations()), ["onboarding"]);
}

/// The demoted set is derived, not transcribed: whatever
/// `available_screens` reaches and the nav no longer offers must appear in
/// `DEMOTED_SCREEN_ROUTES`. Adding a screen to `available_screens` without
/// giving it a route fails here.
// @internal
#[test]
fn the_demoted_set_is_exactly_the_screens_with_declared_routes() {
    let engine = engine_with_identity();
    let primary = screen_ids(engine.primary_destinations());
    let mut demoted: Vec<String> = screen_ids(engine.available_screens())
        .into_iter()
        .filter(|id| !primary.contains(id))
        .collect();
    demoted.sort();

    let mut declared: Vec<String> = DEMOTED_SCREEN_ROUTES
        .iter()
        .map(|(screen, _)| (*screen).to_string())
        .collect();
    declared.sort();

    assert_eq!(
        demoted, declared,
        "every screen the nav stopped offering needs a declared replacement \
         route; add it to DEMOTED_SCREEN_ROUTES and give it an affordance"
    );
}

/// A highlight id Core hands a shell must name a destination that shell
/// actually rendered.
///
/// `current_tab_id` documents exactly that — "The id matches one of the
/// `id` values returned by `tab_info` / `sidebar_items`" — and
/// `nav_tab_id` is the bottom-bar equivalent stamped on every
/// `ScreenModel`. Cutting the nav without cutting these mappings left
/// Settings pointing at `more` and Groups at a `groups` tab no shell
/// renders any more: a selection on nothing, and no error to say so.
// @internal
#[test]
fn every_top_level_screen_highlights_a_rendered_destination() {
    let engine = engine_with_identity();
    let rendered = screen_ids(engine.primary_destinations());

    for screen in engine.available_screens() {
        let stamped = screen
            .nav_tab_id()
            .unwrap_or_else(|| panic!("{screen:?} is a destination but stamps no nav tab"));
        assert!(
            rendered.contains(&stamped),
            "{screen:?} stamps nav_tab_id `{stamped}`, which is not a rendered \
             destination: {rendered:?}"
        );

        for layout in [TabLayout::Mobile, TabLayout::Desktop] {
            let probe = engine_on(screen.clone());
            let current = probe
                .current_tab_id(layout)
                .unwrap_or_else(|| panic!("{screen:?} selects no tab on {layout:?}"));
            assert!(
                rendered.iter().any(|id| id == current),
                "{screen:?} selects `{current}` on {layout:?}, which is not a \
                 rendered destination: {rendered:?}"
            );
        }
    }
}

/// Each demoted screen is reached by activating the affordance declared
/// for it, and lands on the screen that affordance claims to reach.
// @internal
#[test]
fn every_screen_demoted_from_the_nav_is_still_reachable() {
    assert!(
        !DEMOTED_SCREEN_ROUTES.is_empty(),
        "no demoted screens, so this test proved nothing"
    );

    for (screen_id, route) in DEMOTED_SCREEN_ROUTES {
        let target = AppScreen::from_screen_id(screen_id)
            .unwrap_or_else(|| panic!("`{screen_id}` does not name a screen"));

        let (result, landed) = follow(*route);

        assert!(
            matches!(result, ActionResult::NavigateTo(_)),
            "{route:?} must navigate, got {result:?}"
        );
        assert_eq!(
            landed, target,
            "`{screen_id}` is no longer in the navigation, so {route:?} is \
             its only route — it must land there"
        );
    }
}
