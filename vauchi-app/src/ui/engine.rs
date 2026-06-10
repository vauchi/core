// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::any::Any;

use super::{ActionResult, EngineOutput, ScreenModel, UserAction};
use crate::notification_types::PendingNotification;
use vauchi_core::{Command, Event};

/// Trait that all core-driven workflows implement.
///
/// Core describes screens via `current_screen()`. Frontends render them.
/// User interactions flow back via `handle_action()`.
pub trait WorkflowEngine: Send {
    /// Returns the current screen to render.
    fn current_screen(&self) -> ScreenModel;

    /// Handles a user action and returns the result.
    fn handle_action(&mut self, action: UserAction) -> ActionResult;

    /// Poll for any new OS notifications.
    // @scenario: accessibility.feature:Notifications are announced
    fn poll_notifications(&mut self) -> Vec<PendingNotification> {
        Vec::new()
    }

    /// Salient typed state this engine exposes to `AppEngine` at
    /// completion/interception time (see [`EngineOutput`]).
    ///
    /// The typed replacement for downcasting via [`Self::as_any`]:
    /// hub sites match the expected variant and `tracing::warn!` +
    /// degrade on a mismatch (foreign engine active when a stale
    /// async result lands). Default `None` — engine has nothing the
    /// hub reads.
    fn engine_output(&self) -> Option<EngineOutput> {
        None
    }

    /// Returns `true` if the last `Complete` was triggered by a cancel action
    /// rather than a submit. `AppEngine::handle_completion` skips persistence
    /// for cancelled workflows.
    fn was_cancelled(&self) -> bool {
        false
    }

    /// Whether this engine can rewind one *internal* step right now.
    ///
    /// Some engines host a multi-step flow under a single `AppScreen`
    /// (e.g. the exchange flow: mode selection → group selection →
    /// field preview → sub-flow). Those step transitions never touch the
    /// AppScreen `nav_history`, so without this hook a BACK press jumps
    /// straight out of the whole flow — or, at an `is_root` screen like
    /// `Exchange`, does nothing (the back-trap this hook fixes).
    ///
    /// When `true`, `AppEngine::can_go_back` reports BACK as available
    /// even at an AppScreen root, and `AppEngine::navigate_back` routes
    /// the press to [`Self::navigate_back_within`] first. Default `false`.
    fn can_navigate_back_within(&self) -> bool {
        false
    }

    /// Rewind exactly one internal step.
    ///
    /// Returns `true` if a step was consumed — the caller re-renders the
    /// *same* engine via `current_screen()`. Returns `false` if the
    /// engine is at its root step, in which case the caller falls through
    /// to popping the AppScreen `nav_history`. Default `false`.
    ///
    /// Implementations must only rewind *back-safe* steps (selection /
    /// pre-handshake) — never mid-protocol or terminal steps, where
    /// rewinding would corrupt live cryptographic state.
    fn navigate_back_within(&mut self) -> bool {
        false
    }

    /// Signal that async/background processing completed successfully.
    ///
    /// Used by backup and other engines that have an intermediate Processing
    /// state. Default is a no-op for engines without a Processing step.
    fn processing_complete(&mut self) {}

    /// Signal that async/background processing failed.
    fn processing_failed(&mut self) {}

    /// Handle a hardware event from the frontend (ADR-031).
    ///
    /// Engines that interact with platform hardware (camera, BLE, NFC,
    /// audio) override this to process events. Default returns `None`
    /// (engine does not handle hardware events).
    fn handle_hardware_event(&mut self, _event: Event) -> Option<ActionResult> {
        None
    }

    /// Advance the animated QR display to the next frame.
    ///
    /// Engines that render animated QR codes (currently only `ExchangeEngine`
    /// on the ShowQr step) override this. Returns `Some(ScreenModel)` with the
    /// next-frame QR data, or `None` if no animation is active (static QR or
    /// non-QR screen). Default returns `None`.
    ///
    /// Frontends call this on a ~100ms timer while displaying the ShowQr screen
    /// to cycle the V6 frames for reliable 240p-camera decode.
    fn advance_qr_frame(&mut self) -> Option<ScreenModel> {
        None
    }

    /// Screen-presentation [`Command`]s emitted when this engine becomes
    /// the active one (ADR-031 §Hardware, Phase 2b of
    /// `2026-05-04-exchange-command-screen-presentation`).
    ///
    /// `AppEngine` calls this immediately after switching to the engine in
    /// `navigate_to_internal` / `navigate_back`. The returned commands
    /// reach the frontend via the same dispatch path as
    /// `ActionResult::Commands`. Engines whose screen is content with the
    /// platform default (no brightness override, normal idle behaviour,
    /// no orientation lock) inherit the empty default. Engines that need
    /// specific presentation (e.g. exchange flows that dim the screen
    /// and disable the idle timer) override this to declare the entry
    /// preferences.
    ///
    /// Symmetric counterpart: [`Self::screen_exited`] runs when the
    /// engine ceases to be active and typically restores defaults
    /// (`Command::SetScreenBrightness { level: None }`,
    /// `Command::SetIdleTimerDisabled { disabled: false }`).
    fn screen_entered(&mut self) -> Vec<Command> {
        Vec::new()
    }

    /// Screen-presentation [`Command`]s emitted when this engine stops
    /// being active. Pair with [`Self::screen_entered`].
    fn screen_exited(&mut self) -> Vec<Command> {
        Vec::new()
    }

    /// Downcast to concrete type for AppEngine-level interception.
    ///
    /// Used by `MyInfoEntryDetailEngine` for group visibility persistence.
    /// Prefer adding trait methods over new downcast sites.
    fn as_any(&self) -> Option<&dyn Any> {
        None
    }

    /// Downcast to concrete mutable type for AppEngine-level interception.
    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }
}
