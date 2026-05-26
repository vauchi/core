// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Presentational view-state for the BLE exchange status indicator.
//!
//! Kept separate from the `exchange` UniFFI facade so the
//! state-machine wrapper does not grow, and so the
//! `MobileExchangeState` → label/progress mapping lives in one tested
//! place (ADR-021/043 Humble UI).

use crate::exchange::MobileExchangeState;

/// Presentational view-state for a BLE exchange status indicator.
///
/// Core owns the `MobileExchangeState` → label/progress mapping so
/// frontends render `label_key` (via their i18n table) and
/// `show_progress` directly, instead of duplicating a
/// `when (MobileExchangeState)` switch per platform.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MobileExchangeViewState {
    /// i18n key for the status label (e.g. `"exchange.waiting_peer"`).
    pub label_key: String,
    /// Whether to render an in-progress affordance (spinner) alongside
    /// the label. `false` for terminal states (Complete / Failed).
    pub show_progress: bool,
}

/// Compute the presentational view-state for a BLE exchange status.
///
/// Pure mapping — the single source of truth for which i18n label and
/// progress affordance each exchange state shows.
#[uniffi::export]
pub fn exchange_view_state(state: MobileExchangeState) -> MobileExchangeViewState {
    let (label_key, show_progress) = match state {
        MobileExchangeState::Idle | MobileExchangeState::DisplayingQr { .. } => {
            ("exchange.waiting_peer", true)
        }
        MobileExchangeState::PeerScanned => ("exchange.peer_found", true),
        MobileExchangeState::AwaitingKeyAgreement => ("exchange.verifying", true),
        MobileExchangeState::AwaitingCardExchange => ("exchange.transferring", true),
        MobileExchangeState::Complete { .. } => ("exchange.contact_exchanged", false),
        MobileExchangeState::Failed { .. } => ("exchange.failed_title", false),
    };
    MobileExchangeViewState {
        label_key: label_key.to_string(),
        show_progress,
    }
}

// INLINE_TEST_REQUIRED: pure mapping; co-locate the label/progress table
// with the implementation.
#[cfg(test)]
mod tests {
    use super::{MobileExchangeState, exchange_view_state};

    // @internal
    #[test]
    fn exchange_view_state_maps_each_state_to_label_and_progress() {
        let cases = [
            (MobileExchangeState::Idle, "exchange.waiting_peer", true),
            (
                MobileExchangeState::DisplayingQr {
                    qr_data: "x".into(),
                },
                "exchange.waiting_peer",
                true,
            ),
            (
                MobileExchangeState::PeerScanned,
                "exchange.peer_found",
                true,
            ),
            (
                MobileExchangeState::AwaitingKeyAgreement,
                "exchange.verifying",
                true,
            ),
            (
                MobileExchangeState::AwaitingCardExchange,
                "exchange.transferring",
                true,
            ),
            (
                MobileExchangeState::Complete {
                    contact_id: "c".into(),
                    contact_name: "Alice".into(),
                },
                "exchange.contact_exchanged",
                false,
            ),
            (
                MobileExchangeState::Failed {
                    error: "boom".into(),
                },
                "exchange.failed_title",
                false,
            ),
        ];
        for (state, expected_key, expected_progress) in cases {
            let vs = exchange_view_state(state.clone());
            assert_eq!(vs.label_key, expected_key, "label for {state:?}");
            assert_eq!(
                vs.show_progress, expected_progress,
                "progress for {state:?}"
            );
        }
    }
}
