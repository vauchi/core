// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden JSON fixtures for Phase 1–3 workflow engine screens.
//!
//! Generates canonical JSON for each engine's default screen, consumed
//! by frontend contract tests.
//!
//! Verify freshness: `cargo test -p vauchi-core --test engine_golden_fixtures`
//! Regenerate all:   `cargo test -p vauchi-core --test engine_golden_fixtures -- --ignored`

use std::fs;
use std::path::PathBuf;
use vauchi_app::ui::*;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden")
}

fn screen_to_json(screen: &ScreenModel) -> String {
    serde_json::to_string_pretty(screen).expect("ScreenModel serialization failed")
}

fn assert_fixture_fresh(screen: &ScreenModel, filename: &str) {
    let json = screen_to_json(screen);
    let path = fixtures_dir().join(filename);

    if path.exists() {
        let existing = fs::read_to_string(&path).unwrap();
        // Normalize CRLF → LF so fixtures work on any OS/git checkout config.
        assert_eq!(
            existing.replace("\r\n", "\n").trim(),
            json.trim(),
            "Golden fixture `{}` is stale! Regenerate with:\n  \
             cargo test -p vauchi-core --test engine_golden_fixtures -- --ignored",
            filename
        );
    } else {
        fs::create_dir_all(fixtures_dir()).unwrap();
        fs::write(&path, &json).unwrap();
    }
}

// ── Sample data builders ─────────────────────────────────────────

fn sample_contacts() -> Vec<IndexedItem> {
    vec![
        IndexedItem::from(Item {
            id: "c1".into(),
            name: "Alice".into(),
            subtitle: Some("Friend".into()),
            avatar_initials: "A".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }),
        IndexedItem::from(Item {
            id: "c2".into(),
            name: "Bob".into(),
            subtitle: None,
            avatar_initials: "B".into(),
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
            answer_url: Some("https://docs.vauchi.app/users/faq#contacts--exchange".into()),
            category: "Getting Started".into(),
        },
        HelpItem {
            id: "faq2".into(),
            question: "What is a duress PIN?".into(),
            answer: Some("A secondary PIN that triggers data protection.".into()),
            answer_url: Some("https://docs.vauchi.app/users/faq#privacy--security".into()),
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

// ── Per-engine freshness tests ───────────────────────────────────

// @internal
#[test]
fn home_fixture_is_fresh() {
    let engine = MyInfoEngine::new(MyInfoProgress {
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
    );
    assert_fixture_fresh(&engine.current_screen(), "home.json");
}

// @internal
#[test]
fn home_empty_fixture_is_fresh() {
    let engine = MyInfoEngine::new(MyInfoProgress {
        completed_steps: 6,
        total_steps: 6,
    });
    assert_fixture_fresh(&engine.current_screen(), "home_empty.json");
}

// @internal
#[test]
fn contact_list_fixture_is_fresh() {
    let engine = ContactListEngine::new(sample_contacts());
    assert_fixture_fresh(&engine.current_screen(), "contact_list.json");
}

// @internal
#[test]
fn settings_fixture_is_fresh() {
    let engine = SettingsEngine::new(sample_settings_config());
    assert_fixture_fresh(&engine.current_screen(), "settings.json");
}

// @internal
#[test]
fn help_fixture_is_fresh() {
    let engine = HelpEngine::new(sample_help_items());
    assert_fixture_fresh(&engine.current_screen(), "help.json");
}

// @internal
#[test]
fn delivery_status_fixture_is_fresh() {
    let engine = DeliveryStatusEngine::new(sample_delivery_items());
    assert_fixture_fresh(&engine.current_screen(), "delivery_status.json");
}

// @internal
#[test]
fn delivery_empty_fixture_is_fresh() {
    let engine = DeliveryStatusEngine::new(vec![]);
    assert_fixture_fresh(&engine.current_screen(), "delivery_empty.json");
}

// @internal
#[test]
fn lock_screen_fixture_is_fresh() {
    let engine = LockScreenEngine::new(5);
    assert_fixture_fresh(&engine.current_screen(), "lock_screen.json");
}

// ── ContactEditEngine fixtures ───────────────────────────────────

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

// @internal
#[test]
fn contact_edit_fields_fixture_is_fresh() {
    let engine = ContactEditEngine::new(sample_editable_contact(), sample_edit_groups());
    assert_fixture_fresh(&engine.current_screen(), "contact_edit_fields.json");
}

// @internal
#[test]
fn contact_edit_visibility_fixture_is_fresh() {
    let mut engine = ContactEditEngine::new(sample_editable_contact(), sample_edit_groups());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert_fixture_fresh(&engine.current_screen(), "contact_edit_visibility.json");
}

// @internal
#[test]
fn contact_edit_preview_fixture_is_fresh() {
    let mut engine = ContactEditEngine::new(sample_editable_contact(), sample_edit_groups());
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert_fixture_fresh(&engine.current_screen(), "contact_edit_preview.json");
}

// ── Phase 3 sample data builders ─────────────────────────────────

fn sample_duress_config() -> DuressConfig {
    DuressConfig {
        enabled: false,
        available_contacts: vec![Item {
            id: "c1".into(),
            name: "Emergency Contact".into(),
            subtitle: None,
            avatar_initials: "E".into(),
            status: None,
            actions: vec![],
            a11y: None,
        }],
        selected_contact_ids: vec!["c1".into()],
        alert_message: "I may be in danger".into(),
        include_location: true,
    }
}

// ── Phase 3 per-engine freshness tests ──────────────────────────

// @internal
#[test]
fn device_linking_fixture_is_fresh() {
    let engine = DeviceLinkingEngine::new("vauchi://link?token=abc123".to_string());
    assert_fixture_fresh(&engine.current_screen(), "device_linking.json");
}

// @internal
#[test]
fn backup_choose_fixture_is_fresh() {
    let engine = BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English);
    assert_fixture_fresh(&engine.current_screen(), "backup_choose.json");
}

// @internal
#[test]
fn duress_overview_fixture_is_fresh() {
    let engine = DuressPinEngine::new(sample_duress_config(), vauchi_app::i18n::Locale::English);
    assert_fixture_fresh(&engine.current_screen(), "duress_overview.json");
}

// @internal
#[test]
fn emergency_shred_fixture_is_fresh() {
    let engine = EmergencyShredEngine::new(vauchi_app::i18n::Locale::English);
    assert_fixture_fresh(&engine.current_screen(), "emergency_shred.json");
}

// @internal
#[test]
fn emergency_broadcast_overview_fixture_is_fresh() {
    let engine = EmergencyBroadcastEngine::new(None);
    assert_fixture_fresh(
        &engine.current_screen(),
        "emergency_broadcast_overview.json",
    );
}

// ── Phase 4: single-screen nav/setup engines (screenshot catalog,
//    problem 2026-06-12-device-screenshot-catalog) ──────────────────

// @internal
#[test]
fn more_fixture_is_fresh() {
    let engine = MoreEngine::new(vauchi_app::i18n::Locale::English);
    assert_fixture_fresh(&engine.current_screen(), "more.json");
}

// @internal
#[test]
fn support_fixture_is_fresh() {
    let engine = SupportEngine::new();
    assert_fixture_fresh(&engine.current_screen(), "support.json");
}

// @internal
#[test]
fn recovery_help_fixture_is_fresh() {
    let engine = RecoveryHelpEngine::new();
    assert_fixture_fresh(&engine.current_screen(), "recovery_help.json");
}

// @internal
#[test]
fn change_password_fixture_is_fresh() {
    let engine = ChangePasswordEngine::new(true);
    assert_fixture_fresh(&engine.current_screen(), "change_password.json");
}

// @internal
#[test]
fn set_password_fixture_is_fresh() {
    // Setup mode (no password yet): 2-field "Set Password" form. Distinct
    // wire shape from change_password.json — desktop/TUI renderers need it.
    let engine = ChangePasswordEngine::new(false);
    assert_fixture_fresh(&engine.current_screen(), "set_password.json");
}

// @internal
#[test]
fn contact_limit_fixture_is_fresh() {
    let engine = ContactLimitEngine::new(150, 150);
    assert_fixture_fresh(&engine.current_screen(), "contact_limit.json");
}

// @internal
#[test]
fn archived_contacts_fixture_is_fresh() {
    let engine = ArchivedContactsEngine::new(vec![
        ("c1".into(), "Alice".into()),
        ("c2".into(), "Bob".into()),
    ]);
    assert_fixture_fresh(&engine.current_screen(), "archived_contacts.json");
}

// @internal
#[test]
fn privacy_settings_fixture_is_fresh() {
    let engine = GdprEngine::new(
        None,
        "2 contacts, 1 group".into(),
        vauchi_app::i18n::Locale::English,
    );
    assert_fixture_fresh(&engine.current_screen(), "privacy_settings.json");
}

// ── Regenerate all fixtures (run with --ignored) ─────────────────

// @internal
#[test]
#[ignore]
fn regenerate_all_engine_fixtures() {
    let dir = fixtures_dir();
    fs::create_dir_all(&dir).unwrap();

    let fixtures: Vec<(&str, ScreenModel)> = vec![
        (
            "home.json",
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
            "home_empty.json",
            MyInfoEngine::new(MyInfoProgress {
                completed_steps: 6,
                total_steps: 6,
            })
            .current_screen(),
        ),
        (
            "contact_list.json",
            ContactListEngine::new(sample_contacts()).current_screen(),
        ),
        (
            "settings.json",
            SettingsEngine::new(sample_settings_config()).current_screen(),
        ),
        (
            "help.json",
            HelpEngine::new(sample_help_items()).current_screen(),
        ),
        (
            "delivery_status.json",
            DeliveryStatusEngine::new(sample_delivery_items()).current_screen(),
        ),
        (
            "delivery_empty.json",
            DeliveryStatusEngine::new(vec![]).current_screen(),
        ),
        (
            "lock_screen.json",
            LockScreenEngine::new(5).current_screen(),
        ),
        (
            "contact_edit_fields.json",
            ContactEditEngine::new(sample_editable_contact(), sample_edit_groups())
                .current_screen(),
        ),
        {
            let mut e = ContactEditEngine::new(sample_editable_contact(), sample_edit_groups());
            let _ = e.handle_action(UserAction::ActionPressed {
                action_id: "continue".into(),
            });
            ("contact_edit_visibility.json", e.current_screen())
        },
        {
            let mut e = ContactEditEngine::new(sample_editable_contact(), sample_edit_groups());
            let _ = e.handle_action(UserAction::ActionPressed {
                action_id: "continue".into(),
            });
            let _ = e.handle_action(UserAction::ActionPressed {
                action_id: "continue".into(),
            });
            ("contact_edit_preview.json", e.current_screen())
        },
        // Phase 3 engines
        (
            "device_linking.json",
            DeviceLinkingEngine::new("vauchi://link?token=abc123".to_string()).current_screen(),
        ),
        (
            "backup_choose.json",
            BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::English)
                .current_screen(),
        ),
        (
            "duress_overview.json",
            DuressPinEngine::new(sample_duress_config(), vauchi_app::i18n::Locale::English)
                .current_screen(),
        ),
        (
            "emergency_shred.json",
            EmergencyShredEngine::new(vauchi_app::i18n::Locale::English).current_screen(),
        ),
        (
            "emergency_broadcast_overview.json",
            EmergencyBroadcastEngine::new(None).current_screen(),
        ),
        // Phase 4: single-screen nav/setup engines
        (
            "more.json",
            MoreEngine::new(vauchi_app::i18n::Locale::English).current_screen(),
        ),
        ("support.json", SupportEngine::new().current_screen()),
        (
            "recovery_help.json",
            RecoveryHelpEngine::new().current_screen(),
        ),
        (
            "change_password.json",
            ChangePasswordEngine::new(true).current_screen(),
        ),
        (
            "set_password.json",
            ChangePasswordEngine::new(false).current_screen(),
        ),
        (
            "contact_limit.json",
            ContactLimitEngine::new(150, 150).current_screen(),
        ),
        (
            "archived_contacts.json",
            ArchivedContactsEngine::new(vec![
                ("c1".into(), "Alice".into()),
                ("c2".into(), "Bob".into()),
            ])
            .current_screen(),
        ),
        (
            "privacy_settings.json",
            GdprEngine::new(
                None,
                "2 contacts, 1 group".into(),
                vauchi_app::i18n::Locale::English,
            )
            .current_screen(),
        ),
    ];

    assert_eq!(fixtures.len(), 24, "expected 24 engine fixtures");

    for (filename, screen) in &fixtures {
        let json = screen_to_json(screen);
        fs::write(dir.join(filename), &json).unwrap();
        println!("Generated {filename}");
    }
}
