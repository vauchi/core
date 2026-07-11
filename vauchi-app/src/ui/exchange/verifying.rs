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
pub(super) fn build_verifying_screen(locale: crate::i18n::Locale) -> ScreenModel {
    let t = |key: &str| crate::i18n::get_string(locale, key);
    ScreenModel {
        screen_id: "exchange_verifying".into(),
        title: t("exchange.verifying.title"),
        subtitle: None,
        components: vec![Component::StatusIndicator {
            id: "verifying_status".into(),
            icon: None,
            title: t("exchange.verifying.status"),
            detail: None,
            status: Status::InProgress,
            status_label: t(Status::InProgress.label_key()),
            a11y: Some(A11y {
                label: Some(t("exchange.verifying.a11y")),
                hint: Some(t("exchange.verifying.a11y_hint")),
                role: None,
            }),
        }],
        actions: vec![],
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
        let screen = build_verifying_screen(crate::i18n::Locale::English);
        assert_eq!(screen.screen_id, "exchange_verifying");
        assert!(screen.progress.is_none(), "no numeric progress (M2 S2)");
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
