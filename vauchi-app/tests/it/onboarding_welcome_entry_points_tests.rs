// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The onboarding welcome screen shows its two "I already have an
//! identity" entry points as body rows, not as context-bar overflow.
//!
//! The recorded Core batch for `onboarding` used to carry only the
//! "Create new identity" primary; "Link this device" and "Restore from
//! backup" were secondary contextual actions, which the context bar folds
//! into its "Actions" launcher — so a first-run user saw no way to bring
//! an existing identity along. ADR-066: shells are display-only, so the
//! rows must be in the surface's initial batch.

use vauchi_app::ui::{
    ActionResult, ActionStyle, Component, OnboardingEngine, PreparedSurface, ScreenModel, Section,
    UserAction, WorkflowEngine,
};
use vauchi_core::{Command, FilePickPurpose, PresentationNode, SurfaceId};

const HAVE_IDENTITY_CHOICES: &str = "have_identity_choices";

fn welcome() -> ScreenModel {
    let engine = OnboardingEngine::new();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "identity_check");
    screen
}

fn have_identity_section(screen: &ScreenModel) -> &Section {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SectionedActionList { id, sections } if id == HAVE_IDENTITY_CHOICES => {
                Some(sections)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no SectionedActionList `{HAVE_IDENTITY_CHOICES}` on welcome"))
        .first()
        .expect("the entry points share one labelled section")
}

fn lists_in(nodes: &[PresentationNode], out: &mut Vec<PresentationNode>) {
    for node in nodes {
        match node {
            PresentationNode::List { .. } => out.push(node.clone()),
            PresentationNode::Group { children, .. } => lists_in(children, out),
            _ => {}
        }
    }
}

fn select_row(engine: &mut OnboardingEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

// @internal
#[test]
fn welcome_lists_link_and_restore_under_an_already_have_identity_heading() {
    let screen = welcome();
    let section = have_identity_section(&screen);

    assert_eq!(section.label, "I already have an identity");
    let rows: Vec<(&str, &str, &str)> = section
        .items
        .iter()
        .map(|item| {
            (
                item.id.as_str(),
                item.label.as_str(),
                item.detail
                    .as_deref()
                    .expect("every entry point explains itself"),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "link_device",
                "Link this device",
                "Use your other signed-in device to invite this one."
            ),
            (
                "load_backup",
                "Restore from backup",
                "The password you set when you exported the backup."
            ),
        ]
    );
}

// @internal
#[test]
fn welcome_keeps_create_new_identity_as_its_single_action() {
    let screen = welcome();

    let ids: Vec<(&str, ActionStyle)> = screen
        .contextual_actions
        .iter()
        .map(|a| (a.id.as_str(), a.style.clone()))
        .collect();
    assert_eq!(
        ids,
        vec![("create_new", ActionStyle::Primary)],
        "the entry points live in the body, not behind the Actions launcher"
    );
}

// @internal
#[test]
fn welcome_initial_batch_carries_the_entry_point_rows() {
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("onboarding").unwrap(), 1, &welcome())
            .expect("welcome projects to a generic surface");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("surface projection must be atomic");
    };

    let mut lists = Vec::new();
    lists_in(&surface.nodes, &mut lists);
    let entry_points = lists
        .iter()
        .find_map(|node| match node {
            PresentationNode::List { label, rows, .. }
                if label.as_deref() == Some("I already have an identity") =>
            {
                Some(rows)
            }
            _ => None,
        })
        .expect("the initial batch has the labelled entry-point list");

    let titles: Vec<(&str, Option<&str>)> = entry_points
        .iter()
        .map(|row| (row.title.as_str(), row.subtitle.as_deref()))
        .collect();
    assert_eq!(
        titles,
        vec![
            (
                "Link this device",
                Some("Use your other signed-in device to invite this one.")
            ),
            (
                "Restore from backup",
                Some("The password you set when you exported the backup.")
            ),
        ]
    );
    assert!(
        entry_points.iter().all(|row| row.activation.is_some()),
        "every entry point row is tappable"
    );
}

// @internal
#[test]
fn choosing_link_this_device_opens_the_link_instructions() {
    let mut engine = OnboardingEngine::new();

    let result = select_row(&mut engine, HAVE_IDENTITY_CHOICES, "link_device");

    let ActionResult::NavigateTo(screen) = result else {
        panic!("the link row navigates, got {result:?}");
    };
    assert_eq!(screen.screen_id, "device_link_instructions");
}

// @internal
#[test]
fn choosing_a_row_through_its_projected_section_id_takes_the_same_path() {
    let mut engine = OnboardingEngine::new();
    let section_id = have_identity_section(&welcome()).id.clone();

    let result = select_row(
        &mut engine,
        &format!("{HAVE_IDENTITY_CHOICES}.{section_id}"),
        "link_device",
    );

    let ActionResult::NavigateTo(screen) = result else {
        panic!("the projected row id navigates too, got {result:?}");
    };
    assert_eq!(screen.screen_id, "device_link_instructions");
}

// @internal
#[test]
fn choosing_restore_from_backup_asks_for_the_backup_file() {
    let mut engine = OnboardingEngine::new();

    let result = select_row(&mut engine, HAVE_IDENTITY_CHOICES, "load_backup");

    let ActionResult::Commands { commands } = result else {
        panic!("the restore row opens the file picker, got {result:?}");
    };
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::FilePickFromUser { purpose, .. } if *purpose == FilePickPurpose::ImportBackup)),
        "restore starts with picking the backup file; the password entry follows the pick"
    );
}
