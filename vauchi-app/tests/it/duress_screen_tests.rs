// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Duress PIN screen (#459, design pass #419 item 8; owner decisions
//! 2026-10-01): once set up it is a management screen — rows for the PIN,
//! the decoy contacts and the alerts, each opening what it names; How
//! duress mode looks; Turn off. Setup says that Emergency Shred forced in
//! duress mode erases the real data (#462).

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, DuressConfig, DuressPinEngine, Item,
    ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::ImportSource;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;

const SHRED_NOTE: &str = "If someone forces you to use Emergency Shred while Vauchi is in \
duress mode, it erases your real data, not only the decoys.";

fn person(id: &str) -> Item {
    Item {
        id: id.into(),
        name: id.into(),
        initials: id.chars().take(1).collect(),
        ..Default::default()
    }
}

fn engine(enabled: bool, recipients: usize, decoys: usize) -> DuressPinEngine {
    let pool: Vec<Item> = (0..3).map(|i| person(&format!("c{i}"))).collect();
    DuressPinEngine::new(
        DuressConfig {
            enabled,
            selected_contact_ids: pool.iter().take(recipients).map(|c| c.id.clone()).collect(),
            available_contacts: pool,
            alert_message: "help".into(),
            include_location: false,
        },
        Locale::English,
    )
    .with_decoy_count(decoys)
}

fn rows(screen: &ScreenModel) -> Vec<(String, Option<String>)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == "duress_rows" => Some(
                items
                    .iter()
                    .map(|i| (i.label.clone(), i.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn buttons(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ButtonList { id, items } if id == "duress_actions" => {
                Some(items.iter().map(|i| i.label.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn select(engine: &mut impl WorkflowEngine, list: &str, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: list.into(),
        item_id: id.into(),
    })
}

fn has_pin_input(screen: &ScreenModel) -> bool {
    screen
        .components
        .iter()
        .any(|c| matches!(c, Component::PinInput { .. }))
}

// @internal
#[test]
fn a_set_up_duress_pin_is_a_management_screen() {
    let screen = engine(true, 2, 4).current_screen();
    assert_eq!(
        rows(&screen),
        [
            ("Duress PIN".into(), Some("Set".into())),
            (
                "Decoy contacts".into(),
                Some("4 shown in duress mode".into())
            ),
            ("Duress alerts".into(), Some("Sent to 2 contacts".into())),
        ]
    );
    assert_eq!(
        buttons(&screen),
        ["How duress mode looks", "Turn off duress PIN"]
    );
    assert!(screen.contextual_actions.is_empty());
    assert!(screen.progress.is_none(), "no step bar on the overview");
}

// @internal
#[test]
fn counts_read_naturally() {
    let one = engine(true, 1, 1).current_screen();
    assert_eq!(rows(&one)[1].1.as_deref(), Some("1 shown in duress mode"));
    assert_eq!(rows(&one)[2].1.as_deref(), Some("Sent to 1 contact"));
    let none = engine(true, 0, 0).current_screen();
    assert_eq!(rows(&none)[1].1.as_deref(), Some("None yet"));
    assert_eq!(
        rows(&none)[2].1.as_deref(),
        Some("Off · no contacts chosen")
    );
}

// @internal
#[test]
fn not_set_up_offers_set_up_only() {
    let screen = engine(false, 0, 0).current_screen();
    assert!(rows(&screen).is_empty());
    assert_eq!(buttons(&screen), ["Set Up PIN"]);
}

// @internal
#[test]
fn setup_says_what_emergency_shred_does_in_duress_mode() {
    let mut fresh = engine(false, 0, 0);
    let _ = select(&mut fresh, "duress_actions", "set_up");
    let screen = fresh.current_screen();
    assert!(has_pin_input(&screen));
    assert!(texts(&screen).contains(&SHRED_NOTE.to_string()));

    let mut set_up = engine(true, 2, 4);
    let _ = select(&mut set_up, "duress_rows", "pin");
    assert!(
        has_pin_input(&set_up.current_screen()),
        "the PIN row changes the PIN"
    );
}

// @internal
#[test]
fn the_alerts_row_edits_alerts_only_and_back_returns_to_the_overview() {
    let mut engine = engine(true, 2, 4);
    let _ = select(&mut engine, "duress_rows", "alerts");
    let alerts = engine.current_screen();
    assert!(!has_pin_input(&alerts), "no PIN asked to edit alerts");
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    assert_eq!(
        rows(&engine.current_screen()).len(),
        3,
        "back on the overview"
    );
}

// @internal
#[test]
fn how_duress_mode_looks_explains_the_concealment() {
    let mut engine = engine(true, 2, 4);
    match select(&mut engine, "duress_actions", "how_it_looks") {
        ActionResult::ShowInfoOverlay { title, body } => {
            assert_eq!(title, "How duress mode looks");
            assert!(
                body.contains("look as if no duress PIN was ever set up"),
                "{body}"
            );
        }
        other => panic!("expected the overlay, got {other:?}"),
    }
}

// @internal
#[test]
fn turn_off_asks_first() {
    let mut engine = engine(true, 2, 4);
    let _ = select(&mut engine, "duress_actions", "turn_off");
    assert!(
        engine
            .current_screen()
            .components
            .iter()
            .any(|c| matches!(c, Component::InlineConfirm { id, .. } if id == "disable"))
    );
    assert_eq!(
        engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm_disable".into(),
        }),
        ActionResult::Complete
    );
}

const PIN: &str = "135790";

fn ready_with_contact() -> (AppEngine, String) {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine
        .vauchi_mut()
        .setup_app_password("app-password-123")
        .unwrap();
    let contact = Contact::from_import(
        "contact-ally".into(),
        ContactCard::new("Ally"),
        ImportSource::VcardFile,
        None,
        0,
    );
    engine.vauchi().add_contact(contact).unwrap();
    let ally = engine
        .vauchi()
        .list_contacts()
        .unwrap()
        .first()
        .map(|c| c.id().to_string())
        .unwrap();
    (engine, ally)
}

// @internal
#[test]
fn a_pin_without_alerts_reads_as_set_up() {
    let (mut engine, _) = ready_with_contact();
    engine.vauchi_mut().setup_duress_password(PIN).unwrap();
    let screen = engine.navigate_to(AppScreen::DuressPin);
    assert_eq!(
        rows(&screen),
        [
            ("Duress PIN".into(), Some("Set".into())),
            ("Decoy contacts".into(), Some("None yet".into())),
            (
                "Duress alerts".into(),
                Some("Off · no contacts chosen".into())
            ),
        ]
    );
}

// @internal
#[test]
fn editing_alerts_keeps_the_pin() {
    let (mut engine, ally) = ready_with_contact();
    engine.vauchi_mut().setup_duress_password(PIN).unwrap();
    let _ = engine.navigate_to(AppScreen::DuressPin);
    let _ = select(&mut engine, "duress_rows", "alerts");
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "recipients".into(),
        item_id: ally.clone(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    let saved = engine.vauchi().load_duress_settings().unwrap().unwrap();
    assert_eq!(saved.alert_contact_ids, [ally]);
    assert_eq!(
        engine.vauchi_mut().authenticate(PIN).unwrap(),
        vauchi_core::AuthMode::Duress,
        "the duress PIN is unchanged"
    );
}

// @internal
#[test]
fn the_decoy_row_opens_decoy_contacts() {
    let (mut engine, _) = ready_with_contact();
    engine.vauchi_mut().setup_duress_password(PIN).unwrap();
    let _ = engine.navigate_to(AppScreen::DuressPin);
    let result = select(&mut engine, "duress_rows", "decoys");
    let ActionResult::NavigateTo(screen) = result else {
        panic!("the decoy row navigates, got {result:?}");
    };
    assert_eq!(screen.screen_id, "decoy_contacts");
}
