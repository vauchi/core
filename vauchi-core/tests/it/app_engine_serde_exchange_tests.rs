// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine serde roundtrip tests, entry detail delete/undo, exchange session wiring,
//! and the onboarding random actions proptest (CC-13).

use proptest::prelude::*;
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, FormDialogType, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::exchange::mode::ExchangeMode;

// ── AppScreen / FormDialogType serde roundtrip tests ─────────────────

// @internal
#[test]
fn app_screen_serde_roundtrip_simple_variants() {
    let screens = vec![
        AppScreen::Onboarding,
        AppScreen::MyInfo,
        AppScreen::Contacts,
        AppScreen::Exchange,
        AppScreen::Settings,
        AppScreen::Help,
        AppScreen::Backup,
        AppScreen::Lock,
        AppScreen::DeviceLinking,
        AppScreen::DuressPin,
        AppScreen::EmergencyShred,
        AppScreen::DeliveryStatus,
        AppScreen::Sync,
        AppScreen::Recovery,
        AppScreen::Groups,
        AppScreen::Privacy,
        AppScreen::Support,
        AppScreen::ContactDuplicates,
        AppScreen::ContactLimit,
        AppScreen::MultiStageExchange {
            mode: ExchangeMode::Glance,
        },
    ];
    for screen in &screens {
        let json = serde_json::to_string(screen).expect("serialize");
        let deserialized: AppScreen = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&deserialized, screen, "roundtrip failed for {screen:?}");
    }
}

// @internal
#[test]
fn app_screen_serde_roundtrip_parameterized_variants() {
    let screens = vec![
        AppScreen::ContactDetail {
            contact_id: "contact-123".into(),
        },
        AppScreen::ContactEdit {
            contact_id: "contact-456".into(),
        },
        AppScreen::ContactVisibility {
            contact_id: "contact-789".into(),
        },
        AppScreen::GroupDetail {
            group_id: "group-1".into(),
        },
        AppScreen::MyInfoEntryDetail {
            field_id: "field-0".into(),
        },
        AppScreen::ContactMerge {
            primary_name: "Alice".into(),
            primary_fields: vec!["phone".into()],
            secondary_name: "Bob".into(),
            secondary_fields: vec!["email".into()],
        },
        AppScreen::FormDialog {
            dialog_type: FormDialogType::EditName {
                current_name: "Alice".into(),
            },
        },
    ];
    for screen in &screens {
        let json = serde_json::to_string(screen).expect("serialize");
        let deserialized: AppScreen = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&deserialized, screen, "roundtrip failed for {screen:?}");
    }
}

// @internal
#[test]
fn form_dialog_type_serde_roundtrip() {
    let variants = vec![
        FormDialogType::AddField {
            available_groups: vec![("g1".into(), "Family".into())],
        },
        FormDialogType::EditField {
            field_id: "f1".into(),
            field_label: "Phone".into(),
            current_value: "+1234".into(),
            current_note: None,
        },
        FormDialogType::EditName {
            current_name: "Alice".into(),
        },
        FormDialogType::EditRelayUrl {
            current_url: "https://relay.example.com".into(),
        },
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).expect("serialize");
        let deserialized: FormDialogType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&deserialized, variant, "roundtrip failed for {variant:?}");
    }
}

// @internal
#[test]
fn app_screen_form_dialog_serde_roundtrip() {
    let screen = AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Bob".into(),
        },
    };
    let json = serde_json::to_string(&screen).expect("serialize");
    let deserialized: AppScreen = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized, screen);
}

// ── entry detail delete / undo tests ─────────────────────────────────

// @internal
#[test]
fn entry_detail_delete_returns_show_toast_with_undo() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    // Add a field to delete
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1234567890", 0);
    let field_id = field.id().to_string();
    let mut card = vauchi
        .own_card()
        .expect("own_card should succeed")
        .expect("card should exist");
    card.add_field(field).unwrap();
    vauchi.update_own_card(&card).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfoEntryDetail {
        field_id: field_id.clone(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    match result {
        ActionResult::ShowToast {
            message,
            undo_action_id,
        } => {
            assert!(message.contains("deleted"), "toast should mention deletion");
            assert!(undo_action_id.is_some(), "should have undo action_id");
            let undo_id = undo_action_id.unwrap();
            assert!(
                undo_id.contains(&field_id),
                "undo_id should reference field"
            );
        }
        other => panic!("Expected ShowToast, got {:?}", other),
    }
    // Should have navigated back to MyInfo
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

// @internal
#[test]
fn entry_detail_delete_undo_restores_field() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1234567890", 0);
    let field_id = field.id().to_string();
    let mut card = vauchi
        .own_card()
        .expect("own_card should succeed")
        .expect("card should exist");
    card.add_field(field).unwrap();
    vauchi.update_own_card(&card).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfoEntryDetail {
        field_id: field_id.clone(),
    });

    // Delete the field
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete".into(),
    });
    let undo_id = match result {
        ActionResult::ShowToast { undo_action_id, .. } => undo_action_id.unwrap(),
        other => panic!("Expected ShowToast, got {:?}", other),
    };

    // Verify field is gone
    let card = engine.vauchi().own_card().unwrap().unwrap();
    assert!(
        card.fields().iter().all(|f| f.id() != field_id),
        "field should be deleted"
    );

    // Undo
    let result = engine.handle_action(UserAction::UndoPressed { action_id: undo_id });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "undo should return UpdateScreen"
    );

    // Verify field is restored
    let card = engine.vauchi().own_card().unwrap().unwrap();
    assert!(
        card.fields().iter().any(|f| f.id() == field_id),
        "field should be restored after undo"
    );
}

// ── ADR-031: AppEngine exchange round-trip tests ──────────────────

// @internal
#[test]
fn exchange_screen_with_identity_has_session() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let static_public_id = vauchi.public_id().unwrap_or_default();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);

    // Mode selection is the first screen — canonical tab-root id `exchange`.
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange");

    // Pick TapHoverShake — graduated to the multi-stage engine (P2.D of the
    // TapHoverShake plan). It now hands off via StartMultiStageExchange and
    // the AppEngine navigates to the multi-stage screen, which owns the
    // ADR-031 session wiring (the legacy ShowQr screen is unreachable; the
    // ephemeral-QR-from-session check now lives on the multi-stage path).
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:fun".into(),
        item_id: "mode:tap_hover_shake".into(),
    });

    // The handoff navigates to the multi-stage exchange screen and the
    // AppEngine creates the backing session (ADR-031 wiring).
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "multi_stage_exchange");
    assert!(
        engine.multi_stage_session_active(),
        "AppEngine must create the multi-stage session on handoff (ADR-031 wiring)"
    );
    let _ = static_public_id;
}

// @internal
#[test]
fn exchange_hardware_event_delegated_to_session() {
    use vauchi_core::Event;

    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);

    // Send a BLE discovery event — should be handled by the session
    let result = engine.handle_hardware_event(Event::BleDeviceDiscovered {
        id: "device-1".into(),
        rssi: -42,
        adv_data: vec![],
    });

    // Should return Some (handled by session)
    assert!(
        result.is_some(),
        "Hardware event should be handled by the session"
    );
}

// @internal
#[test]
fn exchange_hardware_unavailable_shows_toast() {
    use vauchi_core::Event;

    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);

    let result = engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "BLE".into(),
    });

    match result {
        Some(ActionResult::ShowToast { message, .. }) => {
            assert!(message.contains("BLE"), "Toast should mention BLE");
        }
        other => panic!("Expected ShowToast, got {:?}", other),
    }
}

// ── Pair 4 — MultiStageExchange routing ──────────────────────────

// @internal
#[test]
fn multi_stage_exchange_navigates_to_engine_idle_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "multi_stage_exchange");
    // Initial screen must show the active chrome with cancel + switch
    // camera, not the success or failed terminal screens.
    // The active screen's switch/cancel buttons live inside the preview
    // `Row`'s `ActionList` (so they sit beside the camera preview).
    let mut ids: Vec<String> = screen.actions.iter().map(|a| a.id.clone()).collect();
    collect_row_action_ids(&screen.components, &mut ids);
    let action_ids: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    assert!(action_ids.contains(&"cancel"));
    assert!(action_ids.contains(&"switch_camera"));
    assert!(!action_ids.contains(&"done"));
    assert!(!action_ids.contains(&"retry"));
}

/// Collect `ActionList` item ids from components, recursing into `Row`
/// containers (the active exchange screen nests its buttons there).
fn collect_row_action_ids(components: &[vauchi_app::ui::Component], out: &mut Vec<String>) {
    use vauchi_app::ui::Component;
    for c in components {
        match c {
            Component::ActionList { items, .. } => {
                out.extend(items.iter().map(|i| i.id.clone()));
            }
            Component::Row { items, .. } => collect_row_action_ids(items, out),
            _ => {}
        }
    }
}

// @internal
#[test]
fn multi_stage_exchange_camera_permission_denied_event_swaps_chrome() {
    use vauchi_core::Event;
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let result = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    // AppEngine routes hardware-error events through a ShowToast
    // ui_override before the engine's own UpdateScreen — that's the
    // existing pattern shared with `ExchangeEngine`. The user sees the
    // toast immediately; the screen state on the next render flips to
    // the engine's permission-required chrome.
    assert!(
        result.is_some(),
        "permission denial must be handled by the multi-stage engine",
    );
    let screen = engine.current_screen();
    let action_ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert!(
        action_ids.contains(&"grant_camera_permission"),
        "engine must surface Grant Permission action after camera denial; got actions {action_ids:?}",
    );
}

// @internal
#[test]
fn multi_stage_exchange_cancel_action_completes() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    // AppEngine routes ActionResult::Complete by navigating away — the
    // exact target is policy (typically Exchange parent tab), but the
    // engine's Complete signal must propagate without crash and the
    // screen must change away from multi_stage_exchange.
    let screen_id_after = engine.current_screen().screen_id.clone();
    assert_ne!(
        screen_id_after, "multi_stage_exchange",
        "Complete must navigate away (got result: {result:?})",
    );
}

// ── stateful proptest: onboarding random actions (CC-13) ─────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Random sequences of UserActions fired at a fresh AppEngine never
    /// panic and always produce a non-empty screen_id. This satisfies
    /// CC-13 (stateful property tests for state machines).
// @internal
    #[test]
    fn onboarding_random_actions_never_panic(
        actions in prop::collection::vec(
            prop_oneof![
                Just(UserAction::ActionPressed { action_id: "create_new".into() }),
                Just(UserAction::ActionPressed { action_id: "have_identity".into() }),
                Just(UserAction::ActionPressed { action_id: "continue".into() }),
                Just(UserAction::ActionPressed { action_id: "skip".into() }),
                Just(UserAction::ActionPressed { action_id: "back".into() }),
                Just(UserAction::ActionPressed { action_id: "start_app".into() }),
                Just(UserAction::ActionPressed { action_id: "exchange".into() }),
                Just(UserAction::ActionPressed { action_id: "import_contacts".into() }),
                ".*".prop_map(|s| UserAction::TextChanged {
                    component_id: "display_name".into(),
                    value: s,
                }),
            ],
            0..30
        )
    ) {
        let vauchi = Vauchi::in_memory().unwrap();
        let mut engine = AppEngine::new(vauchi);
        for action in actions {
            // Result intentionally discarded — proptest asserts no-panic + non-empty screen_id
            let _ = engine.handle_action(action);
            let screen = engine.current_screen();
            prop_assert!(!screen.screen_id.is_empty(),
                "screen_id must never be empty");
        }
    }
}
