// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::PresentationCoordinator;
use vauchi_core::{
    Command, Event, InputMode, MotionPreference, PaneLayout, PresentationProfile, SurfaceId,
    WindowClass,
};

fn surface(id: &str) -> SurfaceId {
    SurfaceId::new(id).expect("valid surface id")
}

fn environment(width: u32) -> Event {
    Event::PresentationEnvironmentChanged {
        available_width: width,
        available_height: 900,
        input_modes: vec![InputMode::Touch],
        motion: MotionPreference::Full,
    }
}

fn profile(window_class: WindowClass, pane_layout: PaneLayout, active_surface: &str) -> Command {
    Command::SetPresentationProfile {
        profile: PresentationProfile {
            window_class,
            pane_layout,
            primary_surface: surface("main"),
            detail_surface: Some(surface("detail")),
            active_surface: surface(active_surface),
        },
    }
}

/// Feature: generic_presentation_protocol.feature
/// Scenario Outline: Available window drives structural composition
// @scenario: generic_presentation_protocol.feature :: Available window drives structural composition
#[test]
fn test_available_width_boundaries_emit_core_owned_composition() {
    let cases = [
        (599, WindowClass::Compact, PaneLayout::Single),
        (600, WindowClass::Medium, PaneLayout::Split),
        (839, WindowClass::Medium, PaneLayout::Split),
        (840, WindowClass::Expanded, PaneLayout::Split),
    ];

    for (width, window_class, pane_layout) in cases {
        let mut coordinator = PresentationCoordinator::new(surface("main"));
        coordinator.set_detail_surface(Some(surface("detail")));

        assert_eq!(
            coordinator
                .handle_event(environment(width))
                .expect("valid environment"),
            vec![profile(window_class, pane_layout, "main")],
            "wrong composition at width {width}"
        );
    }
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Responsive transitions preserve interaction state
// @scenario: generic_presentation_protocol.feature :: Responsive transitions preserve interaction state
#[test]
fn test_collapse_and_expand_preserve_active_detail_surface() {
    let mut coordinator = PresentationCoordinator::new(surface("main"));
    coordinator.set_detail_surface(Some(surface("detail")));

    assert_eq!(
        coordinator
            .handle_event(environment(840))
            .expect("expanded environment"),
        vec![profile(WindowClass::Expanded, PaneLayout::Split, "main")]
    );
    assert_eq!(
        coordinator
            .handle_event(Event::SurfaceActivated {
                surface_id: surface("detail"),
            })
            .expect("activate visible detail"),
        vec![profile(WindowClass::Expanded, PaneLayout::Split, "detail")]
    );
    assert_eq!(
        coordinator
            .handle_event(environment(567))
            .expect("compact environment"),
        vec![profile(WindowClass::Compact, PaneLayout::Single, "detail")]
    );
    assert_eq!(
        coordinator
            .handle_event(environment(840))
            .expect("expanded environment"),
        vec![profile(WindowClass::Expanded, PaneLayout::Split, "detail")]
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Class boundaries are damped on collapse
// @scenario: generic_presentation_protocol.feature :: Class boundaries are damped on collapse
#[test]
fn test_collapse_waits_for_the_damping_band_but_expansion_does_not() {
    let mut coordinator = PresentationCoordinator::new(surface("main"));
    coordinator.set_detail_surface(Some(surface("detail")));
    let mut class_at = |width| match coordinator
        .handle_event(environment(width))
        .expect("valid environment")
        .as_slice()
    {
        [Command::SetPresentationProfile { profile }] => profile.window_class,
        other => panic!("expected one profile, got {other:?}"),
    };

    assert_eq!(class_at(600), WindowClass::Medium);
    assert_eq!(class_at(568), WindowClass::Medium, "within 32 below 600");
    assert_eq!(class_at(567), WindowClass::Compact, "past the band");
    assert_eq!(class_at(599), WindowClass::Compact, "still below 600");
    assert_eq!(
        class_at(600),
        WindowClass::Medium,
        "expands at the threshold"
    );
    assert_eq!(class_at(840), WindowClass::Expanded);
    assert_eq!(class_at(808), WindowClass::Expanded, "within 32 below 840");
    assert_eq!(class_at(807), WindowClass::Medium, "past the band");
    assert_eq!(class_at(839), WindowClass::Medium, "still below 840");
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Class boundaries are damped on collapse
// @scenario: generic_presentation_protocol.feature :: Class boundaries are damped on collapse
#[test]
fn test_one_jump_across_both_boundaries_lands_in_the_damped_class() {
    let mut coordinator = PresentationCoordinator::new(surface("main"));
    coordinator.set_detail_surface(Some(surface("detail")));
    coordinator
        .handle_event(environment(1200))
        .expect("expanded environment");

    assert_eq!(
        coordinator
            .handle_event(environment(580))
            .expect("medium environment"),
        vec![Command::SetPresentationProfile {
            profile: PresentationProfile {
                window_class: WindowClass::Medium,
                pane_layout: PaneLayout::Split,
                primary_surface: surface("main"),
                detail_surface: Some(surface("detail")),
                active_surface: surface("main"),
            },
        }],
        "580 is within the band below 600, so it stays medium, not compact"
    );
}
