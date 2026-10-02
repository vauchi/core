// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The exchange screen draws its own code where the link trainer says: a
//! `placement` on the display QR node, in permille of the node's square
//! (#450, design D7).

#![cfg(feature = "testing")]

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::{ProtocolState, QrPayload};
use vauchi_core::platform::{
    Command, PresentationNode, PresentationQrErrorCorrection, PresentationQrPurpose, QrPlacement,
    SurfaceSpec,
};

fn engine_on_hover() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    engine
}

fn presented_surface(engine: &mut AppEngine) -> SurfaceSpec {
    engine
        .initial_commands()
        .expect("commands")
        .into_iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
        .expect("a surface is presented")
}

/// Placement of the display QR node, wherever it sits in the surface.
fn own_code_placement(surface: &SurfaceSpec) -> Option<Option<QrPlacement>> {
    fn walk(nodes: &[PresentationNode]) -> Option<Option<QrPlacement>> {
        nodes.iter().find_map(|node| match node {
            PresentationNode::Qr {
                purpose: PresentationQrPurpose::Display,
                placement,
                ..
            } => Some(*placement),
            PresentationNode::Group { children, .. } => walk(children),
            _ => None,
        })
    }
    walk(&surface.nodes)
}

fn frame_at_layout(layout: u8) -> QrPayload {
    QrPayload {
        data: "FRAME".into(),
        error_correction: "M".into(),
        display_duration_ms: 300,
        layout,
    }
}

fn placement(size: u16, x: u16, y: u16) -> QrPlacement {
    QrPlacement::new(size, x, y).expect("placement inside the square")
}

// @internal
#[test]
fn a_placement_outside_the_square_cannot_be_built() {
    assert_eq!(placement(1000, 0, 0).size(), 1000);
    assert_eq!(placement(650, 350, 175).x(), 350);
    assert_eq!(placement(650, 350, 175).y(), 175);
    assert_eq!(placement(500, 500, 500).size(), 500);

    let refused = [
        ("smaller than half the square", (499, 0, 0)),
        ("larger than the square", (1001, 0, 0)),
        ("past the right edge", (800, 201, 0)),
        ("past the bottom edge", (800, 0, 201)),
        ("far outside", (650, u16::MAX, 0)),
        ("size overflowing", (u16::MAX, 0, 0)),
    ];
    for (case, (size, x, y)) in refused {
        assert_eq!(QrPlacement::new(size, x, y), None, "{case}");
    }
}

// @internal
#[test]
fn a_placement_outside_the_square_is_refused_from_json() {
    let inside: QrPlacement =
        serde_json::from_str(r#"{"size":800,"x":200,"y":0}"#).expect("valid placement");
    assert_eq!(inside, placement(800, 200, 0));

    for outside in [
        r#"{"size":800,"x":201,"y":0}"#,
        r#"{"size":400,"x":0,"y":0}"#,
        r#"{"size":800,"x":0}"#,
        r#"{"size":-1,"x":0,"y":0}"#,
        r#"{"size":800,"x":0,"y":0,"z":1}"#,
    ] {
        assert!(
            serde_json::from_str::<QrPlacement>(outside).is_err(),
            "{outside}"
        );
    }
}

// @internal
#[test]
fn a_full_size_code_carries_no_placement_on_the_wire() {
    let mut engine = engine_on_hover();
    assert!(engine.apply_multi_stage_qr_payload(&frame_at_layout(0)));

    let surface = presented_surface(&mut engine);

    assert_eq!(own_code_placement(&surface), Some(None));
    let json = serde_json::to_string(&surface).expect("surface serialises");
    assert!(!json.contains("placement"), "{json}");
}

// @internal
#[test]
fn the_exchange_screen_draws_its_code_at_the_trainers_layout() {
    // Layout 0 is full size; 1–4 are 80 % in the four corners; 5–13 are
    // 65 % on a 3 × 3 grid, row by row. Every placed code keeps 50 permille
    // clear of the square's edges, so it has a light border whatever colour
    // the screen is around the square.
    let expected = [
        (1, placement(800, 50, 50)),
        (2, placement(800, 150, 50)),
        (3, placement(800, 50, 150)),
        (4, placement(800, 150, 150)),
        (5, placement(650, 50, 50)),
        (6, placement(650, 175, 50)),
        (7, placement(650, 300, 50)),
        (8, placement(650, 50, 175)),
        (9, placement(650, 175, 175)),
        (10, placement(650, 300, 175)),
        (11, placement(650, 50, 300)),
        (12, placement(650, 175, 300)),
        (13, placement(650, 300, 300)),
    ];
    let mut engine = engine_on_hover();

    for (layout, at) in expected {
        assert!(engine.apply_multi_stage_qr_payload(&frame_at_layout(layout)));
        let surface = presented_surface(&mut engine);
        assert_eq!(
            own_code_placement(&surface),
            Some(Some(at)),
            "layout {layout}"
        );
    }
}

// @internal
#[test]
fn a_layout_outside_the_set_is_drawn_full_size() {
    let mut engine = engine_on_hover();
    assert!(engine.apply_multi_stage_qr_payload(&frame_at_layout(200)));

    let surface = presented_surface(&mut engine);

    assert_eq!(own_code_placement(&surface), Some(None));
}

// @internal
#[test]
fn the_saved_screen_keeps_the_code_where_it_was() {
    let mut engine = engine_on_hover();
    assert!(engine.apply_multi_stage_qr_payload(&frame_at_layout(12)));

    assert!(engine.apply_multi_stage_state(ProtocolState::Finalized));
    let surface = presented_surface(&mut engine);

    assert_eq!(
        own_code_placement(&surface),
        Some(Some(placement(650, 175, 300)))
    );
}

// @internal
#[test]
fn every_placed_code_keeps_a_quiet_zone_inside_the_square() {
    let mut engine = engine_on_hover();

    for layout in 1..14 {
        assert!(engine.apply_multi_stage_qr_payload(&frame_at_layout(layout)));
        let at = own_code_placement(&presented_surface(&mut engine))
            .flatten()
            .unwrap_or_else(|| panic!("layout {layout} has a placement"));
        for edge in [
            at.x(),
            at.y(),
            1000 - at.size() - at.x(),
            1000 - at.size() - at.y(),
        ] {
            assert!(
                edge >= 50,
                "layout {layout} leaves {edge} permille at an edge"
            );
        }
    }
}

fn own_code_error_correction(
    surface: &SurfaceSpec,
) -> Option<Option<PresentationQrErrorCorrection>> {
    fn walk(nodes: &[PresentationNode]) -> Option<Option<PresentationQrErrorCorrection>> {
        nodes.iter().find_map(|node| match node {
            PresentationNode::Qr {
                purpose: PresentationQrPurpose::Display,
                error_correction,
                ..
            } => Some(*error_correction),
            PresentationNode::Group { children, .. } => walk(children),
            _ => None,
        })
    }
    walk(&surface.nodes)
}

/// At 7 cm the Pixel read the iPhone's 37-module frames and none of its
/// 41-module ones (rig series F7–L7, 2026-10-02). At level L every
/// exchange frame fits 37 modules, so the exchange code asks for it.
// @internal
#[test]
fn the_exchange_code_asks_for_the_error_correction_level_its_frame_names() {
    let mut engine = engine_on_hover();
    let mut frame = frame_at_layout(3);

    frame.error_correction = "L".into();
    assert!(engine.apply_multi_stage_qr_payload(&frame));
    let low = presented_surface(&mut engine);
    assert_eq!(
        own_code_error_correction(&low),
        Some(Some(PresentationQrErrorCorrection::Low))
    );
    assert!(
        serde_json::to_string(&low)
            .unwrap()
            .contains(r#""error_correction":"low""#)
    );

    frame.error_correction = "M".into();
    assert!(engine.apply_multi_stage_qr_payload(&frame));
    let unset = presented_surface(&mut engine);
    assert_eq!(own_code_error_correction(&unset), Some(None));
    assert!(
        !serde_json::to_string(&unset)
            .unwrap()
            .contains("error_correction")
    );
}
