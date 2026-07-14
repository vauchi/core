// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `contact_detail.rs` — extracted to keep the
//! engine file under the src size limit (tidy, M3 S5-12). Loaded via
//! `#[path]`; stays a unit-test child module (private-field access
//! preserved).

// INLINE_TEST_REQUIRED: extracted from contact_detail.rs via #[path] to keep
// the engine file under the 1000-line hard limit; needs private-field access.

#[cfg(test)]
use super::*;

fn sample_contact() -> Item {
    Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: Some("+41 79 123 45 67".into()),
        initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    }
}

fn sample_fields() -> Vec<Field> {
    vec![Field {
        id: "f1".into(),
        field_type: "Phone".into(),
        label: "Mobile".into(),
        value: "+41 79 123 45 67".into(),
        icon: crate::ui::component::icon_for_field_type("Phone").into(),
        visibility: UiFieldVisibility::Shown,
        a11y: None,
    }]
}

fn sample_shared_info() -> SharedInfoView {
    SharedInfoView {
        shared_display_name: "Bob (Work)".into(),
        my_fields: vec![
            Field {
                id: "mf1".into(),
                field_type: "Email".into(),
                label: "Work Email".into(),
                value: "bob@work.com".into(),
                icon: crate::ui::component::icon_for_field_type("Email").into(),
                visibility: UiFieldVisibility::Shown,
                a11y: None,
            },
            Field {
                id: "mf2".into(),
                field_type: "Phone".into(),
                label: "Personal".into(),
                value: "+41 79 999 88 77".into(),
                icon: crate::ui::component::icon_for_field_type("Phone").into(),
                visibility: UiFieldVisibility::Hidden,
                a11y: None,
            },
        ],
        visible_groups: vec!["Work".into()],
    }
}

// @internal
#[test]
fn test_default_shows_their_info() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new());
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "contact_detail");
    assert_eq!(screen.title, "Alice");
    assert_eq!(engine.view_mode(), &ContactViewMode::TheirInfo);

    // No toggle when shared_info is None
    let has_toggle = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "view_mode"));
    assert!(!has_toggle, "Should not show toggle without shared info");
}

// @internal
#[test]
fn test_with_shared_info_shows_toggle() {
    let engine = ContactDetailEngine::with_shared_info(
        sample_contact(),
        sample_fields(),
        sample_shared_info(),
        String::new(),
    );
    let screen = engine.current_screen();

    let has_toggle = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "view_mode"));
    assert!(
        has_toggle,
        "Should show toggle when shared info is available"
    );
}

// @internal
#[test]
fn test_toggle_to_my_info_shows_shared_name() {
    let mut engine = ContactDetailEngine::with_shared_info(
        sample_contact(),
        sample_fields(),
        sample_shared_info(),
        String::new(),
    );

    // Switch to MyInfoForThem
    let result = engine.handle_action(UserAction::ItemToggled {
        component_id: "view_mode".into(),
        item_id: "my_info_for_them".into(),
    });
    assert!(matches!(result, ActionResult::UpdateScreen(_)));
    assert_eq!(engine.view_mode(), &ContactViewMode::MyInfoForThem);

    let screen = engine.current_screen();
    assert_eq!(screen.title, "Shared with Alice");

    // Should show shared display name
    let name_panel = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::InfoPanel { id, .. } if id == "shared_name_info"));
    assert!(name_panel.is_some(), "Should show shared name panel");
    if let Some(Component::InfoPanel { items, .. }) = name_panel {
        assert_eq!(items[0].detail, "Bob (Work)");
    }
}

// @internal
#[test]
fn test_toggle_to_my_info_shows_my_fields() {
    let mut engine = ContactDetailEngine::with_shared_info(
        sample_contact(),
        sample_fields(),
        sample_shared_info(),
        String::new(),
    );

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "view_mode".into(),
        item_id: "my_info_for_them".into(),
    });

    let screen = engine.current_screen();
    let field_list = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::FieldList { id, .. } if id == "my_fields"));
    assert!(field_list.is_some(), "Should show my fields");
    if let Some(Component::FieldList { fields, .. }) = field_list {
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].label, "Work Email");
        assert_eq!(fields[1].visibility, UiFieldVisibility::Hidden);
    }
}

// @internal
#[test]
fn test_toggle_back_to_their_info() {
    let mut engine = ContactDetailEngine::with_shared_info(
        sample_contact(),
        sample_fields(),
        sample_shared_info(),
        String::new(),
    );

    // Switch to MyInfoForThem then back
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "view_mode".into(),
        item_id: "my_info_for_them".into(),
    });
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "view_mode".into(),
        item_id: "their_info".into(),
    });

    assert_eq!(engine.view_mode(), &ContactViewMode::TheirInfo);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Alice");
}

// @internal
#[test]
fn test_edit_action_still_works() {
    let mut engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });
    assert_eq!(
        result,
        ActionResult::EditContact {
            contact_id: "c1".into()
        }
    );
}

// @internal
#[test]
fn test_main_screen_has_no_back_action() {
    // Back is the frontend's core-driven chrome now (gated on
    // `can_go_back`); the main contact-detail footer no longer offers a
    // "back" action (2026-06-05-core-driven-back-chrome). The not-found
    // error screen keeps its own explicit Back.
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new());
    let screen = engine.current_screen();
    assert!(
        !screen.actions.iter().any(|a| a.id == "back"),
        "main contact-detail must not offer a footer back action"
    );
}

// @internal
#[test]
fn test_contact_detail_has_no_preview_action() {
    // "What do they see?" was removed (2026-06-05-screen-ux-declutter):
    // it duplicated the on-screen "My Info for Them" perspective toggle.
    // The full preview-as flow stays reachable from My Card → "Preview
    // as…". The footer is leaner as a result.
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new());
    let screen = engine.current_screen();

    assert!(
        !screen
            .actions
            .iter()
            .any(|a| a.id.starts_with("preview-as:")),
        "contact-detail must not offer a preview-as footer action; \
         use the perspective toggle or My Card → Preview as…"
    );
}

// ===== Trust level and proposal_trusted tests =====

// @internal
#[test]
fn test_contact_detail_shows_trust_level_badge() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_trust("Verified".into(), false);
    let screen = engine.current_screen();

    // The trust level must appear as an InfoItem inside the contact_info panel.
    let contact_info = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::InfoPanel { id, .. } if id == "contact_info"));
    assert!(contact_info.is_some(), "contact_info panel must exist");
    if let Some(Component::InfoPanel { items, .. }) = contact_info {
        let trust_item = items.iter().find(|i| i.title == "Trust");
        assert!(trust_item.is_some(), "Trust InfoItem must be present");
        assert_eq!(trust_item.unwrap().detail, "Verified");
    }
}

// @internal
#[test]
fn test_contact_detail_no_trust_badge_when_trust_level_empty() {
    // Without with_trust, no Trust InfoItem should appear
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new());
    let screen = engine.current_screen();

    if let Some(Component::InfoPanel { items, .. }) = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::InfoPanel { id, .. } if id == "contact_info"))
    {
        let trust_item = items.iter().find(|i| i.title == "Trust");
        assert!(
            trust_item.is_none(),
            "Trust InfoItem must not appear when trust_level is empty"
        );
    }
}

// @internal
#[test]
fn test_contact_detail_shows_proposal_trusted_toggle() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_trust("Standard".into(), false);
    let screen = engine.current_screen();

    // A SettingsGroup with id "trust_permissions" must exist containing "proposal_trusted"
    let trust_group = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::SettingsGroup { id, .. } if id == "trust_permissions"));
    assert!(
        trust_group.is_some(),
        "trust_permissions SettingsGroup must exist"
    );
    if let Some(Component::SettingsGroup { items, .. }) = trust_group {
        let toggle = items.iter().find(|i| i.id == "proposal_trusted");
        assert!(toggle.is_some(), "proposal_trusted SettingsItem must exist");
        assert_eq!(
            toggle.unwrap().kind,
            SettingsItemKind::Toggle { enabled: false }
        );
    }
}

// @internal
#[test]
fn test_proposal_trusted_toggle_reflects_value() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_trust("High".into(), true);
    let screen = engine.current_screen();

    if let Some(Component::SettingsGroup { items, .. }) = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::SettingsGroup { id, .. } if id == "trust_permissions"))
    {
        let toggle = items.iter().find(|i| i.id == "proposal_trusted").unwrap();
        assert_eq!(
            toggle.kind,
            SettingsItemKind::Toggle { enabled: true },
            "Toggle must reflect proposal_trusted=true"
        );
    }
}

// @internal
#[test]
fn test_settings_toggled_flips_proposal_trusted() {
    let mut engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_trust("Standard".into(), false);

    assert!(!engine.proposal_trusted());

    let result = engine.handle_action(UserAction::SettingsToggled {
        component_id: "trust_permissions".into(),
        item_id: "proposal_trusted".into(),
    });

    assert!(matches!(result, ActionResult::UpdateScreen(_)));
    assert!(
        engine.proposal_trusted(),
        "proposal_trusted must be true after toggle"
    );

    // Screen must reflect new state
    if let ActionResult::UpdateScreen(screen) = result
        && let Some(Component::SettingsGroup { items, .. }) = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::SettingsGroup { id, .. } if id == "trust_permissions"))
    {
        let toggle = items.iter().find(|i| i.id == "proposal_trusted").unwrap();
        assert_eq!(toggle.kind, SettingsItemKind::Toggle { enabled: true });
    }
}

// @internal
#[test]
fn test_settings_toggled_back_to_false() {
    let mut engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_trust("Standard".into(), true);

    assert!(engine.proposal_trusted());

    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "trust_permissions".into(),
        item_id: "proposal_trusted".into(),
    });

    assert!(
        !engine.proposal_trusted(),
        "proposal_trusted must be false after second toggle"
    );
}

// ===== Delete/Archive action tests =====

// @internal
#[test]
fn test_imported_contact_shows_delete_action() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_imported(true);
    let screen = engine.current_screen();

    let delete_action = screen.actions.iter().find(|a| a.id == "delete_contact");
    assert!(
        delete_action.is_some(),
        "Imported contact must have delete_contact action"
    );
    assert_eq!(delete_action.unwrap().label, "Delete Contact");
    assert_eq!(delete_action.unwrap().style, ActionStyle::Destructive);

    let archive_action = screen.actions.iter().find(|a| a.id == "archive_contact");
    assert!(
        archive_action.is_none(),
        "Imported contact must not have archive_contact action"
    );
}

// @internal
#[test]
fn test_exchanged_contact_shows_archive_action() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_imported(false);
    let screen = engine.current_screen();

    let archive_action = screen.actions.iter().find(|a| a.id == "archive_contact");
    assert!(
        archive_action.is_some(),
        "Exchanged contact must have archive_contact action"
    );
    assert_eq!(archive_action.unwrap().label, "Archive Contact");
    assert_eq!(archive_action.unwrap().style, ActionStyle::Secondary);

    let delete_action = screen.actions.iter().find(|a| a.id == "delete_contact");
    assert!(
        delete_action.is_none(),
        "Exchanged contact must not have delete_contact action"
    );
}

// @internal
#[test]
fn test_delete_action_shows_inline_confirm() {
    let mut engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_imported(true);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });
    // Must return UpdateScreen (not ShowToast) — InlineConfirm flow
    assert!(matches!(result, ActionResult::UpdateScreen(_)));

    let screen = engine.current_screen();
    let has_confirm = screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { id, destructive, .. }
            if id == "delete_contact" && *destructive)
    });
    assert!(has_confirm, "Screen must contain InlineConfirm for delete");
}

// @internal
#[test]
fn test_archive_action_returns_show_toast_with_undo() {
    let mut engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_imported(false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "archive_contact".into(),
    });
    assert_eq!(
        result,
        ActionResult::ShowToast {
            message: "Contact archived".into(),
            undo_action_id: Some("undo_archive_contact:c1".into()),
            undo_label: Some("Undo".into()),
        }
    );
}

// @internal
#[test]
fn test_is_imported_defaults_to_false() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new());
    assert!(
        !engine.is_imported(),
        "Default contact must not be imported"
    );
}

// @internal
#[test]
fn test_with_imported_sets_flag() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_imported(true);
    assert!(engine.is_imported(), "with_imported(true) must set flag");
}

// @internal
#[test]
fn test_footer_action_id_imported_returns_delete() {
    assert_eq!(footer_action_id(true), "delete_contact");
}

// @internal
#[test]
fn test_footer_action_id_not_imported_returns_archive() {
    assert_eq!(footer_action_id(false), "archive_contact");
}

// @internal
#[test]
fn test_footer_action_id_matches_build_screen_emission() {
    // The helper must agree with what build_screen emits, so frontends
    // that switch on the helper's return value see the same id the
    // engine would put in the ScreenModel.
    for is_imported in [true, false] {
        let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
            .with_imported(is_imported);
        let screen = engine.current_screen();
        let expected_id = footer_action_id(is_imported);
        let footer_action_present = screen.actions.iter().any(|a| a.id == expected_id);
        assert!(
            footer_action_present,
            "build_screen must emit ScreenAction with id `{}` for is_imported={}",
            expected_id, is_imported
        );
    }
}
