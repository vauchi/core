// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content that arrives while someone is onboarding must not throw them
//! back to the start. A locale catalogue downloaded after start rebuilt
//! the onboarding engine, which begins again at Welcome: on a Pixel 3a the
//! name page went back to Welcome and the typed name was lost
//! (vauchi/private#1339), and on an iPhone in French the first run went
//! back to the name step when French arrived (vauchi/private#821).

use vauchi_app::ui::{AppEngine, RenderContext};
use vauchi_core::api::Vauchi;
use vauchi_core::{BindingId, Command, Event, InputValue, PresentationNode, SurfaceId};

use super::common::app_engine_helpers::present_the_name_page;
use super::i18n_support::load_locale;

struct NamePage {
    title: String,
    field: Option<(BindingId, String)>,
}

fn shown(commands: &[Command]) -> NamePage {
    commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(NamePage {
                title: surface.title.clone(),
                field: input(&surface.nodes),
            }),
            _ => None,
        })
        .expect("the batch replaces the surface")
}

fn input(nodes: &[PresentationNode]) -> Option<(BindingId, String)> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Input {
            binding_id, value, ..
        } => Some((binding_id.clone(), value.clone())),
        PresentationNode::Group { children, .. } => input(children),
        _ => None,
    })
}

/// Welcome, "Create new identity", then "Carol" typed into the name field.
fn carol_typed_on_the_name_page() -> (AppEngine, NamePage) {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let name_page = shown(&present_the_name_page(&mut engine));
    let (binding_id, _) = name_page
        .field
        .clone()
        .expect("the name page has its field");
    engine
        .dispatch(Event::ValueChanged {
            surface_id: SurfaceId::new("onboarding").unwrap(),
            binding_id,
            value: InputValue::Text("Carol".to_owned()),
        })
        .expect("type the name");
    (engine, name_page)
}

// @internal
#[test]
fn locales_reloaded_on_the_name_page_keep_the_page_and_the_typed_name() {
    let (mut engine, name_page) = carol_typed_on_the_name_page();

    engine.reload_locales();
    let after = shown(
        &engine
            .dispatch(Event::PresentationInvalidated)
            .expect("invalidation"),
    );

    assert_eq!(after.title, name_page.title);
    assert_eq!(
        after.field.map(|(_, value)| value),
        Some("Carol".to_owned())
    );
}

// @internal
#[test]
fn a_new_language_on_the_name_page_keeps_the_typed_name_and_shows_the_page_in_it() {
    load_locale("fr");
    let (mut engine, name_page) = carol_typed_on_the_name_page();

    engine.set_render_context(RenderContext {
        locale: Some("fr".to_owned()),
        ..RenderContext::default()
    });
    let after = shown(&engine.initial_commands().expect("re-present"));

    assert_ne!(after.title, name_page.title, "the page is shown in French");
    assert_eq!(
        after.field.map(|(_, value)| value),
        Some("Carol".to_owned())
    );
}
