// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Flow-agnostic "verification in progress" screen.
//!
//! Relocated from `exchange/qr.rs` (2026-06-03) so it outlives the legacy QR
//! sub-flow: the `session.state()` → step sync drives
//! [`crate::ui::exchange::ExchangeStep::Verifying`] for the QR/session path,
//! and this renders it. The screen is intentionally flow-agnostic
//! (`screen_id: "exchange_verifying"`) — no QR component, no mode-specific
//! copy.

use crate::ui::*;

/// Builds the flow-agnostic "Verifying" screen.
pub(super) fn build_verifying_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_verifying".into(),
        title: "Verifying".into(),
        subtitle: None,
        components: vec![Component::StatusIndicator {
            id: "verifying_status".into(),
            icon: None,
            title: "Verifying...".into(),
            detail: None,
            status: Status::InProgress,
            a11y: Some(A11y {
                label: Some("Verifying exchange".into()),
                hint: Some("Confirming the other person's identity".into()),
                role: None,
            }),
        }],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

// INLINE_TEST_REQUIRED: tests assert the flow-agnostic verifying screen shape
// that the neutral ExchangeStep::Verifying renders (no public API surface).
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn verifying_screen_is_flow_agnostic() {
        let screen = build_verifying_screen(Progress {
            current_step: 6,
            total_steps: 8,
            label: None,
        });
        assert_eq!(screen.screen_id, "exchange_verifying");
        // No QR component — the screen is shared across exchange flows.
        assert!(
            !screen
                .components
                .iter()
                .any(|c| matches!(c, Component::QrCode { .. })),
            "verifying screen must not carry a QR component",
        );
        match &screen.components[0] {
            Component::StatusIndicator { status, title, .. } => {
                assert_eq!(*status, Status::InProgress);
                assert_eq!(title, "Verifying...");
            }
            other => panic!("expected StatusIndicator, got {other:?}"),
        }
    }
}
