// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-owned application reducer entry points.

use super::{AppEngine, AppScreen};
use crate::ui::{DeviceReplacementEngine, ReplacementRole};
use vauchi_core::api::Vauchi;

impl AppEngine {
    /// Create the reducer at a device-replacement entry point.
    ///
    /// The selector is construction data, not a presentation boundary:
    /// consumers still interact exclusively through `initial_commands` and
    /// `dispatch`.
    pub fn for_device_replacement(vauchi: Vauchi, role: ReplacementRole) -> Self {
        let mut app = Self::new(vauchi);
        let replacement = match role {
            ReplacementRole::Source => DeviceReplacementEngine::new_source(),
            ReplacementRole::Target => DeviceReplacementEngine::new_target(),
            ReplacementRole::PostRestore => DeviceReplacementEngine::new_post_restore(),
        }
        .with_locale(app.render_context.resolved_locale());

        app.screen = AppScreen::DeviceReplacement;
        app.engine = Box::new(replacement);
        app.engine_cache.clear();
        app.nav_history.clear();
        app.pending_commands.clear();
        app.contextual_actions.clear();
        app.presentation_coordinator.set_primary_surface(
            vauchi_core::SurfaceId::new(app.screen.screen_id())
                .expect("AppScreen identifiers are valid presentation identifiers"),
        );
        app
    }
}
