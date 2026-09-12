// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Help batch matches the design canvas: a search field, FAQ
//! HIGHLIGHTS, the remaining questions, and a MORE section with
//! Contact Support, Privacy Policy and About (real Core versions).

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::ui::{
    ActionListItem, ActionResult, AppEngine, AppScreen, Component, RenderContext, ScreenModel,
    UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

fn help_screen() -> (AppEngine, ScreenModel) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Help);
    (engine, screen)
}

fn sections(screen: &ScreenModel) -> Vec<(&str, &str, &[ActionListItem])> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::SectionedActionList { id, sections } if id == "help" => Some(sections),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "help emits one `help` sectioned list: {:?}",
                screen.components
            )
        })
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str(), s.items.as_slice()))
        .collect()
}

fn section<'a>(screen: &'a ScreenModel, id: &str) -> &'a [ActionListItem] {
    sections(screen)
        .into_iter()
        .find(|(sid, _, _)| *sid == id)
        .map(|(_, _, items)| items)
        .unwrap_or_else(|| panic!("no `{id}` section"))
}

fn select(engine: &mut AppEngine, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: "help".into(),
        item_id: item_id.into(),
    })
}

// @scenario: help_faq :: View FAQ categories
#[test]
fn help_screen_is_search_then_highlights_more_questions_and_more() {
    let (_, screen) = help_screen();

    assert!(
        matches!(&screen.components[0], Component::TextInput { id, .. } if id == "help_search"),
        "search field first: {:?}",
        screen.components[0]
    );
    let headers: Vec<(&str, &str)> = sections(&screen)
        .into_iter()
        .map(|(id, label, _)| (id, label))
        .collect();
    assert_eq!(
        headers,
        vec![
            ("highlights", "FAQ HIGHLIGHTS"),
            ("faq", "MORE QUESTIONS"),
            ("more", "MORE"),
        ]
    );
}

// @scenario: help_faq :: View FAQ categories
#[test]
fn faq_highlights_are_the_four_canvas_questions() {
    let (_, screen) = help_screen();

    let rows: Vec<(&str, &str)> = section(&screen, "highlights")
        .iter()
        .map(|item| (item.id.as_str(), item.label.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("exchange-flow", "How does an exchange work?"),
            ("relay-sees", "What can the relay see?"),
            (
                "tag-vs-group",
                "What is the difference between a tag and a group?"
            ),
            ("lost-phone", "I lost my phone. Now what?"),
        ]
    );
    assert!(
        section(&screen, "faq").len() >= 6,
        "the earlier catalog entries stay reachable below the highlights"
    );
}

// @scenario: help_faq :: View a specific FAQ
#[test]
fn selecting_a_highlight_shows_its_answer_inline() {
    let (mut engine, _) = help_screen();

    let result = select(&mut engine, "tag-vs-group");
    let ActionResult::ShowInfoOverlay { title, body } = result else {
        panic!("a highlight answers inline, got {result:?}");
    };
    assert_eq!(title, "What is the difference between a tag and a group?");
    assert!(body.contains("Tags never leave your devices"), "{body}");

    let result = select(&mut engine, "lost-phone");
    let ActionResult::ShowInfoOverlay { body, .. } = result else {
        panic!("a highlight answers inline, got {result:?}");
    };
    assert!(body.contains("Social recovery"), "{body}");
    assert!(body.contains("A backup"), "{body}");
}

// @scenario: help_faq :: View FAQ categories
#[test]
fn more_section_lists_contact_support_privacy_policy_and_about() {
    let (_, screen) = help_screen();
    let app = env!("CARGO_PKG_VERSION");
    let core = vauchi_core::VERSION;

    let rows: Vec<(&str, &str, Option<&str>)> = section(&screen, "more")
        .iter()
        .map(|item| {
            (
                item.id.as_str(),
                item.label.as_str(),
                item.detail.as_deref(),
            )
        })
        .collect();
    let versions = format!("Vauchi {app} · core {core}");
    assert_eq!(
        rows,
        vec![
            (
                "contact-support",
                "Contact Support",
                Some("Opens your mail app; nothing is sent automatically")
            ),
            ("privacy-policy", "Privacy Policy", None),
            ("about", "About", Some(versions.as_str())),
            ("feature-idea", "Suggest an Idea", None),
            ("known-issues", "Known Issues", None),
        ]
    );
}

// @scenario: help_faq :: View a specific FAQ
#[test]
fn contact_support_opens_the_bug_report_mail_draft() {
    let (mut engine, _) = help_screen();

    let result = select(&mut engine, "contact-support");

    let ActionResult::OpenUrl { url } = result else {
        panic!("contact support opens the mail app, got {result:?}");
    };
    assert!(
        url.starts_with("mailto:support@vauchi.app?subject=Bug%20Report"),
        "{url}"
    );
    assert!(
        url.contains("&body="),
        "the draft carries the device info: {url}"
    );
}

// @scenario: help_faq :: View a specific FAQ
#[test]
fn about_shows_the_versions_and_what_vauchi_is() {
    let (mut engine, _) = help_screen();

    let result = select(&mut engine, "about");

    let ActionResult::ShowInfoOverlay { title, body } = result else {
        panic!("about answers inline, got {result:?}");
    };
    assert_eq!(title, "About");
    assert!(body.contains(vauchi_core::VERSION), "{body}");
    assert!(body.contains("privacy-focused contact card app"), "{body}");
}

// @scenario: help_faq :: View a specific FAQ
#[test]
fn privacy_policy_and_known_issues_open_their_pages() {
    let (mut engine, _) = help_screen();

    assert_eq!(
        select(&mut engine, "privacy-policy"),
        ActionResult::OpenUrl {
            url: "https://vauchi.app/docs/legal/privacy-policy".into()
        }
    );
    assert_eq!(
        select(&mut engine, "known-issues"),
        ActionResult::OpenUrl {
            url: "https://vauchi.app/docs/users/known-issues".into()
        }
    );
    let ActionResult::OpenUrl { url } = select(&mut engine, "feature-idea") else {
        panic!("suggest an idea opens the mail app");
    };
    assert!(
        url.starts_with("mailto:support@vauchi.app?subject=Idea"),
        "{url}"
    );
}

// @scenario: help_faq :: Search FAQs by keyword
#[test]
fn search_filters_every_section_and_drops_the_empty_ones() {
    let (mut engine, _) = help_screen();

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "help_search".into(),
        value: "relay".into(),
    });

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("search re-renders in place, got {result:?}");
    };
    let ids: Vec<&str> = sections(&screen).into_iter().map(|(id, _, _)| id).collect();
    assert!(ids.contains(&"highlights"), "{ids:?}");
    assert!(!ids.contains(&"more"), "MORE has no relay row: {ids:?}");
    let highlights: Vec<&str> = section(&screen, "highlights")
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert!(highlights.contains(&"relay-sees"), "{highlights:?}");
    assert!(!highlights.contains(&"tag-vs-group"), "{highlights:?}");
}

// @scenario: help_faq :: FAQs are shown in the user's language
#[test]
fn help_canvas_copy_renders_the_active_locale() {
    load_german();
    let (mut engine, en) = help_screen();
    engine.set_render_context(RenderContext {
        locale: Some("de".into()),
        ..RenderContext::default()
    });
    let de = engine.navigate_to(AppScreen::Help);

    let en_headers: Vec<&str> = sections(&en).into_iter().map(|(_, l, _)| l).collect();
    let de_headers: Vec<&str> = sections(&de).into_iter().map(|(_, l, _)| l).collect();
    assert_translated(
        "help section headers",
        &de_headers.join(" | "),
        &en_headers.join(" | "),
    );
    assert_translated(
        "relay highlight",
        &section(&de, "highlights")[1].label,
        &section(&en, "highlights")[1].label,
    );
    assert_translated(
        "contact support description",
        section(&de, "more")[0].detail.as_deref().unwrap(),
        section(&en, "more")[0].detail.as_deref().unwrap(),
    );
}
