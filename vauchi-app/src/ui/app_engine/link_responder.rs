// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link-mode responder bridge methods on `AppEngine` (slice 32l Phase 2).
//!
//! The engine-owned `LinkResponder` state machine (in `vauchi-platform`)
//! drives these on its terminal transitions: `link_responder_completed`
//! on `Finalized` (after core persists the received card via
//! `import_received_link_card`), `link_responder_failed` on `Failed`.
//! Both return `None` when the engine is not on
//! `AppScreen::DeepLinkResponder`, so the platform layer can ignore
//! stale transitions after navigation.
//!
//! Design: `_private/docs/designs/2026-05-25-slice-32l-phase-2-responder-screen-driven-design.md`.

use super::{AppEngine, AppScreen};
use crate::ui::LinkResponderEngine;
use crate::ui::ScreenModel;
use crate::ui::WorkflowEngine;

impl AppEngine {
    /// Terminal success — the sender's card was retrieved and persisted.
    /// Transitions the responder engine to `link_responder_completed`.
    pub fn link_responder_completed(&mut self) -> Option<ScreenModel> {
        let engine = self.link_responder_engine_mut()?;
        engine.transition_to_completed();
        Some(engine.current_screen())
    }

    /// Terminal failure. `reason` is the stable `LinkResponder` failure
    /// id. Transitions the responder engine to `link_responder_failed`.
    pub fn link_responder_failed(&mut self, reason: String) -> Option<ScreenModel> {
        let engine = self.link_responder_engine_mut()?;
        engine.transition_to_failed(reason);
        Some(engine.current_screen())
    }

    fn link_responder_engine_mut(&mut self) -> Option<&mut LinkResponderEngine> {
        if !matches!(self.screen, AppScreen::DeepLinkResponder { .. }) {
            return None;
        }
        self.engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<LinkResponderEngine>())
    }
}
