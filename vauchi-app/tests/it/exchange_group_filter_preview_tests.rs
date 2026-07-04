// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The exchange field-preview must reflect the group filter: selecting a
//! group whose `visible_fields` exclude a card field renders that field as
//! `Hidden` in the "You will share" preview, matching what the BLE payload
//! actually transmits (`ble_handshake.rs build_ble_session_inputs`).
//!
//! End-to-end through the production `AppEngine` flow
//! (GroupSelection → ModeSelection → FieldPreview), so it pins the whole
//! seam: `screens.rs` populates the group data, the engine resolves the
//! allow-list, and `field_preview` renders it.
//!
//! Problem: 2026-06-08-exchange-card-not-group-filtered (G2 Slice 4).

use vauchi_app::ui::{
    AppEngine, AppScreen, Component, UiFieldVisibility, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};

/// AppEngine over an in-memory Vauchi whose own card has `Email` + `Phone`,
/// and a "Work" group exposing only `Email`. Returns the engine and the Work
/// group id (the `group_picker` item id).
fn engine_with_card_and_work_group() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut card = vauchi
        .own_card()
        .expect("own_card")
        .expect("create_identity saves a card");
    card.add_field(ContactField::new(FieldType::Email, "Email", "a@b.com", 0))
        .expect("add email");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Phone",
        "+12025550123",
        0,
    ))
    .expect("add phone");
    vauchi.update_own_card(&card).expect("update own card");
    let email_id = card
        .fields()
        .iter()
        .find(|f| f.label() == "Email")
        .expect("email field")
        .id()
        .to_string();
    let work = vauchi.create_group("Work").expect("create group");
    let work_id = work.id().to_string();
    vauchi
        .set_group_field_visibility(&work_id, &email_id, true)
        .expect("expose email to Work");
    (AppEngine::new(vauchi), work_id)
}

/// Drives the engine from the Exchange entry to the FieldPreview screen,
/// toggling each group in `groups_to_select`, then returns the preview's
/// (label, visibility) pairs.
fn preview_field_visibilities(
    engine: &mut AppEngine,
    groups_to_select: &[&str],
) -> Vec<(String, UiFieldVisibility)> {
    // Groups exist → group-first entry lands on the group picker.
    let _ = engine.navigate_to(AppScreen::Exchange);
    for group in groups_to_select {
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "group_picker".into(),
            item_id: (*group).into(),
        });
    }
    // Continue past group selection → mode picker (mode is None in production).
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // Pick a grouped mode that routes through the field preview.
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:fun".into(),
        item_id: "mode:tap_hover_shake".into(),
    });

    let screen = engine.current_screen();
    assert_eq!(
        screen.screen_id, "exchange_field_preview",
        "continue must land on the field preview"
    );
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::FieldList { fields, .. } => Some(
                fields
                    .iter()
                    .map(|f| (f.label.clone(), f.visibility.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .expect("preview has a FieldList")
}

fn visibility_of(pairs: &[(String, UiFieldVisibility)], label: &str) -> UiFieldVisibility {
    pairs
        .iter()
        .find(|(l, _)| l == label)
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| panic!("no field labelled {label} in preview: {pairs:?}"))
}

// @internal
#[test]
fn selecting_group_hides_non_group_fields_in_preview() {
    let (mut engine, work) = engine_with_card_and_work_group();
    let pairs = preview_field_visibilities(&mut engine, &[&work]);
    assert_eq!(
        visibility_of(&pairs, "Email"),
        UiFieldVisibility::Shown,
        "Work exposes Email → Shown"
    );
    assert_eq!(
        visibility_of(&pairs, "Phone"),
        UiFieldVisibility::Hidden,
        "Work does not expose Phone → must be Hidden in the preview"
    );
}

// NOTE (M2 S7): the "continue with zero groups → preview shows all" case
// was removed — under the unified Skip/Continue button an empty selection
// takes the skip path and no preview renders (the preview is a selection
// refinement, never a gate). The share-all resolver semantics that case
// pinned stay covered at the unit level:
// `group_filter::tests::no_groups_selected_returns_none_share_all`.

// @internal
#[test]
fn selecting_empty_group_hides_all_fields_in_preview() {
    // Default-closed: a selected group that exposes nothing must render every
    // field Hidden in the preview — NOT share-all. Regression for the
    // empty-set footgun (Slice 4b): `Some(∅)` ≠ `None`.
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut card = vauchi
        .own_card()
        .expect("own_card")
        .expect("create_identity saves a card");
    card.add_field(ContactField::new(FieldType::Email, "Email", "a@b.com", 0))
        .expect("add email");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Phone",
        "+12025550123",
        0,
    ))
    .expect("add phone");
    vauchi.update_own_card(&card).expect("update own card");
    // "Empty" group: created but no fields exposed (no set_group_field_visibility).
    let empty = vauchi.create_group("Empty").expect("create group");
    let empty_id = empty.id().to_string();

    let mut engine = AppEngine::new(vauchi);
    let pairs = preview_field_visibilities(&mut engine, &[&empty_id]);
    assert_eq!(
        visibility_of(&pairs, "Email"),
        UiFieldVisibility::Hidden,
        "empty group exposes nothing → Email Hidden"
    );
    assert_eq!(
        visibility_of(&pairs, "Phone"),
        UiFieldVisibility::Hidden,
        "empty group exposes nothing → Phone Hidden (not share-all)"
    );
}
