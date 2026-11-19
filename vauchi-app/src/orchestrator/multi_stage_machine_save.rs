// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Saving the finalized peer: what the AppEngine reads and tells the machine
//! around its save (vauchi/private#1217). A child module because
//! `multi_stage_machine.rs` is at the file-size limit (VRS04); loaded via
//! `#[path]` from the parent.

use vauchi_core::exchange::PeerPayload;
use vauchi_core::exchange::{Ending, Holding};

use super::MultiStageMachine;

impl MultiStageMachine {
    /// The peer's verified identity and card, once the session finalized:
    /// what the AppEngine saves as the exchanged contact.
    pub fn received_peer(&self) -> Option<&PeerPayload> {
        self.inner.received_peer()
    }

    /// The identity key of a peer whose payload verified and for which
    /// this phone has not yet said whether it holds a contact.
    pub fn peer_awaiting_statement(&self) -> Option<[u8; 32]> {
        self.inner.peer_awaiting_statement()
    }

    /// Whether this phone holds the verified peer as a contact: the
    /// statement its confirm carries (vauchi/private#1285).
    pub fn state_holding(&mut self, holding: Holding) {
        self.inner.state_holding(holding);
    }

    /// What this phone stated it holds for the peer, once it did.
    pub fn own_holding(&self) -> Option<Holding> {
        self.inner.own_holding()
    }

    /// What the peer's verified confirm or done said it holds for us.
    pub fn peer_holding(&self) -> Option<Holding> {
        self.inner.peer_holding()
    }

    /// How the exchange ended on the peer's phone, from its verified DONE.
    pub fn peer_ending(&self) -> Option<Ending> {
        self.inner.peer_ending()
    }

    /// The contact is saved: from now on this phone shows DONE.
    pub fn mark_saved(&mut self) {
        self.inner.mark_saved();
    }

    /// The contact this phone held took the exchange's key after its
    /// person agreed: DONE says "reconnected".
    pub fn mark_reconnected(&mut self) {
        self.inner.mark_reconnected();
    }

    /// Nothing new was saved and the old connection stays: DONE says
    /// "kept".
    pub fn mark_kept(&mut self) {
        self.inner.mark_kept();
    }

    /// The contact could not be saved: the exchange fails here, and no
    /// DONE tells the peer it was saved.
    pub fn save_failed(&mut self) {
        self.inner.save_failed();
        self.sync_phase_from_inner_state();
    }

    /// Whether the peer's DONE tag verified in DONE or BOTH.
    /// Gates a restored contact's rekey.
    pub fn peer_saved(&self) -> bool {
        self.inner.peer_saved()
    }
}
