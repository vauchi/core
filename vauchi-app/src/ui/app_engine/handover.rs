// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! What an engine that replaces another must carry over so a shell keeps
//! talking to it seamlessly — after a shred the platform reopens on a fresh
//! engine (vauchi/private#599, #293).

use super::AppEngine;

impl AppEngine {
    /// Surfaces this engine has shown that a shell may still report on.
    pub fn shown_surfaces(&self) -> Vec<vauchi_core::SurfaceId> {
        self.presentation_coordinator.shown_surfaces()
    }

    /// Treats `surfaces`, shown by an engine this one replaces, as left
    /// behind (see `PresentationCoordinator::remember_left_behind`).
    pub fn inherit_shown_surfaces(&mut self, surfaces: Vec<vauchi_core::SurfaceId>) {
        self.presentation_coordinator.remember_left_behind(surfaces);
    }

    /// The revision of the last surface this engine sent.
    pub fn surface_revision(&self) -> u64 {
        self.surface_revision
    }

    /// Counts surface revisions on from `revision`, sent by an engine this
    /// one replaces: a shell drops an envelope older than the one it shows,
    /// so a replacement starting at 1 again would not be shown
    /// (vauchi/private#293).
    pub fn continue_revisions_after(&mut self, revision: u64) {
        self.surface_revision = self.surface_revision.max(revision.saturating_add(1));
    }
}
