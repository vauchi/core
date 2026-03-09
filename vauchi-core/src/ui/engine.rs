// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{ActionResult, ScreenModel, UserAction};

/// Trait that all core-driven workflows implement.
///
/// Core describes screens via `current_screen()`. Frontends render them.
/// User interactions flow back via `handle_action()`.
pub trait WorkflowEngine {
    /// Returns the current screen to render.
    fn current_screen(&self) -> ScreenModel;

    /// Handles a user action and returns the result.
    fn handle_action(&mut self, action: UserAction) -> ActionResult;

    /// Returns sensitive input collected by this engine (e.g. a PIN).
    ///
    /// Used by `AppEngine` to extract credentials before processing
    /// `ActionResult::Complete`. Default returns `None`.
    fn collected_input(&self) -> Option<String> {
        None
    }
}
