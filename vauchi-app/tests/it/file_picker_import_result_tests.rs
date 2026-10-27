// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Import outcomes are ready to render through the canonical shell boundary.
//! Fixtures enter through the same runtime loader used after CDN verification;
//! private-use language tags prove there is no built-in language allowlist.

use vauchi_app::i18n::load_locale_from_bytes;
use vauchi_app::ui::{AppEngine, AppScreen, RenderContext, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event, PresentationNode};

const BOB: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:bob-1\r\nFN:Bob\r\nEND:VCARD\r\n";
const CAROL: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:carol-2\r\nFN:Carol\r\nEND:VCARD\r\n";

const DOWNLOADED_LOCALE: &[u8] = br#"{
    "import_contacts.result_imported": "Downloaded import: {count}",
    "import_contacts.result_skipped": "Downloaded skips: {count}",
    "import_contacts.result_complete": "Downloaded result",
    "import.warning.duplicate_uid": "Downloaded duplicate: {uid}"
}"#;

fn engine(locale: &str, existing: &str) -> AppEngine {
    load_locale_from_bytes(locale, DOWNLOADED_LOCALE).unwrap();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    if !existing.is_empty() {
        assert_eq!(
            vauchi
                .import_contacts_from_vcf(existing.as_bytes())
                .unwrap()
                .imported,
            1
        );
    }
    let mut engine = AppEngine::new(vauchi);
    engine.set_render_context(RenderContext {
        locale: Some(locale.into()),
        ..Default::default()
    });
    assert_eq!(engine.render_context().resolved_locale().code(), locale);
    engine.navigate_to(AppScreen::Contacts);
    engine.initial_commands().unwrap();
    engine
}

fn pick(engine: &mut AppEngine, text: &str) -> Vec<Command> {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "import_contacts".into(),
    });
    engine
        .dispatch(Event::FilePickedFromUser {
            bytes: text.as_bytes().to_vec(),
            filename: "contacts.vcf".into(),
        })
        .unwrap()
}

fn messages(commands: &[Command]) -> Vec<(&str, &str)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::ShowToast { toast } => Some(("", toast.message.as_str())),
            Command::PresentAlert { alert } => Some((alert.title.as_str(), alert.message.as_str())),
            _ => None,
        })
        .collect()
}

// @scenario: contact_import.feature :: Import contacts from a vCard file
#[test]
fn successful_import_sends_localized_counts_without_shell_formatting() {
    let mut engine = engine("qia", "");
    let commands = pick(&mut engine, &format!("{BOB}{CAROL}"));
    assert_eq!(messages(&commands), vec![("", "Downloaded import: 2")]);
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 2);
}

// @scenario: contact_import.feature :: Import contacts from a vCard file
#[test]
fn empty_import_sends_localized_zero_count() {
    let mut engine = engine("qib", "");
    let commands = pick(&mut engine, "not a vCard");
    assert_eq!(messages(&commands), vec![("", "Downloaded import: 0")]);
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: contact_import.feature :: Re-import same vCard file does not create duplicates
#[test]
fn partial_import_preserves_localized_duplicate_warning_in_alert() {
    let mut engine = engine("qic", BOB);
    let commands = pick(&mut engine, &format!("{BOB}{CAROL}"));
    assert_eq!(
        messages(&commands),
        vec![(
            "Downloaded result",
            "Downloaded import: 1 — Downloaded skips: 1\n\n• Downloaded duplicate: bob-1"
        )]
    );
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 2);
}

// @scenario: contact_import.feature :: Import contacts from a vCard file
#[test]
fn import_batch_refreshes_the_visible_contact_rows() {
    let mut engine = engine("qid", "");
    let commands = pick(&mut engine, &format!("{BOB}{CAROL}"));
    let names: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(&surface.nodes),
            _ => None,
        })
        .flatten()
        .filter_map(|node| match node {
            PresentationNode::List { id, rows, .. } if id.as_str() == "contacts" => Some(rows),
            _ => None,
        })
        .flatten()
        .map(|row| row.title.as_str())
        .collect();
    assert_eq!(names, vec!["Bob", "Carol"]);
}

// @scenario: contact_import.feature :: Import contacts from a vCard file
#[test]
fn import_uses_updated_content_without_recreating_the_engine() {
    let mut engine = engine("qie", "");
    assert_eq!(
        messages(&pick(&mut engine, BOB)),
        vec![("", "Downloaded import: 1")]
    );
    load_locale_from_bytes(
        "qie",
        br#"{"import_contacts.result_imported":"Updated content: {count}"}"#,
    )
    .unwrap();
    assert_eq!(
        messages(&pick(&mut engine, CAROL)),
        vec![("", "Updated content: 1")]
    );
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 2);
}
