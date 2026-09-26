// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stateful properties of `PresentationCoordinator` (CC-13): responsive
//! composition and surface continuity hold across arbitrary sequences of
//! resize, activation, and surface-change operations, not only the
//! boundary examples in `responsive_presentation_tests.rs`.

use proptest::prelude::*;
use vauchi_app::ui::{PresentationCoordinator, PresentationCoordinatorError};
use vauchi_core::{
    Command, Event, InputMode, MotionPreference, PaneLayout, PresentationProfile, SurfaceId,
    WindowClass,
};

const SURFACES: [&str; 4] = ["main", "detail", "list", "settings"];

fn surface(index: usize) -> SurfaceId {
    SurfaceId::new(SURFACES[index]).expect("valid surface id")
}

fn environment(width: u32) -> Event {
    Event::PresentationEnvironmentChanged {
        available_width: width,
        available_height: 900,
        input_modes: vec![InputMode::Touch],
        motion: MotionPreference::Full,
    }
}

#[derive(Clone, Debug)]
enum Op {
    Resize(u32),
    Activate(usize),
    SetDetail(Option<usize>),
    SetPrimary(usize),
}

fn width() -> impl Strategy<Value = u32> {
    prop_oneof![
        Just(0),
        Just(599),
        Just(600),
        Just(839),
        Just(840),
        Just(u32::MAX),
        0u32..2000,
        any::<u32>(),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    let index = 0..SURFACES.len();
    prop_oneof![
        3 => width().prop_map(Op::Resize),
        3 => index.clone().prop_map(Op::Activate),
        2 => proptest::option::of(index.clone()).prop_map(Op::SetDetail),
        1 => index.prop_map(Op::SetPrimary),
    ]
}

fn only_profile(commands: Vec<Command>) -> PresentationProfile {
    match commands.as_slice() {
        [Command::SetPresentationProfile { profile }] => profile.clone(),
        other => panic!("expected exactly one SetPresentationProfile, got {other:?}"),
    }
}

/// Re-reporting the current width is the only public way to read the
/// profile; it must not change anything but the (unchanged) window class.
fn probe(
    coordinator: &mut PresentationCoordinator,
    width: Option<u32>,
) -> Option<PresentationProfile> {
    width.map(|w| {
        only_profile(
            coordinator
                .handle_event(environment(w))
                .expect("reporting the environment always succeeds"),
        )
    })
}

fn expected_class(width: u32) -> WindowClass {
    match width {
        0..=599 => WindowClass::Compact,
        600..=839 => WindowClass::Medium,
        _ => WindowClass::Expanded,
    }
}

fn assert_composition(profile: &PresentationProfile) {
    let split = profile.window_class != WindowClass::Compact && profile.detail_surface.is_some();
    assert_eq!(
        profile.pane_layout,
        if split {
            PaneLayout::Split
        } else {
            PaneLayout::Single
        },
        "pane layout disagrees with window class and detail surface in {profile:?}"
    );
    assert!(
        profile.active_surface == profile.primary_surface
            || profile.detail_surface.as_ref() == Some(&profile.active_surface),
        "active surface is neither primary nor detail, so unreachable: {profile:?}"
    );
}

/// Applies `op`, returning the width now reported (if any).
fn apply(
    coordinator: &mut PresentationCoordinator,
    op: &Op,
    width: Option<u32>,
) -> (
    Option<u32>,
    Result<Vec<Command>, PresentationCoordinatorError>,
) {
    match op {
        Op::Resize(w) => (Some(*w), coordinator.handle_event(environment(*w))),
        Op::Activate(i) => (
            width,
            coordinator.handle_event(Event::SurfaceActivated {
                surface_id: surface(*i),
            }),
        ),
        Op::SetDetail(detail) => {
            coordinator.set_detail_surface(detail.map(surface));
            (width, Ok(Vec::new()))
        }
        Op::SetPrimary(i) => {
            coordinator.set_primary_surface(surface(*i));
            (width, Ok(Vec::new()))
        }
    }
}

fn run(ops: &[Op]) -> (PresentationCoordinator, Option<u32>) {
    let mut coordinator = PresentationCoordinator::new(surface(0));
    let mut width = None;
    for op in ops {
        width = apply(&mut coordinator, op, width).0;
    }
    (coordinator, width)
}

proptest! {
    /// Feature: generic_presentation_protocol.feature
    /// Scenario Outline: Available window drives structural composition
    // @scenario: generic_presentation_protocol.feature :: Available window drives structural composition
    #[test]
    fn test_first_reported_width_is_classified_into_its_band(w in width()) {
        let mut coordinator = PresentationCoordinator::new(surface(0));
        coordinator.set_detail_surface(Some(surface(1)));

        let profile = only_profile(
            coordinator.handle_event(environment(w)).expect("valid environment"),
        );

        prop_assert_eq!(profile.window_class, expected_class(w));
        assert_composition(&profile);
    }

    /// Feature: generic_presentation_protocol.feature
    /// Scenario Outline: Available window drives structural composition
    // @scenario: generic_presentation_protocol.feature :: Available window drives structural composition
    #[test]
    fn test_every_emitted_profile_is_composed_consistently(ops in prop::collection::vec(op(), 1..40)) {
        let mut coordinator = PresentationCoordinator::new(surface(0));
        let mut width = None;
        for op in &ops {
            let (next_width, result) = apply(&mut coordinator, op, width);
            width = next_width;
            match result {
                Ok(commands) if commands.is_empty() => {}
                Ok(commands) => assert_composition(&only_profile(commands)),
                Err(error) => prop_assert!(
                    matches!(
                        error,
                        PresentationCoordinatorError::SurfaceNotVisible
                            | PresentationCoordinatorError::EnvironmentMissing
                    ),
                    "unexpected rejection {error:?} for {op:?}"
                ),
            }
            if let Some(profile) = probe(&mut coordinator, width) {
                assert_composition(&profile);
            }
        }
    }

    /// Feature: generic_presentation_protocol.feature
    /// Scenario: Responsive transitions preserve interaction state
    // @scenario: generic_presentation_protocol.feature :: Responsive transitions preserve interaction state
    #[test]
    fn test_resizing_never_moves_surfaces_and_round_trips_losslessly(
        prefix in prop::collection::vec(op(), 0..30),
        start in width(),
        activate in 0..SURFACES.len(),
        resizes in prop::collection::vec(width(), 1..20),
    ) {
        let (mut coordinator, _) = run(&prefix);
        let mut before = probe(&mut coordinator, Some(start)).expect("width reported");
        // The baseline must come from a non-resize event: read through a
        // resize, a collapse that drops the active surface would reset the
        // baseline too and compare equal.
        match coordinator.handle_event(Event::SurfaceActivated {
            surface_id: surface(activate),
        }) {
            Ok(commands) => before = only_profile(commands),
            Err(error) => prop_assert_eq!(error, PresentationCoordinatorError::SurfaceNotVisible),
        }

        for w in &resizes {
            let during = only_profile(
                coordinator.handle_event(environment(*w)).expect("valid environment"),
            );
            prop_assert_eq!(&during.primary_surface, &before.primary_surface);
            prop_assert_eq!(&during.detail_surface, &before.detail_surface);
            prop_assert_eq!(&during.active_surface, &before.active_surface);
        }

        let after = probe(&mut coordinator, Some(start)).expect("width reported");
        prop_assert_eq!(after, before);
    }

    /// Feature: generic_presentation_protocol.feature
    /// Scenario: Responsive transitions preserve interaction state
    // @scenario: generic_presentation_protocol.feature :: Responsive transitions preserve interaction state
    #[test]
    fn test_rejected_activation_leaves_profile_unchanged(
        prefix in prop::collection::vec(op(), 0..30),
        start in width(),
        target in 0..SURFACES.len(),
    ) {
        let (mut coordinator, _) = run(&prefix);
        let before = probe(&mut coordinator, Some(start)).expect("width reported");
        let visible = before.active_surface == surface(target)
            || (before.pane_layout == PaneLayout::Split
                && (before.primary_surface == surface(target)
                    || before.detail_surface == Some(surface(target))));

        let result = coordinator.handle_event(Event::SurfaceActivated {
            surface_id: surface(target),
        });

        if visible {
            let profile = only_profile(result.expect("visible surface activates"));
            prop_assert_eq!(profile.active_surface, surface(target));
        } else {
            prop_assert_eq!(result, Err(PresentationCoordinatorError::SurfaceNotVisible));
            let after = probe(&mut coordinator, Some(start)).expect("width reported");
            prop_assert_eq!(after, before);
        }
    }

    /// Feature: generic_presentation_protocol.feature
    /// Scenario: Responsive transitions preserve interaction state
    // @scenario: generic_presentation_protocol.feature :: Responsive transitions preserve interaction state
    #[test]
    fn test_activation_before_any_environment_is_refused(target in 0..SURFACES.len()) {
        let mut coordinator = PresentationCoordinator::new(surface(0));

        let result = coordinator.handle_event(Event::SurfaceActivated {
            surface_id: surface(target),
        });

        prop_assert_eq!(result, Err(PresentationCoordinatorError::EnvironmentMissing));
    }
}
