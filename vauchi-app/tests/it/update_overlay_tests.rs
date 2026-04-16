// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AppEngine update overlay (banner, blocking screen, dismissal).

use vauchi_app::ui::{ActionResult, AppEngine, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::version::VersionPolicy;

fn test_engine() -> AppEngine {
    let vauchi = Vauchi::in_memory().unwrap();
    AppEngine::new(vauchi)
}

const FAR_FUTURE: u64 = 4_000_000_000; // ~2096, always in the future for tests

// ---------------------------------------------------------------------------
// set_version_policy + apply_update_overlay
// ---------------------------------------------------------------------------

// @internal
#[test]
fn up_to_date_policy_does_not_inject_banner() {
    let mut engine = test_engine();
    let policy = VersionPolicy {
        min_version: 0,
        warn_version: 0,
        grace_deadline: None,
    };
    engine.set_version_policy(&policy);

    let screen = engine.current_screen();
    assert!(
        !screen
            .components
            .iter()
            .any(|c| format!("{c:?}").contains("update")),
        "up-to-date screen should not have update components"
    );
}

// @internal
#[test]
fn update_available_injects_dismissible_banner() {
    let mut engine = test_engine();
    // warn_version > APP_COMPAT_VERSION (1) → UpdateAvailable
    let policy = VersionPolicy {
        min_version: 0,
        warn_version: 2,
        grace_deadline: None,
    };
    engine.set_version_policy(&policy);

    let screen = engine.current_screen();
    let has_banner = screen
        .components
        .iter()
        .any(|c| format!("{c:?}").contains("new version"));
    assert!(has_banner, "UpdateAvailable should show banner");
}

// @internal
#[test]
fn dismissing_update_available_hides_banner() {
    let mut engine = test_engine();
    let policy = VersionPolicy {
        min_version: 0,
        warn_version: 2,
        grace_deadline: None,
    };
    engine.set_version_policy(&policy);

    // Dismiss via action
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_update_link".into(),
    });
    assert!(
        matches!(result, ActionResult::OpenUrl { ref url } if url == "vauchi://update"),
        "open_update_link should return OpenUrl"
    );

    // Banner should be gone after dismissal
    let screen = engine.current_screen();
    let has_banner = screen
        .components
        .iter()
        .any(|c| format!("{c:?}").contains("new version"));
    assert!(
        !has_banner,
        "banner should be hidden after dismiss for UpdateAvailable"
    );
}

// @internal
#[test]
fn update_required_with_grace_shows_deadline_banner() {
    let mut engine = test_engine();
    // min_version > APP_COMPAT_VERSION (1), grace in far future
    let policy = VersionPolicy {
        min_version: 2,
        warn_version: 3,
        grace_deadline: Some(FAR_FUTURE),
    };
    engine.set_version_policy(&policy);

    let screen = engine.current_screen();
    let has_deadline_banner = screen
        .components
        .iter()
        .any(|c| format!("{c:?}").contains("required by"));
    assert!(
        has_deadline_banner,
        "UpdateRequired with grace should show deadline banner"
    );
}

// @internal
#[test]
fn update_required_no_grace_shows_blocking_screen() {
    let mut engine = test_engine();
    // min_version > APP_COMPAT_VERSION (1), no grace deadline
    let policy = VersionPolicy {
        min_version: 2,
        warn_version: 3,
        grace_deadline: None,
    };
    engine.set_version_policy(&policy);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "update_required");
    let has_blocking_message = screen
        .components
        .iter()
        .any(|c| format!("{c:?}").contains("no longer supported"));
    assert!(has_blocking_message, "blocking screen should show message");
}

// @internal
#[test]
fn update_required_resets_dismissed_flag() {
    let mut engine = test_engine();

    // First: set UpdateAvailable and dismiss it
    let soft_policy = VersionPolicy {
        min_version: 0,
        warn_version: 2,
        grace_deadline: None,
    };
    engine.set_version_policy(&soft_policy);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_update_link".into(),
    });

    // Now upgrade to UpdateRequired — dismissed flag should reset
    let hard_policy = VersionPolicy {
        min_version: 2,
        warn_version: 3,
        grace_deadline: Some(FAR_FUTURE),
    };
    engine.set_version_policy(&hard_policy);

    let screen = engine.current_screen();
    let has_banner = screen
        .components
        .iter()
        .any(|c| format!("{c:?}").contains("required by"));
    assert!(
        has_banner,
        "UpdateRequired should reset dismissed flag and show banner"
    );
}

// @internal
#[test]
fn open_update_link_returns_open_url() {
    let mut engine = test_engine();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_update_link".into(),
    });

    match result {
        ActionResult::OpenUrl { url } => {
            assert_eq!(url, "vauchi://update");
        }
        other => panic!("expected OpenUrl, got {other:?}"),
    }
}
