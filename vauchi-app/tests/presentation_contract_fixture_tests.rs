// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use vauchi_app::ui::AppEngine;
use vauchi_core::{
    BindingId, Command, ContextBar, Event, InputValue, PresentationNode, SurfaceId, SurfaceSpec,
    Vauchi,
};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct PresentationContractFixture {
    schema_version: u32,
    initial_commands: Vec<Command>,
    steps: Vec<PresentationContractStep>,
    expected_state: ExpectedPresentationState,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct PresentationContractStep {
    event: Event,
    commands: Vec<Command>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct ExpectedPresentationState {
    active_surface_id: String,
    surface: SurfaceSpec,
    context_bar: ContextBar,
}

fn init_fixture_i18n() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let locales_dir = manifest_dir
        .ancestors()
        .map(|ancestor| ancestor.join("locales"))
        .find(|candidate| candidate.join("en.json").is_file())
        .expect("fixture tests require the workspace locales repository");
    vauchi_app::i18n::init(&locales_dir).expect("load production locale strings");
}

fn replacement(commands: &[Command]) -> &SurfaceSpec {
    commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
        .expect("fixture batch must replace a surface")
}

fn primary_activation(commands: &[Command]) -> (SurfaceId, vauchi_core::InteractionId) {
    commands
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar
                .primary
                .as_ref()
                .map(|action| (surface_id.clone(), action.interaction_id.clone())),
            _ => None,
        })
        .expect("fixture bootstrap must expose a primary action")
}

fn first_input_binding(nodes: &[PresentationNode]) -> Option<BindingId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Input { binding_id, .. } => Some(binding_id.clone()),
        PresentationNode::Group { children, .. } => first_input_binding(children),
        _ => None,
    })
}

fn context_bar(commands: &[Command]) -> &ContextBar {
    commands
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar { bar, .. } => Some(bar.as_ref()),
            _ => None,
        })
        .expect("fixture batch must install a context bar")
}

fn current_fixture() -> PresentationContractFixture {
    init_fixture_i18n();
    let mut engine = AppEngine::new(Vauchi::in_memory().expect("in-memory Core"));
    let initial_commands = engine.initial_commands().expect("initial command batch");

    let (surface_id, interaction_id) = primary_activation(&initial_commands);
    let activate_primary = Event::ActionActivated {
        surface_id,
        interaction_id,
    };
    let first_commands = engine
        .dispatch(activate_primary.clone())
        .expect("activate primary action");

    let first_surface = replacement(&first_commands);
    let binding_id = first_input_binding(&first_surface.nodes).expect("prepared text input");
    let change_value = Event::ValueChanged {
        surface_id: first_surface.surface_id.clone(),
        binding_id,
        value: InputValue::Text("Ada".to_owned()),
    };
    let second_commands = engine
        .dispatch(change_value.clone())
        .expect("change prepared input value");
    let final_surface = replacement(&second_commands).clone();
    let final_context_bar = context_bar(&second_commands).clone();

    PresentationContractFixture {
        schema_version: 1,
        initial_commands,
        steps: vec![
            PresentationContractStep {
                event: activate_primary,
                commands: first_commands,
            },
            PresentationContractStep {
                event: change_value,
                commands: second_commands,
            },
        ],
        expected_state: ExpectedPresentationState {
            active_surface_id: final_surface.surface_id.as_str().to_owned(),
            surface: final_surface,
            context_bar: final_context_bar,
        },
    }
}

#[test]
fn shared_fixture_replays_the_exact_core_event_command_sequence() {
    init_fixture_i18n();
    let fixture: PresentationContractFixture =
        serde_json::from_str(vauchi_app::ui::presentation_contract_fixture_json())
            .expect("Core-owned presentation contract fixture must decode");
    let mut engine = AppEngine::new(Vauchi::in_memory().expect("in-memory Core"));

    assert_eq!(fixture.schema_version, 1);
    assert_eq!(
        engine.initial_commands().expect("initial command batch"),
        fixture.initial_commands
    );
    for step in fixture.steps {
        assert_eq!(
            engine.dispatch(step.event).expect("fixture event dispatch"),
            step.commands
        );
    }
}

#[test]
fn shared_fixture_pins_equal_revision_as_an_atomic_replacement() {
    let fixture: PresentationContractFixture =
        serde_json::from_str(vauchi_app::ui::presentation_contract_fixture_json())
            .expect("Core-owned presentation contract fixture must decode");
    assert_eq!(
        fixture.steps.len(),
        2,
        "fixture must contain both transitions"
    );

    let before = replacement(&fixture.steps[0].commands);
    let after = replacement(&fixture.steps[1].commands);
    assert_eq!(before.surface_id, after.surface_id);
    assert_eq!(before.revision, after.revision);
    assert_ne!(
        before, after,
        "equal revision must carry changed prepared state"
    );
    assert_eq!(
        fixture.expected_state.active_surface_id,
        after.surface_id.as_str()
    );
    assert_eq!(&fixture.expected_state.surface, after);
    assert!(
        fixture
            .expected_state
            .context_bar
            .primary
            .as_ref()
            .is_some_and(|action| action.enabled),
        "the second batch must atomically install its matching enabled context bar"
    );
}

#[test]
fn shared_fixture_is_fresh() {
    let recorded: PresentationContractFixture =
        serde_json::from_str(vauchi_app::ui::presentation_contract_fixture_json())
            .expect("Core-owned presentation contract fixture must decode");

    assert_eq!(recorded, current_fixture());
}

#[test]
#[ignore = "regenerates the checked-in Core contract fixture"]
fn regenerate_shared_fixture() {
    let json = serde_json::to_string_pretty(&current_fixture()).expect("serialize fixture");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/presentation_contract_v1.json");
    std::fs::write(path, format!("{json}\n")).expect("write presentation fixture");
}
