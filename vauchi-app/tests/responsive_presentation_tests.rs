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
            .handle_event(environment(599))
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
