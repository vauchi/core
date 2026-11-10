// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Who sees a card entry is a state, not a third line of text. My Card's
//! entry rows carry that audience as the row's one `Status` control — the
//! row primitive that has a tone — so every shell can draw it as a chip
//! instead of muted sub-text under the value (vauchi/private#1206, UI/UX
//! clarity addendum, slice 1).

use vauchi_app::ui::{AppEngine, ScreenCatalogFixture, screen_catalog_fixture_json};
use vauchi_core::{
    Command, Event, InputMode, MotionPreference, PresentationNode, PresentationRow,
    PresentationTone,
    api::Vauchi,
    contact_card::{ContactField, FieldType},
};

const MY_CARD: &str = "my_info";

fn collect_entry_rows(nodes: &[PresentationNode], out: &mut Vec<PresentationRow>) {
    for node in nodes {
        match node {
            // Only list items carry initials; action rows and tabs do not.
            PresentationNode::List { rows, .. } => {
                out.extend(
                    rows.iter()
                        .filter(|row| row.fallback_text.is_some())
                        .cloned(),
                );
            }
            PresentationNode::Group { children, .. } => collect_entry_rows(children, out),
            _ => {}
        }
    }
}

fn my_card_entry_rows(commands: &[Command]) -> Vec<PresentationRow> {
    let surface = commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } if surface.surface_id.as_str() == MY_CARD => {
                Some(surface)
            }
            _ => None,
        })
        .expect("My Card surface");
    let mut rows = Vec::new();
    collect_entry_rows(&surface.nodes, &mut rows);
    rows
}

fn audience(row: &PresentationRow) -> (&str, PresentationTone) {
    assert_eq!(
        row.controls.len(),
        1,
        "{:?} carries exactly one control, its audience",
        row.title
    );
    match &row.controls[0] {
        PresentationNode::Status {
            title,
            detail,
            badge,
            tone,
            activation,
            accessibility,
            ..
        } => {
            assert_eq!(detail.as_deref(), None);
            assert_eq!(badge.as_deref(), None);
            assert!(
                activation.is_none(),
                "the audience is a state, not a button"
            );
            assert_eq!(accessibility.label, *title);
            (title.as_str(), *tone)
        }
        other => panic!(
            "{:?}'s audience is a Status control, got {other:?}",
            row.title
        ),
    }
}

fn catalog_entry(code_id: &str) -> Vec<Command> {
    let catalog: ScreenCatalogFixture =
        serde_json::from_str(screen_catalog_fixture_json()).expect("checked-in catalog parses");
    catalog
        .screens
        .into_iter()
        .find(|screen| screen.code_id == code_id)
        .unwrap_or_else(|| panic!("catalog has {code_id}"))
        .commands
}

// @scenario: generic_presentation_protocol :: Every shell renders the same prepared presentation
// @internal
#[test]
fn a_new_entry_row_carries_its_default_audience_as_a_status_control_not_as_detail() {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Core");
    vauchi.create_identity("Ada").expect("identity");
    let mut card = vauchi.own_card().expect("own card").expect("identity card");
    card.add_field(ContactField::new(
        FieldType::Email,
        "Email",
        "probe@example.invalid",
        0,
    ))
    .expect("first field");
    vauchi.update_own_card(&card).expect("save card");
    let mut app = AppEngine::new(vauchi);
    let mut commands = app.initial_commands().expect("initial presentation");
    commands.extend(
        app.dispatch(Event::PresentationEnvironmentChanged {
            available_width: 960,
            available_height: 720,
            input_modes: vec![InputMode::Pointer, InputMode::Keyboard],
            motion: MotionPreference::Full,
            color_scheme: None,
        })
        .expect("presentation environment"),
    );

    let rows = my_card_entry_rows(&commands);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row.title, "Email");
    assert_eq!(row.subtitle.as_deref(), Some("probe@example.invalid"));
    assert_eq!(
        row.detail, None,
        "the audience leaves the muted detail line once it is a control"
    );
    assert_eq!(
        audience(row),
        ("Hidden by default", PresentationTone::Neutral),
        "a new entry is hidden until the person grants visibility (ADR-054)"
    );
}

// @scenario: generic_presentation_protocol :: Every shell renders the same prepared presentation
// @internal
#[test]
fn the_catalog_my_card_rows_each_carry_one_audience_status_and_no_detail() {
    let rows = my_card_entry_rows(&catalog_entry(MY_CARD));
    assert_eq!(rows.len(), 5, "{rows:?}");
    for row in &rows {
        assert_eq!(
            row.detail, None,
            "{:?} keeps no muted audience line",
            row.title
        );
    }
    let audiences: Vec<(&str, PresentationTone)> = rows.iter().map(audience).collect();
    assert_eq!(
        audiences,
        vec![
            ("All contacts by default", PresentationTone::Neutral),
            ("All contacts by default", PresentationTone::Neutral),
            ("All contacts by default", PresentationTone::Neutral),
            ("Family", PresentationTone::Neutral),
            ("Cycling club, Family", PresentationTone::Neutral),
        ]
    );
}

// @internal
#[test]
fn the_catalog_empty_my_card_has_no_entry_rows_and_so_no_audience() {
    let rows = my_card_entry_rows(&catalog_entry("my_info-empty"));
    assert_eq!(rows, Vec::new());
}
