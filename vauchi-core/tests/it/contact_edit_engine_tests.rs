// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

// ── Test helpers ────────────────────────────────────────────────────

fn sample_contact() -> EditableContact {
    EditableContact {
        display_name: "Alice".into(),
        fields: vec![
            EditableField {
                id: "phone1".into(),
                field_type: "phone".into(),
                label: "Phone".into(),
                value: "+41 79 123 45 67".into(),
                visible_to_groups: vec!["Family".into()],
                shown: true,
            },
            EditableField {
                id: "email1".into(),
                field_type: "email".into(),
                label: "Email".into(),
                value: "alice@example.com".into(),
                visible_to_groups: vec![],
                shown: false,
            },
        ],
    }
}

fn sample_groups() -> Vec<String> {
    vec!["Family".into(), "Friends".into(), "Work".into()]
}

fn make_engine() -> ContactEditEngine {
    ContactEditEngine::new(sample_contact(), sample_groups())
}

// ── Tests ───────────────────────────────────────────────────────────

// @internal
#[test]
fn edit_starts_at_fields() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "edit_fields");
    let progress = screen.progress.expect("should have progress");
    assert_eq!(progress.current_step, 1);
    assert_eq!(progress.total_steps, 3);
}

// @internal
#[test]
fn edit_fields_has_name_input_and_field_list() {
    let engine = make_engine();
    let screen = engine.current_screen();

    match &screen.components[0] {
        Component::TextInput {
            id, label, value, ..
        } => {
            assert_eq!(id, "display_name");
            assert_eq!(label, "Display Name");
            assert_eq!(value, "Alice");
        }
        other => panic!("Expected TextInput, got {:?}", other),
    }

    // Second component: Divider
    assert_eq!(screen.components[1], Component::Divider);

    // Third component: FieldList
    match &screen.components[2] {
        Component::FieldList {
            id,
            fields,
            visibility_mode,
            available_groups,
            ..
        } => {
            assert_eq!(id, "fields");
            assert_eq!(fields.len(), 2);
            assert_eq!(visibility_mode, &VisibilityMode::ShowHide);
            assert_eq!(available_groups, &sample_groups());
        }
        other => panic!("Expected FieldList, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_change_name_updates_screen() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Bob".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => match &screen.components[0] {
            Component::TextInput { value, .. } => assert_eq!(value, "Bob"),
            other => panic!("Expected TextInput, got {:?}", other),
        },
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }

    assert_eq!(engine.edited_contact().display_name, "Bob");
}

// @internal
#[test]
fn edit_empty_name_shows_validation_error() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::ValidationError {
            component_id,
            message,
        } => {
            assert_eq!(component_id, "display_name");
            assert_eq!(message, "Name is required");
        }
        other => panic!("Expected ValidationError, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_continue_to_visibility() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_visibility");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 2);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_visibility_shows_toggle_lists() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "edit_visibility");

    // One ToggleList per field (2 fields)
    assert_eq!(screen.components.len(), 2);

    match &screen.components[0] {
        Component::ToggleList {
            id, label, items, ..
        } => {
            assert_eq!(id, "vis_phone1");
            assert_eq!(label, "Phone");
            assert_eq!(items.len(), 3); // Family, Friends, Work
            // Family should be selected (phone1 is visible to Family)
            assert!(
                items
                    .iter()
                    .find(|i| i.id == "Family")
                    .expect("Family")
                    .selected
            );
            assert!(
                !items
                    .iter()
                    .find(|i| i.id == "Friends")
                    .expect("Friends")
                    .selected
            );
        }
        other => panic!("Expected ToggleList, got {:?}", other),
    }

    match &screen.components[1] {
        Component::ToggleList {
            id, label, items, ..
        } => {
            assert_eq!(id, "vis_email1");
            assert_eq!(label, "Email");
            // email1 has no groups
            assert!(items.iter().all(|i| !i.selected));
        }
        other => panic!("Expected ToggleList, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_toggle_group_visibility() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "vis_phone1".into(),
        item_id: "Friends".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => match &screen.components[0] {
            Component::ToggleList { items, .. } => {
                let friends = items.iter().find(|i| i.id == "Friends").expect("Friends");
                assert!(friends.selected, "Friends should now be selected");
            }
            other => panic!("Expected ToggleList, got {:?}", other),
        },
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }

    let phone_field = engine
        .edited_contact()
        .fields
        .iter()
        .find(|f| f.id == "phone1")
        .expect("phone1");
    assert!(phone_field.visible_to_groups.contains(&"Friends".into()));
    assert!(phone_field.visible_to_groups.contains(&"Family".into()));
}

// @internal
#[test]
fn edit_continue_to_preview() {
    let mut engine = make_engine();
    // Step 1 → 2
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // Step 2 → 3
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_preview");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 3);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_preview_shows_card() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "edit_preview");
    assert_eq!(screen.components.len(), 1);

    match &screen.components[0] {
        Component::Preview {
            name,
            fields,
            variants,
            selected_variant,
            ..
        } => {
            assert_eq!(name, "Alice");
            assert_eq!(fields.len(), 2);
            assert_eq!(variants.len(), 3); // Family, Friends, Work
            assert!(selected_variant.is_none());
        }
        other => panic!("Expected CardPreview, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_preview_group_selection() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::GroupViewSelected {
        group_name: Some("Family".into()),
    });

    match result {
        ActionResult::UpdateScreen(screen) => match &screen.components[0] {
            Component::Preview {
                selected_variant, ..
            } => {
                assert_eq!(selected_variant.as_deref(), Some("Family"));
            }
            other => panic!("Expected CardPreview, got {:?}", other),
        },
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_save_returns_complete() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn edit_back_from_visibility_to_fields() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_fields");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 1);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_back_from_preview_to_visibility() {
    let mut engine = make_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "edit_visibility");
            let progress = screen.progress.expect("should have progress");
            assert_eq!(progress.current_step, 2);
        }
        other => panic!("Expected NavigateTo, got {:?}", other),
    }
}

// @internal
#[test]
fn edit_data_persists_across_steps() {
    let mut engine = make_engine();

    // Change name in step 1
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Updated Alice".into(),
    });

    // Step 1 → 2
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // Step 2 → 3
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    match &screen.components[0] {
        Component::Preview { name, .. } => {
            assert_eq!(name, "Updated Alice");
        }
        other => panic!("Expected CardPreview, got {:?}", other),
    }

    assert_eq!(engine.edited_contact().display_name, "Updated Alice");
}

// Wire-level contract: `Component::Preview` carries core-derived `initials`
// so frontends never recompute `displayName.take(1)` (ADR-021/043 Humble UI).
//
// @internal
#[test]
fn edit_preview_carries_core_derived_initials() {
    let contact = EditableContact {
        display_name: "Alice Smith".into(),
        fields: vec![],
    };
    let mut engine = ContactEditEngine::new(contact, sample_groups());

    // Step 1 → 2 → 3 (Preview)
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    let Component::Preview { initials, .. } = &screen.components[0] else {
        panic!(
            "Expected Component::Preview, got {:?}",
            &screen.components[0]
        );
    };
    assert_eq!(
        initials, "AS",
        "initials are first letters of first two words, uppercased"
    );
}

// Wire-level contract: `Component::Preview.visible_fields` never includes
// `UiFieldVisibility::Hidden` entries, even when the raw `fields` list does.
//
// This pins the privacy guarantee that the
// `2026-05-21-component-preview-legacy-fields` problem record protects:
// frontends that mistakenly render `fields` (Windows pre-Tier-1 still does,
// TUI did until vauchi/tui!301) leak Hidden values to the rendered card.
// Until the legacy `fields` field is removed from the wire, this test
// guards that `visible_fields` is the safe-to-render list.
//
// @internal
#[test]
fn edit_preview_visible_fields_drop_hidden_entries() {
    let contact = EditableContact {
        display_name: "Alice".into(),
        fields: vec![
            EditableField {
                id: "phone1".into(),
                field_type: "phone".into(),
                label: "Phone".into(),
                value: "+41 79 123 45 67".into(),
                visible_to_groups: vec![], // No groups
                shown: true,               // Shown
            },
            EditableField {
                id: "secret_email".into(),
                field_type: "email".into(),
                label: "Personal Email".into(),
                value: "secret@example.com".into(),
                visible_to_groups: vec![], // No groups
                shown: false,              // Hidden
            },
        ],
    };
    let mut engine = ContactEditEngine::new(contact, sample_groups());

    // Step 1 → 2 → 3 (Preview)
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });

    let screen = engine.current_screen();
    let Component::Preview {
        fields,
        visible_fields,
        selected_variant,
        ..
    } = &screen.components[0]
    else {
        panic!(
            "Expected Component::Preview, got {:?}",
            &screen.components[0]
        );
    };

    // Both fields appear in the raw `fields` list (the legacy wire shape).
    assert_eq!(
        fields.len(),
        2,
        "raw `fields` contains both Shown and Hidden entries"
    );
    let has_hidden_in_fields = fields
        .iter()
        .any(|f| matches!(f.visibility, UiFieldVisibility::Hidden));
    assert!(
        has_hidden_in_fields,
        "raw `fields` must include the Hidden entry (this test would not be meaningful otherwise)"
    );

    // No variant selected yet — preview shows the global default.
    assert!(
        selected_variant.is_none(),
        "test precondition: no variant selected"
    );

    // The wire-level contract: `visible_fields` MUST NOT contain any
    // Hidden-visibility entry. A renderer that binds on this list (linux-gtk,
    // linux-qt, iOS, TUI post-MR-301) cannot accidentally leak hidden values.
    assert!(
        visible_fields
            .iter()
            .all(|f| !matches!(f.visibility, UiFieldVisibility::Hidden)),
        "visible_fields leaked Hidden entry: {:?}",
        visible_fields
            .iter()
            .filter(|f| matches!(f.visibility, UiFieldVisibility::Hidden))
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>()
    );

    // And specifically: the Shown field is present, the Hidden one is gone.
    let visible_ids: Vec<_> = visible_fields.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(visible_ids, vec!["phone1"]);
}
