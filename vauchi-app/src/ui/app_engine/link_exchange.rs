// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Link-mode initiator lifecycle on `AppEngine`.
//!
//! While the active screen is `AppScreen::LinkExchange`, the engine holds a
//! Core [`LinkExchange`](vauchi_core::api::LinkExchange) started with
//! `Vauchi::start_link_exchange`, shows its share link, and advances it on
//! each heartbeat with `Vauchi::advance_link_exchange`, which runs the relay
//! escrow and completes the contact (ADR-049, ADR-050 T5b). The engine only
//! maps progress to screens: the share link while the peer has not joined,
//! "retrieving" once the peer's card is on its way, then success or
//! failure. The same API serves the CLI's `vauchi exchange link`, so the
//! ceremony's loop exists once, in Core (private#670). Frontends pull no
//! session object and own no workflow logic (ADR-021/043, ADR-031).

use super::{AppEngine, AppScreen};
use crate::ui::ActionResult;

use vauchi_core::Event;

impl AppEngine {
    /// A link pasted on the share screen is routed exactly like an opened
    /// deep link, so a camera-less device (the TUI) can answer a peer's
    /// Link exchange. Reads the typed value from the rendered screen.
    pub(super) fn intercept_open_peer_link(
        &mut self,
        action: &crate::ui::UserAction,
    ) -> Option<ActionResult> {
        use crate::ui::UserAction;
        use crate::ui::link_exchange::{ACTION_OPEN_PEER_LINK, PEER_LINK_INPUT_ID};

        if !matches!(self.current_app_screen(), AppScreen::LinkExchange) {
            return None;
        }
        let requested = match action {
            UserAction::ActionPressed { action_id } => action_id == ACTION_OPEN_PEER_LINK,
            UserAction::TextSubmitted { component_id } => component_id == PEER_LINK_INPUT_ID,
            _ => false,
        };
        if !requested {
            return None;
        }
        let uri = self
            .engine
            .current_screen()
            .components
            .iter()
            .find_map(|c| match c {
                crate::ui::Component::TextInput { id, value, .. } if id == PEER_LINK_INPUT_ID => {
                    Some(value.trim().to_string())
                }
                _ => None,
            })
            .unwrap_or_default();
        if uri.is_empty() {
            return None;
        }
        Some(crate::ui::WorkflowEngine::handle_action(
            self,
            UserAction::LinkOpened { uri },
        ))
    }

    /// Start / drop the initiator exchange as navigation enters or leaves
    /// `AppScreen::LinkExchange`. Called from `navigate_to_internal`.
    pub(super) fn sync_link_initiator_lifecycle(&mut self, old: &AppScreen, new: &AppScreen) {
        let was = matches!(old, AppScreen::LinkExchange);
        let is = matches!(new, AppScreen::LinkExchange);
        match (was, is) {
            (true, false) => {
                #[cfg(all(feature = "network-http", feature = "storage"))]
                {
                    self.link_initiator = None;
                }
            }
            (false, true) => self.build_link_initiator(),
            _ => {}
        }
    }

    /// Start the exchange and push its share link into the renderer.
    /// Without networking there is no relay to run it, so only the link is
    /// shown. No-op without an identity (rare, non-fatal here).
    fn build_link_initiator(&mut self) {
        #[cfg(all(feature = "network-http", feature = "storage"))]
        let share_url = match self.vauchi.start_link_exchange() {
            Ok((url, exchange)) => {
                self.link_initiator = Some(exchange);
                url
            }
            Err(_) => return,
        };
        #[cfg(not(all(feature = "network-http", feature = "storage")))]
        let share_url = {
            let display_name = self
                .vauchi
                .identity()
                .map(|identity| identity.display_name().to_string())
                .unwrap_or_default();
            vauchi_core::exchange::link_mode::initiator_generate_named(&display_name)
                .0
                .url
        };
        let _ = self
            .engine
            .apply_update(crate::ui::EngineUpdate::LinkExchange(
                crate::ui::LinkExchangeUpdate::ShareUrl(share_url),
            ));
        self.reschedule_wakeup_for_new_session();
    }

    /// Hand a relay hardware event to the initiator exchange and reconcile
    /// the screen. A no-op off the LinkExchange screen, for unrelated
    /// events, or with no live exchange, so the caller can pass every
    /// hardware event through. Core runs any follow-up relay step itself.
    pub(super) fn route_link_initiator_hardware_event(
        &mut self,
        event: &Event,
    ) -> Option<ActionResult> {
        if !matches!(self.screen, AppScreen::LinkExchange)
            || !matches!(
                event,
                Event::LinkShared
                    | Event::LinkOpened { .. }
                    | Event::RelayEscrowReady { .. }
                    | Event::RelayEscrowFailed { .. }
                    | Event::RelayEscrowBlobReceived { .. }
            )
        {
            return None;
        }
        #[cfg(all(feature = "network-http", feature = "storage"))]
        {
            let mut exchange = self.link_initiator.take()?;
            let was_retrieving = exchange.is_retrieving();
            let progress = self
                .vauchi
                .feed_link_exchange_event(&mut exchange, event.clone())
                .ok()?;
            self.present_link_initiator(exchange, was_retrieving, progress);
        }
        None
    }

    /// Advance the initiator exchange one relay step (ADR-049). Called each
    /// `poll_notifications` tick; a no-op off the `LinkExchange` screen.
    /// Returns `true` if the exchange advanced past waiting.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    pub(crate) fn advance_link_initiator_session(&mut self) -> bool {
        if !matches!(self.screen, AppScreen::LinkExchange) {
            return false;
        }
        let Some(mut exchange) = self.link_initiator.take() else {
            return false;
        };
        let was_retrieving = exchange.is_retrieving();
        let progress = match self.vauchi.advance_link_exchange(&mut exchange) {
            Ok(progress) => progress,
            Err(_) => {
                self.link_initiator = Some(exchange);
                return false;
            }
        };
        self.present_link_initiator(exchange, was_retrieving, progress)
    }

    /// Map progress to the renderer: success and failure end the exchange;
    /// entering "retrieving" is shown once. Returns `true` on a change.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    fn present_link_initiator(
        &mut self,
        exchange: vauchi_core::api::LinkExchange,
        was_retrieving: bool,
        progress: vauchi_core::api::LinkExchangeProgress,
    ) -> bool {
        use crate::ui::{EngineUpdate, LinkExchangeUpdate};
        use vauchi_core::api::LinkExchangeProgress;
        match progress {
            LinkExchangeProgress::Completed { contact_id } => {
                // Link mode assigns no group, so group_names is empty.
                let summary = self.build_exchange_summary(&contact_id, Vec::new());
                let _ = self.engine.apply_update(EngineUpdate::LinkExchange(
                    LinkExchangeUpdate::Succeeded(summary),
                ));
                // Ceremony (M2 S4): validated success, once per link session.
                self.extend_pending_commands(vec![
                    crate::ui::exchange::ceremony::exchange_celebrate(),
                ]);
                true
            }
            LinkExchangeProgress::Failed { reason } => {
                let _ = self.engine.apply_update(EngineUpdate::LinkExchange(
                    LinkExchangeUpdate::Failed(reason),
                ));
                true
            }
            LinkExchangeProgress::Waiting => {
                let entered_retrieving = !was_retrieving && exchange.is_retrieving();
                if entered_retrieving {
                    let _ = self
                        .engine
                        .apply_update(EngineUpdate::LinkExchange(LinkExchangeUpdate::Retrieving));
                }
                self.link_initiator = Some(exchange);
                entered_retrieving
            }
        }
    }
}
