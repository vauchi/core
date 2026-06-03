// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-internal BACK for the exchange flow.
//!
//! The exchange flow is a multi-step state machine (`ExchangeStep`)
//! living under the single `AppScreen::Exchange`. Step transitions never
//! touch the AppScreen `nav_history`, so a BACK press used to jump out of
//! the whole flow — or, because `Exchange` is an `is_root` screen, do
//! nothing at all (the "Exchange Mode" back-trap from the 2026-06-01
//! device pass).
//!
//! This module gives the engine its own back-stack
//! (`ExchangeEngine::step_history`, pushed only on *user-initiated*
//! selection-phase forward transitions) and the two primitives
//! `AppEngine::navigate_back` / `can_go_back` delegate to via the
//! `WorkflowEngine::{navigate_back_within, can_navigate_back_within}`
//! hooks (wired in `mod.rs`).
//!
//! Back-safety is the load-bearing invariant: only *selection* and
//! *pre-handshake entry* steps may be rewound. Mid-handshake and terminal
//! steps must never rewind — doing so would discard live cryptographic
//! session state mid-protocol.

use super::{BleStep, DirectStep, ExchangeStep, NfcStep, mode_selection::ModeSelectionEngine};

impl super::ExchangeEngine {
    /// A step is back-safe when the user is *choosing* (selection phase)
    /// or sitting at the *entry* of a sub-flow before any handshake has
    /// begun. Mid-protocol and terminal steps are never rewound.
    ///
    /// `ModeSelection` itself is deliberately **not** back-safe: it is the
    /// flow's root step, so BACK there must exit the Exchange screen (the
    /// AppScreen-level `navigate_back` / tab convention), not rewind.
    fn is_back_safe_step(step: &ExchangeStep) -> bool {
        matches!(
            step,
            ExchangeStep::GroupSelection
                | ExchangeStep::FieldPreview
                | ExchangeStep::NfcRoleSelection
                | ExchangeStep::Ble(BleStep::Discovering)
                | ExchangeStep::Nfc(NfcStep::AwaitingTap)
                | ExchangeStep::DirectTransport(DirectStep::WaitingForConnection)
        )
    }

    /// `true` when a BACK press should rewind one internal step rather
    /// than leave the Exchange screen. Backs the
    /// `WorkflowEngine::can_navigate_back_within` override.
    pub(super) fn can_back_within(&self) -> bool {
        !self.step_history.is_empty() && Self::is_back_safe_step(&self.step)
    }

    /// Rewind one internal step. Returns `true` if a step was consumed.
    /// Backs the `WorkflowEngine::navigate_back_within` override.
    pub(super) fn back_within(&mut self) -> bool {
        if !self.can_back_within() {
            return false;
        }
        // Safe: `can_back_within` checked non-empty.
        let prev = self.step_history.pop().expect("step_history non-empty");
        self.restore_step(prev);
        true
    }

    /// Restore a previously-recorded selection step, tearing down any
    /// forward sub-flow state so the rewound screen renders cleanly and a
    /// subsequent forward choice starts a fresh session.
    ///
    /// Only `ModeSelection` and `GroupSelection` are ever pushed onto
    /// `step_history` (see the recording sites in `mod.rs::handle_action`),
    /// so those are the cases that matter; the catch-all simply sets the
    /// step for forward-compat.
    fn restore_step(&mut self, prev: ExchangeStep) {
        // Forward sub-flow / live-session state is invalid once we step
        // back; drop it in every case. The dropped `session` was at most a
        // not-yet-used QR/transport session (back-safety guarantees no
        // handshake was in flight), so this discards no confirmed contact.
        self.session = None;
        self.ble_flow = None;
        self.nfc_flow = None;
        self.field_preview = None;
        self.scanned_data = None;

        match prev {
            ExchangeStep::ModeSelection => {
                // Re-arm the mode picker and forget the chosen mode +
                // group selection so the user genuinely starts over.
                self.config.mode = None;
                self.selected_groups.clear();
                self.mode_selection = Some(ModeSelectionEngine::new(
                    self.config.device_capabilities.clone(),
                ));
                self.step = ExchangeStep::ModeSelection;
            }
            ExchangeStep::GroupSelection => {
                // Keep the chosen mode + group selection; just return to
                // the group picker.
                self.step = ExchangeStep::GroupSelection;
            }
            other => {
                self.step = other;
            }
        }
    }
}
