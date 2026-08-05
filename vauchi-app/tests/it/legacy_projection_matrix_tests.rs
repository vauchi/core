// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Legacy screen projection matrix (ADR-066).
//!
//! Every screen the retired `ScreenModel` boundary could carry must project
//! through the generic presentation boundary with no legacy component escape
//! hatch. The matrix used to be replayed from golden JSON files; those
//! snapshots pinned the *serialization* of a boundary that no longer crosses
//! process limits, so they retired with the rest of the golden scaffolding.
//! Building the same 29 screens from live engines keeps the coverage and
//! drops the staleness: a screen shape the engines can no longer produce
//! leaves the matrix instead of drifting out of date on disk.

use vauchi_app::ui::*;

fn init_fixture_i18n() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let locales_dir = manifest_dir
        .ancestors()
        .map(|ancestor| ancestor.join("locales"))
        .find(|candidate| candidate.join("en.json").is_file())
        .expect("projection matrix tests require the workspace locales repository");
    vauchi_app::i18n::init(&locales_dir).expect("load production locale strings");
}

// ── Sample data builders ─────────────────────────────────────────

fn sample_contacts() -> Vec<IndexedItem> {
    vec![
        IndexedItem::from(Item {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: Some("Friend".into()),
            initials: "A".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
        IndexedItem::from(Item {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            initials: "B".into(),
            status: Some("Updated".into()),
            actions: vec![],
            a11y: None,
        }),
    ]
}

fn sample_settings_config() -> SettingsConfig {
    SettingsConfig {
        display_name: "Alice".into(),
        delivery_receipts_enabled: true,
        suppress_presence: false,
        new_field_default_visible: false,
        contact_added_notifications: false,
        card_update_notifications: true,
        relay_url: "https://relay.vauchi.app".into(),
        device_count: 2,
        password_set: true,
        theme_id: String::new(),
        available_themes: vec![],
        language_id: String::new(),
        available_languages: vec![],
        reduce_motion: false,
        large_touch: false,
        show_help_icons: true,
        version: String::new(),
        build: String::new(),
        pending_updates: 0,
        failed_deliveries: 0,
        debug_mode: false,
        backup_reminder_frequency: "Weekly".into(),
        last_backup_display: "Never".into(),
    }
}

fn sample_help_items() -> Vec<HelpItem> {
    vec![
        HelpItem {
            id: "faq1".into(),
            question: "How do I exchange contacts?".into(),
            answer: Some("Meet in person and use the Exchange screen.".into()),
            answer_url: Some("https://vauchi.app/docs/users/faq#contacts--exchange".into()),
            category: "Getting Started".into(),
        },
        HelpItem {
            id: "faq2".into(),
            question: "What is a duress PIN?".into(),
            answer: Some("A secondary PIN that triggers data protection.".into()),
            answer_url: Some("https://vauchi.app/docs/users/faq#privacy--security".into()),
            category: "Security".into(),
        },
    ]
}

fn sample_delivery_items() -> Vec<DeliveryItem> {
    vec![
        DeliveryItem {
            message_id: "m1".into(),
            contact_id: "c1".into(),
            contact_name: "Alice".into(),
            status: Status::Success,
            detail: Some("Delivered 2 min ago".into()),
            retryable: false,
        },
        DeliveryItem {
            message_id: "m2".into(),
            contact_id: "c2".into(),
            contact_name: "Bob".into(),
            status: Status::Failed,
            detail: Some("Relay unreachable".into()),
            retryable: true,
        },
    ]
}

fn sample_editable_contact() -> EditableContact {
    EditableContact {
        display_name: "Alice".into(),
        fields: vec![
            EditableField {
                id: "f1".into(),
                field_type: "Phone".into(),
                label: "Mobile".into(),
                value: "+1-555-0100".into(),
                visible_to_groups: vec!["Family".into()],
                shown: true,
            },
            EditableField {
                id: "f2".into(),
                field_type: "Email".into(),
                label: "Work".into(),
                value: "alice@example.com".into(),
                visible_to_groups: vec!["Friends".into(), "Work".into()],
                shown: true,
            },
        ],
    }
}

fn sample_edit_groups() -> Vec<String> {
    vec!["Family".into(), "Friends".into(), "Work".into()]
}

fn sample_duress_config() -> DuressConfig {
    DuressConfig {
        enabled: false,
        available_contacts: vec![Item {
            id: "c1".into(),
            name: "Emergency Contact".into(),
            subtitle: None,
            initials: "E".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }],
        selected_contact_ids: vec!["c1".into()],
        alert_message: "I may be in danger".into(),
        include_location: true,
    }
}

// ── Onboarding walk ──────────────────────────────────────────────

/// Walks all 6 onboarding screens in order, collecting each ScreenModel.
fn walk_onboarding_screens() -> Vec<(&'static str, ScreenModel)> {
    let mut engine = OnboardingEngine::new();
    let mut screens = Vec::new();

    // 1. IdentityCheck
    screens.push(("identity_check", engine.current_screen()));

    // 2. DeviceLinkInstructions (side-flow reached via "link_device")
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));
    screens.push(("device_link_instructions", engine.current_screen()));

    // Navigate back, then IdentityCheck -> DefaultName (via "create_new")
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 3. DefaultName (empty — captures the initial state)
    screens.push(("default_name", engine.current_screen()));

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 4. GroupsSetup
    screens.push(("groups_setup", engine.current_screen()));

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 5. ContactInfo
    screens.push(("contact_info", engine.current_screen()));

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 6. WhatNext
    screens.push(("what_next", engine.current_screen()));

    assert_eq!(screens.len(), 6, "expected exactly 6 onboarding screens");
    screens
}

// ── Engine screen matrix ─────────────────────────────────────────

/// Builds the 23 engine screens the retired golden fixtures captured.
fn engine_screen_matrix() -> Vec<(&'static str, ScreenModel)> {
    let contact_edit_visibility = {
        let mut e = ContactEditEngine::new(sample_editable_contact(), sample_edit_groups());
        let _ = e.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        e.current_screen()
    };
    let contact_edit_preview = {
        let mut e = ContactEditEngine::new(sample_editable_contact(), sample_edit_groups());
        let _ = e.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        let _ = e.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        e.current_screen()
    };

    let screens = vec![
        (
            "home",
            MyInfoEngine::new(MyInfoProgress {
                completed_steps: 3,
                total_steps: 6,
            })
            .with_own_card(
                "Alice".into(),
                vec![OwnFieldInfo {
                    field_id: "f1".into(),
                    field_type: "Phone".into(),
                    label: "Mobile".into(),
                    value: "+41 79 000 00 00".into(),
                    visible_groups: vec![],
                    contact_count: 0,
                }],
            )
            .current_screen(),
        ),
        (
            "home_empty",
            MyInfoEngine::new(MyInfoProgress {
                completed_steps: 6,
                total_steps: 6,
            })
            .current_screen(),
        ),
        (
            "contact_list",
            ContactListEngine::new(sample_contacts()).current_screen(),
        ),
        (
            "settings",
            SettingsEngine::new(sample_settings_config()).current_screen(),
        ),
        (
            "help",
            HelpEngine::new(sample_help_items()).current_screen(),
        ),
        (
            "delivery_status",
            DeliveryStatusEngine::new(sample_delivery_items()).current_screen(),
        ),
        (
            "delivery_empty",
            DeliveryStatusEngine::new(vec![]).current_screen(),
        ),
        ("lock_screen", LockScreenEngine::new(5).current_screen()),
        (
            "contact_edit_fields",
            ContactEditEngine::new(sample_editable_contact(), sample_edit_groups())
                .current_screen(),
        ),
        ("contact_edit_visibility", contact_edit_visibility),
        ("contact_edit_preview", contact_edit_preview),
        (
            "device_linking",
            DeviceLinkingEngine::new("vauchi://link?token=abc123".to_string()).current_screen(),
        ),
        (
            "backup_choose",
            BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English)
                .current_screen(),
        ),
        (
            "duress_overview",
            DuressPinEngine::new(sample_duress_config(), vauchi_app::i18n::Locale::English)
                .current_screen(),
        ),
        (
            "emergency_shred",
            EmergencyShredEngine::new(vauchi_app::i18n::Locale::English).current_screen(),
        ),
        (
            "more",
            MoreEngine::new(vauchi_app::i18n::Locale::English).current_screen(),
        ),
        ("support", SupportEngine::new().current_screen()),
        ("recovery_help", RecoveryHelpEngine::new().current_screen()),
        (
            "change_password",
            ChangePasswordEngine::new(true).current_screen(),
        ),
        (
            "set_password",
            ChangePasswordEngine::new(false).current_screen(),
        ),
        (
            "contact_limit",
            ContactLimitEngine::new(150, 150).current_screen(),
        ),
        (
            "archived_contacts",
            ArchivedContactsEngine::new(vec![
                ("c1".into(), "Alice".into()),
                ("c2".into(), "Bob".into()),
            ])
            .current_screen(),
        ),
        (
            "privacy_settings",
            GdprEngine::new(
                None,
                "2 contacts, 1 group".into(),
                vauchi_app::i18n::Locale::English,
            )
            .current_screen(),
        ),
    ];

    assert_eq!(screens.len(), 23, "expected 23 engine screens");
    screens
}

// ── Projection contract ──────────────────────────────────────────

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn every_legacy_screen_projects_without_a_legacy_component_escape_hatch() {
    init_fixture_i18n();
    let mut matrix = engine_screen_matrix();
    matrix.extend(walk_onboarding_screens());
    assert_eq!(
        matrix.len(),
        29,
        "the matrix replaces 29 golden fixtures — a dropped screen is lost coverage"
    );

    for (name, screen) in &matrix {
        PreparedSurface::from_screen(
            vauchi_core::SurfaceId::new(screen.screen_id.clone())
                .expect("matrix surface ids are valid"),
            1,
            screen,
        )
        .unwrap_or_else(|error| panic!("{name} must project: {error}"));
    }
}
