// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Saving the finalized peer: what the AppEngine reads and tells the machine
//! around its save (vauchi/private#1217). A child module because
//! `multi_stage_machine.rs` is at the file-size limit (VRS04); loaded via
//! `#[path]` from the parent.

use vauchi_core::exchange::PeerPayload;

use super::MultiStageMachine;

impl MultiStageMachine {
    /// The peer's verified identity and card, once the session finalized:
    /// what the AppEngine saves as the exchanged contact.
    pub fn received_peer(&self) -> Option<&PeerPayload> {
        self.inner.received_peer()
    }

    /// The contact is saved: from now on this phone shows DONE.
    pub fn mark_saved(&mut self) {
        self.inner.mark_saved();
    }

    /// The contact could not be saved: the exchange fails here, and no
    /// DONE tells the peer it was saved.
    pub fn save_failed(&mut self) {
        self.inner.save_failed();
        self.sync_phase_from_inner_state();
    }

    /// Whether the peer's DONE was read and its tag verified; a BOTH, which
    /// is unkeyed, does not count. Gates a restored contact's rekey.
    pub fn peer_saved(&self) -> bool {
        self.inner.peer_saved()
    }
}
