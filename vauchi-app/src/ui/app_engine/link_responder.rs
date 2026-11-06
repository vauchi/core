// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Link-mode responder lifecycle on `AppEngine`.
//!
//! While the active screen is `AppScreen::DeepLinkResponder` (a link whose
//! consent was granted), the person first types the code the sender passed on
//! another way. Only then does the engine hold a Core
//! [`LinkExchange`](vauchi_core::api::LinkExchange), accepted with
//! `Vauchi::accept_link_exchange_payload`, and advance it on each heartbeat
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
        self.engine_cache.remove(&AppScreen::Contacts);
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

    /// Drop the responder exchange as navigation leaves
    /// `AppScreen::DeepLinkResponder`. Called from `navigate_to_internal`.
    pub(super) fn sync_link_responder_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::DeepLinkResponder { .. });
        let is = matches!(new, AppScreen::DeepLinkResponder { .. });
        #[cfg(all(feature = "network-http", feature = "storage"))]
        if was && !is {
            self.link_responder = None;
        }
        #[cfg(not(all(feature = "network-http", feature = "storage")))]
        let _ = (was, is);
    }

    /// A code submitted on the responder's code screen: a well-formed one
    /// starts the exchange, anything else asks again and sends nothing. Reads
    /// the typed value from the rendered screen, which has the input only while
    /// the code is still owed.
    pub(super) fn intercept_link_code(
        &mut self,
        action: &crate::ui::UserAction,
    ) -> Option<ActionResult> {
        use crate::ui::link_responder::{ACTION_CONFIRM_CODE, LINK_CODE_INPUT_ID};
        use crate::ui::{EngineUpdate, LinkResponderUpdate, UserAction};

        let AppScreen::DeepLinkResponder { payload } = self.current_app_screen() else {
            return None;
        };
        let requested = match action {
            UserAction::ActionPressed { action_id } => action_id == ACTION_CONFIRM_CODE,
            UserAction::TextSubmitted { component_id } => component_id == LINK_CODE_INPUT_ID,
            _ => false,
        };
        if !requested {
            return None;
        }
        let payload = payload.clone();
        let typed = self
            .engine
            .current_screen()
            .components
            .iter()
            .find_map(|c| match c {
                crate::ui::Component::TextInput { id, value, .. } if id == LINK_CODE_INPUT_ID => {
                    Some(zeroize::Zeroizing::new(value.clone()))
                }
                _ => None,
            })?;
        let update = match vauchi_core::exchange::link_pake::LinkCode::parse(&typed) {
            Ok(code) => {
                self.start_link_responder(&payload, &code);
                LinkResponderUpdate::CodeAccepted
            }
            Err(_) => LinkResponderUpdate::CodeInvalid,
        };
        let _ = self
            .engine
            .apply_update(EngineUpdate::LinkResponder(update));
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    #[cfg(all(feature = "network-http", feature = "storage"))]
    fn start_link_responder(
        &mut self,
        payload: &vauchi_core::exchange::link_mode::DeepLinkPayload,
        code: &vauchi_core::exchange::link_pake::LinkCode,
    ) {
        match self.vauchi.accept_link_exchange_payload(payload, code) {
            Ok(exchange) => {
                self.link_responder = Some(exchange);
                self.reschedule_wakeup_for_new_session();
            }
            // Only a missing identity or a small-order key in the link get
            // here; the waiting screen would otherwise never end.
            Err(_) => {
                self.link_responder_failed("handshake_failed".to_string());
            }
        }
    }

    #[cfg(not(all(feature = "network-http", feature = "storage")))]
    fn start_link_responder(
        &mut self,
        _payload: &vauchi_core::exchange::link_mode::DeepLinkPayload,
        _code: &vauchi_core::exchange::link_pake::LinkCode,
    ) {
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
