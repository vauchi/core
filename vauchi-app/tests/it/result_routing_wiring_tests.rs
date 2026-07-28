// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `AppEngine` boundary contract: engine-level domain results are resolved
//! by `route_result` before they reach a frontend (ADR-043/ADR-044).
//!
//! Frontends driving core through `AppEngine` (cabi: Windows, linux-qt;
//! Rust: linux-gtk) must never receive `StartDeviceLink` or
//! `VerifyFingerprint` — deleting their handling branches is safe only
//! while these tests hold (`2026-07-06-desktop-tui-web-domain-shell-
//! violations` W1/G1/G2/Q3). The screen ids pinned here are the hosted
//! engines' rendered first steps, not `AppScreen::screen_id` constants.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn link_device_resolves_to_device_linking_navigation() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::DeviceManagement);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "link_device".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            // Lands on the generating-link spinner, not a bare QR: the real
            // vauchi:// invitation only appears after a successful relay offer
            // (QrReady). See device_link_initiator regression test.
            assert_eq!(screen.screen_id, "link_qr_pending");
            assert_eq!(
                screen.parent_screen_id.as_deref(),
                Some("device_management")
            );
        }
        other => panic!("StartDeviceLink must not cross the AppEngine boundary; got {other:?}"),
    }
}

// @internal
#[test]
fn verify_fingerprint_resolves_to_navigation() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .import_contacts_from_vcf(b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n")
        .unwrap();
    let contact_id = vauchi.list_contacts().unwrap()[0].id().to_string();
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.clone(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "verify_fingerprint".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "fingerprint_verify");
        }
        other => panic!("VerifyFingerprint must not cross the AppEngine boundary; got {other:?}"),
    }
}
