// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings link-row routing (GTK-3/QT-3 in
//! `_private/docs/testing/2026-08-07-exploratory-core-v0.60.1-verification.md`).
//!
//! Prepared-surface shells deliver settings row taps as opaque interaction
//! activations; the projection must route them through the same
//! `ListItemSelected` contract that `list()`/`action_list()` use and that
//! `intercept_settings_action` + `SettingsEngine` handle. The intercept must
//! also fire on the Advanced sub-screen (`relay_url`, `failed_deliveries`
//! live there). These tests drive the full chain: project → reduce the
//! activation → dispatch → assert the landing screen.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, FormDialogType, PreparedSurface, UserAction,
    WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event, InteractionId, PresentationNode, SurfaceId};

fn engine_on(screen: AppScreen) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(screen.clone());
    assert_eq!(*engine.current_app_screen(), screen);
    engine
}

fn project(engine: &AppEngine, surface_id: &SurfaceId) -> PreparedSurface {
    PreparedSurface::from_screen(surface_id.clone(), 1, &engine.current_screen())
        .expect("settings screen projects to a generic surface")
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
        panic!("settings screen projects atomically");
    };
    let mut ids = Vec::new();
    walk(&surface.nodes, &mut ids);
    ids
}

/// The action a shell would deliver when the user taps the row for
/// `item_id`, resolved through the same opaque-event path shells use.
fn row_action(prepared: &PreparedSurface, surface_id: &SurfaceId, item_id: &str) -> UserAction {
    row_activations(prepared)
        .into_iter()
        .filter_map(|interaction_id| {
            prepared
                .reduce(Event::ActionActivated {
                    surface_id: surface_id.clone(),
                    interaction_id,
                })
                .ok()
        })
        .find(|action| match action {
            UserAction::ListItemSelected { item_id: id, .. } => id == item_id,
            UserAction::ActionPressed { action_id } => action_id == item_id,
            _ => false,
        })
        .unwrap_or_else(|| panic!("no settings row routes item id `{item_id}`"))
}

fn tap_row(engine: &mut AppEngine, item_id: &str) -> ActionResult {
    let surface_id = SurfaceId::new("settings.test").unwrap();
    let prepared = project(engine, &surface_id);
    let action = row_action(&prepared, &surface_id, item_id);
    engine.handle_action(action)
}

/// Every interactive settings row must reduce to the `ListItemSelected`
/// contract — the variant `intercept_settings_action` and
/// `SettingsEngine::handle_action` match on (ActionPressed routes nowhere).
#[track_caller]
fn assert_rows_reduce_to_list_item_selected(engine: &AppEngine, expected: &[(&str, &str)]) {
    let surface_id = SurfaceId::new("settings.test").unwrap();
    let prepared = project(engine, &surface_id);
    for (component_id, item_id) in expected {
        let action = row_action(&prepared, &surface_id, item_id);
        assert_eq!(
            action,
            UserAction::ListItemSelected {
                component_id: (*component_id).into(),
                item_id: (*item_id).into(),
            },
            "settings row `{item_id}` must reduce to ListItemSelected"
        );
    }
}

// @internal
#[test]
fn main_settings_rows_reduce_to_list_item_selected() {
    let engine = engine_on(AppScreen::Settings);
    assert_rows_reduce_to_list_item_selected(
        &engine,
        &[
            ("profile", "display_name"),
            ("profile", "edit_profile"),
            ("security_backup", "change_password"),
            ("security_backup", "devices"),
            ("security_backup", "duress_pin"),
            ("security_backup", "decoy_contacts"),
            ("security_backup", "backup_export"),
            ("security_backup", "backup_import"),
            ("security_backup", "setup_new_device"),
            ("security_backup", "last_backup"),
            ("security_backup", "backup_reminders"),
            ("help_about", "help_center"),
            ("help_about", "funding"),
            ("help_about", "privacy_policy"),
            ("help_about", "what_is_vauchi"),
            ("help_about", "version"),
            ("advanced_nav", "advanced"),
        ],
    );
}

// @internal
#[test]
fn advanced_settings_rows_reduce_to_list_item_selected() {
    let engine = engine_on(AppScreen::SettingsAdvanced);
    assert_rows_reduce_to_list_item_selected(
        &engine,
        &[
            ("network", "relay_url"),
            ("delivery", "pending_updates"),
            ("delivery", "failed_deliveries"),
            ("danger", "emergency_wipe"),
        ],
    );
}

// @internal
#[test]
fn tapping_help_center_navigates_to_help_screen() {
    let mut engine = engine_on(AppScreen::Settings);
    let result = tap_row(&mut engine, "help_center");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "help_center tap must navigate, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::Help);
}

// @internal
#[test]
fn tapping_edit_profile_navigates_to_my_info() {
    let mut engine = engine_on(AppScreen::Settings);
    let result = tap_row(&mut engine, "edit_profile");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "edit_profile tap must navigate, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::MyInfo);
}

// @internal
#[test]
fn tapping_change_password_navigates_to_change_password() {
    let mut engine = engine_on(AppScreen::Settings);
    let result = tap_row(&mut engine, "change_password");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "change_password tap must navigate, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::ChangePassword);
}

// @internal
#[test]
fn tapping_backup_export_navigates_to_backup_screen() {
    let mut engine = engine_on(AppScreen::Settings);
    let result = tap_row(&mut engine, "backup_export");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "backup_export tap must navigate, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::Backup);
}

// @internal
#[test]
fn tapping_funding_opens_supporters_url() {
    let mut engine = engine_on(AppScreen::Settings);
    let result = tap_row(&mut engine, "funding");
    assert!(
        matches!(
            result,
            ActionResult::OpenUrl { ref url } if url == "https://vauchi.app/docs/about/supporters"
        ),
        "funding tap must open the supporters URL, got {result:?}"
    );
}

// @internal
#[test]
fn tapping_advanced_link_navigates_to_settings_advanced() {
    let mut engine = engine_on(AppScreen::Settings);
    let result = tap_row(&mut engine, "advanced");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "advanced tap must navigate, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::SettingsAdvanced);
}

// @internal
#[test]
fn advanced_tapping_relay_url_opens_edit_dialog() {
    let mut engine = engine_on(AppScreen::SettingsAdvanced);
    let result = tap_row(&mut engine, "relay_url");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "relay_url tap must navigate, got {result:?}"
    );
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::FormDialog {
                dialog_type: FormDialogType::EditRelayUrl { .. }
            }
        ),
        "relay_url tap must open the EditRelayUrl dialog, landed on {:?}",
        engine.current_app_screen()
    );
}

// @internal
#[test]
fn advanced_tapping_failed_deliveries_navigates_to_delivery_status() {
    let mut engine = engine_on(AppScreen::SettingsAdvanced);
    let result = tap_row(&mut engine, "failed_deliveries");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "failed_deliveries tap must navigate, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::DeliveryStatus);
}

// @internal
#[test]
fn advanced_tapping_emergency_wipe_shows_inline_confirm() {
    let mut engine = engine_on(AppScreen::SettingsAdvanced);
    let _ = tap_row(&mut engine, "emergency_wipe");
    assert_eq!(
        *engine.current_app_screen(),
        AppScreen::SettingsAdvanced,
        "emergency_wipe keeps the inline-confirm flow on the advanced screen"
    );
    assert!(
        engine
            .current_screen()
            .components
            .iter()
            .any(|component| matches!(
                component,
                Component::InlineConfirm { id, .. } if id == "emergency_wipe"
            )),
        "emergency_wipe tap must reveal the inline confirmation"
    );
}

// @internal
#[test]
fn tapping_what_is_vauchi_shows_info_overlay() {
    let mut engine = engine_on(AppScreen::Settings);
    let result = tap_row(&mut engine, "what_is_vauchi");
    assert!(
        matches!(
            result,
            ActionResult::ShowInfoOverlay {
                ref title,
                ref body
            } if !title.is_empty() && !body.is_empty()
        ),
        "what_is_vauchi tap must show the info overlay, got {result:?}"
    );
}

// @internal
#[test]
fn tapping_backup_reminders_cycles_frequency() {
    let mut engine = engine_on(AppScreen::Settings);
    let _ = tap_row(&mut engine, "backup_reminders");
    let screen = engine.current_screen();
    let detail = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::SettingsGroup { items, .. } => items.iter().find_map(|item| {
                (item.id == "backup_reminders").then(|| match &item.kind {
                    vauchi_app::ui::SettingsItemKind::Link { detail } => detail.clone(),
                    other => panic!("backup_reminders stays a Link row, got {other:?}"),
                })
            }),
            _ => None,
        })
        .expect("backup_reminders row still rendered");
    assert_eq!(
        detail.as_deref(),
        Some("Monthly"),
        "tap cycles Weekly -> Monthly"
    );
}

/// Value rows (version) have no destination; a tap must be a harmless
/// no-op that keeps the user on Settings — not a navigation.
// @internal
#[test]
fn tapping_version_value_row_stays_on_settings() {
    let mut engine = engine_on(AppScreen::Settings);
    let _ = tap_row(&mut engine, "version");
    assert_eq!(*engine.current_app_screen(), AppScreen::Settings);
}

/// Legacy (ScreenModel) renderers emit `ListItemSelected` directly; the
/// Advanced sub-screen rows must route for them too — the intercept's
/// screen guard used to drop every advanced-screen row on every shell.
// @internal
#[test]
fn legacy_list_item_selected_on_advanced_routes_relay_url() {
    let mut engine = engine_on(AppScreen::SettingsAdvanced);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "network".into(),
        item_id: "relay_url".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "relay_url selection must navigate, got {result:?}"
    );
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::FormDialog {
                dialog_type: FormDialogType::EditRelayUrl { .. }
            }
        ),
        "landed on {:?}",
        engine.current_app_screen()
    );
}

// @internal
#[test]
fn legacy_list_item_selected_on_advanced_routes_failed_deliveries() {
    let mut engine = engine_on(AppScreen::SettingsAdvanced);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "delivery".into(),
        item_id: "failed_deliveries".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "failed_deliveries selection must navigate, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::DeliveryStatus);
}
