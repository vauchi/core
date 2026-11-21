// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A display that answers file picks is asked for the avatar image the same
//! way: one pick under Core's title, whose answer opens the editor. A
//! terminal's picker is a path line and needs that title
//! (vauchi/private#1233).

use crate::event_path_support::{activate, row, surface_titles};
use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event, FilePickFailure, FilePickOutcome, MAX_FILE_PICK_BYTES};

fn avatar_editor(answers_picks: bool) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine
        .dispatch(Event::FilePickerCapabilitiesChanged {
            correlated_results: answers_picks,
        })
        .unwrap();
    engine.navigate_to(AppScreen::AvatarEditor);
    engine
}

fn choose_photos(engine: &mut AppEngine) -> Vec<Command> {
    let shown = engine.initial_commands().unwrap();
    let (surface_id, photos) = row(&shown, "Photos");
    activate(engine, surface_id, &photos)
}

fn asked_pick(commands: &[Command]) -> String {
    let picks: Vec<&Command> = commands
        .iter()
        .filter(|command| {
            matches!(
                command,
                Command::PickFile { .. }
                    | Command::ImagePickFromFile
                    | Command::ImagePickFromLibrary
            )
        })
        .collect();
    let [
        Command::PickFile {
            request_id,
            title,
            accepted_mime_types,
            accepted_extensions,
            max_bytes,
        },
    ] = picks.as_slice()
    else {
        panic!("one titled pick and no other image request, got {picks:#?}");
    };
    assert_eq!(title, "Choose Avatar");
    assert_eq!(
        accepted_mime_types,
        &["image/png", "image/jpeg", "image/webp", "image/bmp"]
    );
    assert_eq!(accepted_extensions, &["png", "jpg", "jpeg", "webp", "bmp"]);
    assert_eq!(*max_bytes, MAX_FILE_PICK_BYTES as u64);
    request_id.clone()
}

fn answer(engine: &mut AppEngine, request_id: &str, outcome: FilePickOutcome) -> Vec<Command> {
    engine
        .dispatch(Event::FilePickCompleted {
            request_id: request_id.into(),
            outcome,
        })
        .unwrap()
}

fn shows_the_picked_image(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .components
        .iter()
        .any(|component| matches!(component, Component::ImageCircle { .. }))
}

fn alert_titles(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::PresentAlert { alert } => Some(alert.title.clone()),
            _ => None,
        })
        .collect()
}

// @internal
#[test]
fn the_picked_image_opens_the_editor() {
    let mut engine = avatar_editor(true);
    let request_id = asked_pick(&choose_photos(&mut engine));
    assert!(!shows_the_picked_image(&engine));

    answer(
        &mut engine,
        &request_id,
        FilePickOutcome::Selected {
            bytes: vauchi_core::avatar::generate_initials_avatar([255, 0, 0], 32),
            filename: "me.png".into(),
        },
    );

    assert!(shows_the_picked_image(&engine));
}

// @internal — the content is checked by Core, whatever the file is named.
#[test]
fn a_picked_file_that_is_no_image_is_refused_with_cores_alert() {
    let mut engine = avatar_editor(true);
    let request_id = asked_pick(&choose_photos(&mut engine));

    let commands = answer(
        &mut engine,
        &request_id,
        FilePickOutcome::Selected {
            bytes: b"BEGIN:VCARD".to_vec(),
            filename: "me.png".into(),
        },
    );

    assert_eq!(alert_titles(&commands), ["Invalid Image"]);
    assert!(!shows_the_picked_image(&engine));
}

// @internal
#[test]
fn a_cancelled_or_failed_pick_leaves_the_sources_and_another_can_be_asked() {
    for outcome in [
        FilePickOutcome::Cancelled,
        FilePickOutcome::Failed {
            code: FilePickFailure::TooLarge,
        },
    ] {
        let mut engine = avatar_editor(true);
        let first = asked_pick(&choose_photos(&mut engine));

        answer(&mut engine, &first, outcome);

        assert!(!shows_the_picked_image(&engine));
        let second = asked_pick(&choose_photos(&mut engine));
        assert_ne!(first, second);
    }
}

// @internal — an answer to a pick Core no longer waits for changes nothing.
#[test]
fn an_answer_after_leaving_the_editor_is_dropped() {
    let mut engine = avatar_editor(true);
    let request_id = asked_pick(&choose_photos(&mut engine));
    engine.navigate_to(AppScreen::Settings);
    let left = engine.initial_commands().unwrap();
    assert!(left.contains(&Command::CancelFilePick {
        request_id: request_id.clone()
    }));

    let commands = answer(
        &mut engine,
        &request_id,
        FilePickOutcome::Selected {
            bytes: vauchi_core::avatar::generate_initials_avatar([255, 0, 0], 32),
            filename: "me.png".into(),
        },
    );

    assert_eq!(surface_titles(&commands), Vec::<(String, String)>::new());
    assert_eq!(engine.vauchi().own_card().unwrap().unwrap().avatar(), None);
}

// @internal — shells that do not answer picks keep the two image requests.
#[test]
fn a_display_that_does_not_answer_picks_gets_the_image_requests() {
    let mut engine = avatar_editor(false);

    let commands = choose_photos(&mut engine);

    let image_requests: Vec<&Command> = commands
        .iter()
        .filter(|command| {
            matches!(
                command,
                Command::PickFile { .. }
                    | Command::ImagePickFromFile
                    | Command::ImagePickFromLibrary
            )
        })
        .collect();
    assert_eq!(
        image_requests,
        [&Command::ImagePickFromLibrary, &Command::ImagePickFromFile]
    );
}
