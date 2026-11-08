// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    Component, FormDialogEngine, FormDialogType, MyInfoEngine, MyInfoProgress, OwnFieldInfo,
    UserAction, WorkflowEngine,
};

fn form(key: &str, locale: Locale) -> FormDialogEngine {
    let mut engine = FormDialogEngine::new(FormDialogType::AddField {
        available_groups: vec![],
    })
    .with_locale(locale);
    let key = if key == "social" {
        let screen = engine.current_screen();
        screen
            .components
            .iter()
            .find_map(|component| match component {
                Component::ActionList { items, .. } => items
                    .iter()
                    .find(|item| item.id.starts_with("social:"))
                    .map(|item| item.id.clone()),
                _ => None,
            })
            .expect("a social entry type")
    } else {
        key.to_owned()
    };
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_types".into(),
        item_id: key,
    });
    engine
}

fn value_input(engine: &FormDialogEngine) -> (String, String, String, String) {
    engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::TextInput {
                id,
                label,
                placeholder,
                value,
                a11y,
                ..
            } if id == "field_value" => Some((
                label,
                placeholder.expect("example"),
                value,
                a11y.and_then(|a| a.label).expect("spoken input label"),
            )),
            _ => None,
        })
        .expect("entry value input")
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn entry_value_inputs_name_the_selected_type() {
    for (key, expected) in [
        ("phone", "Phone number"),
        ("email", "Email address"),
        ("website", "Website URL"),
        ("address", "Postal address"),
        ("birthday", "Birthday"),
        ("custom", "Entry value"),
        ("social", "Account or profile URL"),
    ] {
        let (label, _, _, spoken) = value_input(&form(key, Locale::ENGLISH));
        assert_eq!(label, expected, "{key}");
        assert_eq!(spoken, format!("{expected} input"));
    }
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn downloaded_locale_copy_reloads_without_losing_a_draft() {
    let locale = Locale::from_code("qaa").expect("private-use language");
    let load = |label: &str, example: &str| {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "_meta": {"locale": "qaa", "name": "Test", "english_name": "Test", "is_rtl": false},
            "form.entry_value.email_label": label,
            "form.entry_value.email_placeholder": example,
            "form.entry_value_input_a11y": "Remote input: {label}",
            "field_type.email": "Remote entry type"
        }))
        .expect("locale JSON");
        load_locale_from_bytes("qaa", &bytes).expect("downloaded locale loader");
    };
    load("Remote email", "Remote example");
    let mut engine = form("email", locale);
    assert_eq!(engine.current_screen().title, "Add Remote entry type");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "field_value".into(),
        value: "ada@example.invalid".into(),
    });
    assert_eq!(
        value_input(&engine),
        (
            "Remote email".into(),
            "Remote example".into(),
            "ada@example.invalid".into(),
            "Remote input: Remote email".into()
        )
    );
    load("Updated email", "Updated example");
    assert_eq!(
        value_input(&engine),
        (
            "Updated email".into(),
            "Updated example".into(),
            "ada@example.invalid".into(),
            "Remote input: Updated email".into()
        )
    );
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn entry_rows_put_the_label_before_the_value_and_visibility() {
    for (label, expected) in [("Work", "Work"), ("", "Email address")] {
        let engine = MyInfoEngine::new(MyInfoProgress::default()).with_own_card(
            "Ada".into(),
            vec![OwnFieldInfo {
                field_id: "email".into(),
                field_type: "Email".into(),
                label: label.into(),
                value: "ada@example.invalid".into(),
                visible_groups: vec![],
                contact_count: 0,
                shown: false,
            }],
        );
        let screen = engine.current_screen();
        let row = screen
            .components
            .iter()
            .find_map(|component| match component {
                Component::List { id, items, .. } if id == "own_entries" => items.first(),
                _ => None,
            })
            .expect("entry row");
        assert_eq!(row.name, expected);
        assert_eq!(row.subtitle.as_deref(), Some("ada@example.invalid"));
        assert_eq!(row.status.as_deref(), Some("Hidden by default"));
        assert_eq!(
            row.a11y.as_ref().and_then(|a| a.label.as_deref()),
            Some(format!("{expected}: ada@example.invalid").as_str())
        );
    }
}
