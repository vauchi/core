// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use vauchi_core::{Event, OverlayKind, SurfaceId};

fn surface(id: &str) -> SurfaceId {
    SurfaceId::new(id).expect("valid surface id")
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: User interaction returns as an opaque event
// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn test_back_and_overlay_dismissal_events_remain_presentation_only() {
    let back = Event::BackRequested {
        surface_id: surface("detail"),
    };
    let dismissed = Event::OverlayDismissed {
        surface_id: surface("detail"),
        kind: OverlayKind::ActionMenu,
    };

    assert_eq!(
        serde_json::to_value(back).expect("serialize back request"),
        json!({
            "BackRequested": {
                "surface_id": "detail"
            }
        })
    );
    assert_eq!(
        serde_json::to_value(dismissed).expect("serialize overlay dismissal"),
        json!({
            "OverlayDismissed": {
                "surface_id": "detail",
                "kind": "action_menu"
            }
        })
    );
}
