// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! When the surface's revision moves: only when the ids already handed to
//! shells stop meaning what they meant.

use vauchi_core::SurfaceId;

use super::AppEngine;
use super::presentation_protocol::AppPresentationError;
use crate::ui::{PreparedSurface, WorkflowEngine};

impl AppEngine {
    pub(super) fn projected_visible_surface(&self) -> Option<(SurfaceId, PreparedSurface)> {
        let surface_id = SurfaceId::new(self.screen.screen_id()).ok()?;
        let screen = self.current_screen();
        let prepared = PreparedSurface::from_screen_in(
            surface_id.clone(),
            self.surface_revision,
            &screen,
            &self.render_context,
        )
        .ok()?;
        Some((surface_id, prepared))
    }

    /// Whether the ids already handed to shells still mean what they meant.
    ///
    /// Shells key their composition state on these ids, so reminting them for
    /// a content-only change rebuilds live widgets: on Android that drops
    /// focus and closes the soft keyboard between keystrokes. The revision
    /// therefore tracks binding topology, not content.
    fn preserves_binding_meaning(&self, before: Option<&(SurfaceId, PreparedSurface)>) -> bool {
        let Some((before_id, before_surface)) = before else {
            return false;
        };
        let Some((after_id, after_surface)) = self.projected_visible_surface() else {
            return false;
        };
        *before_id == after_id && before_surface.routes_match(&after_surface)
    }

    /// A late event for ids the shell no longer shows is then stale and
    /// dropped, not refused as unknown (vauchi/private#438, #1339).
    pub(super) fn renew_revision_unless_bindings_kept(
        &mut self,
        before: Option<&(SurfaceId, PreparedSurface)>,
    ) -> Result<(), AppPresentationError> {
        if !self.preserves_binding_meaning(before) {
            self.surface_revision = self
                .surface_revision
                .checked_add(1)
                .ok_or(AppPresentationError::RevisionExhausted)?;
        }
        Ok(())
    }
}
