// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::any::Any;

use super::{ActionResult, ScreenModel, UserAction};
use crate::notification_types::PendingNotification;
use vauchi_core::exchange::ExchangeHardwareEvent;

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

    /// Handle a hardware event from the frontend (ADR-031).
    ///
    /// Engines that interact with platform hardware (camera, BLE, NFC,
    /// audio) override this to process events. Default returns `None`
    /// (engine does not handle hardware events).
    fn handle_hardware_event(&mut self, _event: ExchangeHardwareEvent) -> Option<ActionResult> {
        None
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
