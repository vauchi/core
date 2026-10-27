// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use vauchi_core::{EventJsonError, FilePickFailure, FilePickOutcome};

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn canonical_picker_fixture_preserves_commands_and_bounded_events() {
    let fixture: Value =
        serde_json::from_str(vauchi_app::ui::presentation_contract_fixture_json()).unwrap();
    let contract = &fixture["file_picker_contract"];
    let commands: Vec<Command> = serde_json::from_value(contract["commands"].clone()).unwrap();
    assert_eq!(commands.len(), 2);
    assert_eq!(
        serde_json::to_value(commands).unwrap(),
        contract["commands"]
    );
    let events = contract["events"].as_array().unwrap();
    assert_eq!(events.len(), 7);
    for wire in events {
        let event = vauchi_core::platform::event_from_json(&wire.to_string()).unwrap();
        assert_eq!(serde_json::to_value(event).unwrap(), *wire);
    }
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn picker_decoder_rejects_unbounded_identifiers_filenames_and_unknown_failures() {
    for (id, outcome, error) in [
        (String::new(), json!("cancelled"), EventJsonError::Malformed),
        (
            "x".repeat(129),
            json!("cancelled"),
            EventJsonError::Malformed,
        ),
        (
            "opaque".into(),
            json!({"selected": {"bytes": [], "filename": "x".repeat(1025)}}),
            EventJsonError::TooLarge,
        ),
        (
            "opaque".into(),
            json!({"failed": {"code": "native-error-string"}}),
            EventJsonError::Malformed,
        ),
    ] {
        let wire = json!({"FilePickCompleted": {"request_id": id, "outcome": outcome}});
        assert_eq!(
            vauchi_core::platform::event_from_json(&wire.to_string()),
            Err(error)
        );
    }
}

fn enable(engine: &mut AppEngine) {
    engine
        .dispatch(Event::FilePickerCapabilitiesChanged {
            correlated_results: true,
        })
        .unwrap();
    availability(engine, true);
}

fn request(engine: &mut AppEngine) -> String {
    let shown = menu(&engine.initial_commands().unwrap());
    engine
        .dispatch(activation(&shown))
        .unwrap()
        .into_iter()
        .find_map(|command| match command {
            Command::PickFile {
                request_id,
                title,
                max_bytes,
                ..
            } => {
                assert!(!title.is_empty());
                assert_eq!(max_bytes, vauchi_core::MAX_FILE_PICK_BYTES as u64);
                Some(request_id)
            }
            _ => None,
        })
        .expect("a correlated bounded picker")
}

fn selected() -> FilePickOutcome {
    FilePickOutcome::Selected {
        bytes: VCARD.to_vec(),
        filename: "chosen.vcf".into(),
    }
}

fn completion(request_id: &str, outcome: FilePickOutcome) -> Event {
    Event::FilePickCompleted {
        request_id: request_id.into(),
        outcome,
    }
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn cancelled_import_cannot_complete_a_later_request() {
    let mut engine = engine();
    enable(&mut engine);
    let first = request(&mut engine);
    engine
        .dispatch(completion(&first, FilePickOutcome::Cancelled))
        .unwrap();
    let second = request(&mut engine);
    assert_ne!(first, second);
    for outcome in [
        selected(),
        FilePickOutcome::Cancelled,
        FilePickOutcome::Failed {
            code: FilePickFailure::ReadFailed,
        },
    ] {
        assert!(
            engine
                .dispatch(completion(&first, outcome))
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        engine
            .dispatch(Event::FilePickedFromUser {
                bytes: VCARD.to_vec(),
                filename: "uncorrelated.vcf".into(),
            })
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
    assert_eq!(
        menu(&engine.initial_commands().unwrap())["menus"][0]["items"][0]["enabled"],
        false
    );
    engine.dispatch(completion(&second, selected())).unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 1);
    assert!(
        engine
            .dispatch(completion(&second, selected()))
            .unwrap()
            .is_empty()
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn withdrawing_file_capability_cancels_native_work_and_rejects_its_result() {
    let mut engine = engine();
    enable(&mut engine);
    let request_id = request(&mut engine);
    let commands = engine
        .dispatch(Event::FilePickerCapabilitiesChanged {
            correlated_results: false,
        })
        .unwrap();
    assert_eq!(
        commands,
        vec![Command::CancelFilePick {
            request_id: request_id.clone()
        }]
    );
    assert!(
        engine
            .dispatch(completion(&request_id, selected()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn locking_cancels_the_native_request_and_unlock_does_not_revive_it() {
    let mut engine = engine();
    engine.set_device_capabilities(
        vauchi_core::exchange::capability::types::DeviceCapabilities {
            has_biometrics: true,
            ..Default::default()
        },
    );
    engine
        .vauchi_mut()
        .setup_app_password("app-password-123")
        .unwrap();
    enable(&mut engine);
    let first = request(&mut engine);
    engine.navigate_to(AppScreen::Lock);
    let commands = engine.initial_commands().unwrap();
    assert!(commands.contains(&Command::CancelFilePick {
        request_id: first.clone()
    }));
    engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();
    engine.navigate_to(AppScreen::Settings);
    let second = request(&mut engine);
    assert!(
        engine
            .dispatch(completion(&first, selected()))
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
    engine.dispatch(completion(&second, selected())).unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 1);
}

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn typed_failure_uses_runtime_copy_and_allows_another_request() {
    let mut engine = engine();
    enable(&mut engine);
    engine.set_render_context(RenderContext {
        locale: Some("qjc".into()),
        ..Default::default()
    });
    for text in ["Downloaded failure", "Updated downloaded failure"] {
        vauchi_app::i18n::load_locale_from_bytes(
            "qjc",
            serde_json::to_vec(&json!({
                "platform.error_could_not_read_file": text
            }))
            .unwrap()
            .as_slice(),
        )
        .unwrap();
        let id = request(&mut engine);
        let commands = engine
            .dispatch(completion(
                &id,
                FilePickOutcome::Failed {
                    code: FilePickFailure::TooLarge,
                },
            ))
            .unwrap();
        assert!(serde_json::to_string(&commands).unwrap().contains(text));
        assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
    }
}
