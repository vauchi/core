// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The link code is six digits a person types from what the sender told
//! them, so the code field asks for a number: on the phones test of
//! 2026-11-07 it opened the letter keyboard (vauchi/private#727).

use vauchi_app::ui::{LinkResponderEngine, PreparedSurface, WorkflowEngine};
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};
use vauchi_core::{Command, PresentationInputKind, PresentationNode, SurfaceId};

fn input_kinds(nodes: &[PresentationNode]) -> Vec<(String, PresentationInputKind)> {
    nodes
        .iter()
        .flat_map(|node| match node {
            PresentationNode::Input {
                label, input_kind, ..
            } => vec![(label.clone(), *input_kind)],
            PresentationNode::Group { children, .. } => input_kinds(children),
            _ => Vec::new(),
        })
        .collect()
}

// @internal
#[test]
fn the_link_code_field_asks_for_a_number() {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).expect("canonical URL parses");
    let screen = LinkResponderEngine::new(payload).current_screen();

    let prepared = PreparedSurface::from_screen(
        SurfaceId::new("link_responder").expect("surface id"),
        1,
        &screen,
    )
    .expect("the code screen projects to a generic surface");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("expected ReplaceSurface");
    };

    assert_eq!(
        input_kinds(&surface.nodes),
        vec![("6-digit code".to_string(), PresentationInputKind::Number)]
    );
}
