// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link-mode responder lifecycle on `AppEngine`.
//!
//! While the active screen is `AppScreen::DeepLinkResponder` (a link whose
//! consent was granted), the engine holds a Core
//! [`LinkExchange`](vauchi_core::api::LinkExchange) accepted with
//! `Vauchi::accept_link_exchange_payload` and advances it on each heartbeat
//! with `Vauchi::advance_link_exchange`, which runs the relay escrow and
//! completes the contact (ADR-049, ADR-050 T5b). The engine only maps the
//! outcome to `link_responder_completed` or `link_responder_failed`. The
//! same API serves the CLI, so the ceremony's loop exists once, in Core
//! (private#670). Frontends own no workflow logic (ADR-021/043, ADR-031).
//!
//! Design: `_private/docs/designs/2026-05-25-slice-32l-phase-2-responder-screen-driven-design.md`.

use super::{AppEngine, AppScreen};
use crate::ui::ActionResult;
use crate::ui::ScreenModel;

use vauchi_core::Event;

impl AppEngine {
    /// Terminal success — the sender's card was retrieved and persisted.
    /// Builds the rich success summary from the persisted contact, attaches
    /// it, and transitions the responder engine to
    /// `link_responder_completed`.
    pub fn link_responder_completed(&mut self, contact_id: &str) -> Option<ScreenModel> {
        // Build the summary first so the immutable `self.vauchi` borrow ends
        // before the `&mut self` engine borrow. Link mode assigns no group.
        let summary = self.build_exchange_summary(contact_id, Vec::new());
        // Ceremony (M2 S4): the sender's card was retrieved + persisted —
        // validated success, once per responder session.
        self.extend_pending_commands(vec![crate::ui::exchange::ceremony::exchange_celebrate()]);
        self.engine
            .apply_update(crate::ui::EngineUpdate::LinkResponder(
                crate::ui::LinkResponderUpdate::Completed(summary),
            ))
            .then(|| self.engine.current_screen())
    }

    /// Terminal failure. `reason` is the stable `LinkResponder` failure
    /// id. Transitions the responder engine to `link_responder_failed`.
    pub fn link_responder_failed(&mut self, reason: String) -> Option<ScreenModel> {
        self.engine
            .apply_update(crate::ui::EngineUpdate::LinkResponder(
                crate::ui::LinkResponderUpdate::Failed(reason),
            ))
            .then(|| self.engine.current_screen())
    }

    /// Accept / drop the responder exchange as navigation enters or leaves
    /// `AppScreen::DeepLinkResponder`. Called from `navigate_to_internal`.
    pub(super) fn sync_link_responder_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::DeepLinkResponder { .. });
        let is = matches!(new, AppScreen::DeepLinkResponder { .. });
        #[cfg(all(feature = "network-http", feature = "storage"))]
        match (was, is) {
            (true, false) => self.link_responder = None,
            (false, true) => {
                if let AppScreen::DeepLinkResponder { payload } = new {
                    // No identity or a key-derive failure leaves the screen
                    // on its waiting state; both are rare and non-fatal here.
                    self.link_responder = self.vauchi.accept_link_exchange_payload(payload).ok();
                    self.reschedule_wakeup_for_new_session();
                }
            }
            _ => {}
        }
        #[cfg(not(all(feature = "network-http", feature = "storage")))]
        let _ = (was, is);
    }

    /// Hand a relay hardware event to the responder exchange and reconcile
    /// the screen. A no-op off the responder screen, for unrelated events,
    /// or with no live exchange. Core runs any follow-up relay step itself.
    pub(super) fn route_link_responder_hardware_event(
        &mut self,
        event: &Event,
    ) -> Option<ActionResult> {
        if !matches!(self.screen, AppScreen::DeepLinkResponder { .. })
            || !matches!(
                event,
                Event::RelayEscrowReady { .. }
                    | Event::RelayEscrowFailed { .. }
                    | Event::RelayEscrowBlobReceived { .. }
            )
        {
            return None;
        }
        #[cfg(all(feature = "network-http", feature = "storage"))]
        {
            let mut exchange = self.link_responder.take()?;
            let progress = self
                .vauchi
                .feed_link_exchange_event(&mut exchange, event.clone())
                .ok()?;
            self.present_link_responder(exchange, progress);
        }
        None
    }

    /// Advance the responder exchange one relay step (ADR-049). Called each
    /// `poll_notifications` tick; a no-op off the responder screen. Returns
    /// `true` once the exchange has ended.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    pub(crate) fn advance_link_responder_session(&mut self) -> bool {
        if !matches!(self.screen, AppScreen::DeepLinkResponder { .. }) {
            return false;
        }
        let Some(mut exchange) = self.link_responder.take() else {
            return false;
        };
        match self.vauchi.advance_link_exchange(&mut exchange) {
            Ok(progress) => self.present_link_responder(exchange, progress),
            Err(_) => {
                self.link_responder = Some(exchange);
                false
            }
        }
    }

    /// Success and failure end the exchange and show their screen; while
    /// waiting the screen stays as it is.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    fn present_link_responder(
        &mut self,
        exchange: vauchi_core::api::LinkExchange,
        progress: vauchi_core::api::LinkExchangeProgress,
    ) -> bool {
        use vauchi_core::api::LinkExchangeProgress;
        match progress {
            LinkExchangeProgress::Completed { contact_id } => {
                self.link_responder_completed(&contact_id);
                true
            }
            LinkExchangeProgress::Failed { reason } => {
                self.link_responder_failed(reason);
                true
            }
            LinkExchangeProgress::Waiting => {
                self.link_responder = Some(exchange);
                false
            }
        }
    }
}
