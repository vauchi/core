// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The link trainer's layout carried from one exchange to the next on this
//! device (#450, plan 2.9). A child module because `multi_stage_machine.rs`
//! is at the file-size limit (VRS04); loaded via `#[path]` from the parent.

use super::MultiStageMachine;

impl MultiStageMachine {
    /// Start the first layout sweep at `layout`, the one the previous
    /// exchange on this device settled on.
    #[must_use]
    pub fn with_start_layout(mut self, layout: u8) -> Self {
        self.inner = self.inner.with_start_layout(layout);
        self
    }

    /// The layout the peer last reported reading, if any.
    pub fn last_good_layout(&self) -> Option<u8> {
        self.inner.last_good_layout()
    }
}
