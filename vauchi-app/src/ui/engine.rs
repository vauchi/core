// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::any::Any;

use super::{ActionResult, ScreenModel, UserAction};
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

    /// Returns sensitive input collected by this engine (e.g. a PIN).
    ///
    /// Used by `AppEngine` to extract credentials before processing
    /// `ActionResult::Complete`. Default returns `None`.
    fn collected_input(&self) -> Option<String> {
        None
    }

    /// Returns `true` if the last `Complete` was triggered by a cancel action
    /// rather than a submit. `AppEngine::handle_completion` skips persistence
    /// for cancelled workflows.
    fn was_cancelled(&self) -> bool {
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
