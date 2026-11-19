// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A repeat Hover exchange whose two phones hold different things for
//! each other (vauchi/private#1285). The phone that still holds the
//! contact asks its person and takes the new key only on a yes; the
//! phone that holds nothing saves only once the other said it
//! reconnected. A child module of `multi_stage_exchange.rs` for its size
//! (VRS04); loaded via `#[path]`.

use vauchi_core::exchange::{Ending, Holding};

use super::*;
use crate::ui::{EngineOutput, EngineUpdate, MultiStageUpdate, RepeatEnding, RepeatExchange};

/// How long the question stays open, from the moment it is shown. At the
/// deadline both phones end as if the answer were no.
const RECONNECT_DEADLINE_MS: u64 = 120_000;

/// A finalized exchange that waits for a person, and since when.
#[derive(Clone, Copy)]
pub(super) enum RepeatWait {
    /// This phone holds the peer and its person is asked.
    Asking { since_ms: u64 },
    /// This phone holds nothing and the peer's person is asked.
    Waiting { since_ms: u64 },
}

impl AppEngine {
    /// Which side of the question this phone is on, from what the two
    /// phones stated. `None` when they agree: both save as before.
    pub(super) fn repeat_exchange_wait(&self) -> Option<RepeatWait> {
        let machine = &self.multi_stage_session.as_ref()?.machine;
        let since_ms = self.vauchi.clock().unix_millis();
        match (machine.own_holding()?, machine.peer_holding()?) {
            // A blocked contact is never asked about: the save refuses it
            // as before, and the exchange fails here (ADR-056).
            (Holding::Peer, Holding::Nothing) if self.verified_peer_is_blocked() => None,
            (Holding::Peer, Holding::Nothing) => Some(RepeatWait::Asking { since_ms }),
            (Holding::Nothing, Holding::Peer) => Some(RepeatWait::Waiting { since_ms }),
            (Holding::Peer, Holding::Peer) | (Holding::Nothing, Holding::Nothing) => None,
        }
    }

    fn verified_peer_is_blocked(&self) -> bool {
        self.multi_stage_session
            .as_ref()
            .and_then(|h| h.machine.received_peer())
            .and_then(|peer| {
                self.vauchi
                    .get_contact(&hex::encode(peer.identity_pk))
                    .ok()
                    .flatten()
            })
            .is_some_and(|contact| contact.is_blocked())
    }

    /// Finalized, and nothing is saved yet: show the question or the wait
    /// and keep the code on screen.
    pub(super) fn begin_repeat_exchange(&mut self, wait: RepeatWait, peer_name: String) -> bool {
        if let Some(holder) = self.multi_stage_session.as_mut() {
            holder.repeat = Some(wait);
        }
        self.show_final_frame();
        let shown = match wait {
            RepeatWait::Asking { .. } => RepeatExchange::Asking,
            RepeatWait::Waiting { .. } => RepeatExchange::Waiting,
        };
        let state_applied =
            self.apply_multi_stage_state(vauchi_core::exchange::ProtocolState::Finalized);
        if !peer_name.is_empty() {
            let _ = self.apply_multi_stage_finalized(peer_name);
        }
        self.apply_multi_stage_repeat(Some(shown)) | state_applied
    }

    /// Acts on the person's answer, the peer's ending or the deadline,
    /// whichever came. Called on every tick and scan; `true` when the
    /// screen changed.
    pub(super) fn resolve_repeat_exchange(&mut self) -> bool {
        let Some(wait) = self.multi_stage_session.as_ref().and_then(|h| h.repeat) else {
            return false;
        };
        let now = self.vauchi.clock().unix_millis();
        let past = |since_ms: u64| now.saturating_sub(since_ms) >= RECONNECT_DEADLINE_MS;
        match wait {
            RepeatWait::Asking { since_ms } => match (self.reconnect_answer(), past(since_ms)) {
                (Some(true), _) => self.reconnect_after_a_yes(),
                (Some(false), _) => self.keep_after_a_no(RepeatEnding::KeptByYou),
                (None, true) => self.keep_after_a_no(RepeatEnding::TimedOut),
                (None, false) => false,
            },
            RepeatWait::Waiting { since_ms } => {
                let peer_ending = self
                    .multi_stage_session
                    .as_ref()
                    .and_then(|h| h.machine.peer_ending());
                match (peer_ending, past(since_ms)) {
                    (Some(Ending::Reconnected), _) => self.save_after_the_peers_yes(),
                    // `Saved` from a phone that stated it holds us: it kept
                    // its key without asking, so a new key here would not
                    // match it.
                    (Some(Ending::Kept(_) | Ending::Saved(_)), _) => {
                        self.end_repeat_with_nothing_new(RepeatEnding::KeptByThem)
                    }
                    (None, true) => self.end_repeat_with_nothing_new(RepeatEnding::TimedOut),
                    (None, false) => false,
                }
            }
        }
    }

    fn reconnect_answer(&self) -> Option<bool> {
        match self.engine.engine_output() {
            Some(EngineOutput::MultiStageExchange {
                reconnect_answer, ..
            }) => reconnect_answer,
            _ => None,
        }
    }

    fn apply_multi_stage_repeat(&mut self, repeat: Option<RepeatExchange>) -> bool {
        self.engine
            .apply_update(EngineUpdate::MultiStage(MultiStageUpdate::Repeat(repeat)))
    }

    fn stop_waiting(&mut self) {
        if let Some(holder) = self.multi_stage_session.as_mut() {
            holder.repeat = None;
        }
    }

    /// Puts the session's current final frame on screen at once, so the
    /// peer reads this phone's confirm or DONE and not a stale data frame.
    fn show_final_frame(&mut self) {
        if let Some(final_frame) = self
            .multi_stage_session
            .as_ref()
            .map(|h| h.machine.finalization_qr())
        {
            let _ = self.apply_multi_stage_qr_payload(&final_frame);
        }
    }

    fn fail_the_save(&mut self) -> bool {
        self.stop_waiting();
        if let Some(holder) = self.multi_stage_session.as_mut() {
            holder.machine.save_failed();
        }
        false
    }

    /// The person said yes: the contact this phone holds takes the
    /// exchange's key, and the peer is told it may save.
    fn reconnect_after_a_yes(&mut self) -> bool {
        let Some(ExchangeMaterial {
            contact,
            ratchet: Some((ratchet, is_initiator)),
            saved,
        }) = self.finalized_exchange_material()
        else {
            return self.fail_the_save();
        };
        if let Err(e) = self
            .vauchi
            .reconnect_exchanged_contact(&contact, &ratchet, is_initiator)
        {
            tracing::warn!("multi-stage: failed to reconnect the contact: {e}");
            return self.fail_the_save();
        }
        self.stop_waiting();
        if let Some(holder) = self.multi_stage_session.as_mut() {
            holder.machine.mark_reconnected();
        }
        self.exchange_was_reconnection = true;
        self.show_saved_exchange(saved)
    }

    /// The person said no, or did not answer: the card is refreshed, the
    /// key stays, and the peer is told to save nothing.
    fn keep_after_a_no(&mut self, ending: RepeatEnding) -> bool {
        let Some(ExchangeMaterial { contact, .. }) = self.finalized_exchange_material() else {
            return self.fail_the_save();
        };
        if let Err(e) = self.vauchi.refresh_exchanged_card(&contact) {
            tracing::warn!("multi-stage: failed to refresh the kept contact's card: {e}");
            return self.fail_the_save();
        }
        self.end_repeat_with_nothing_new(ending)
    }

    /// The peer said it reconnected: this phone saves the new contact
    /// with the exchange's key, as for a first meeting.
    fn save_after_the_peers_yes(&mut self) -> bool {
        let Some(saved) = self.persist_exchanged_contact() else {
            return self.fail_the_save();
        };
        self.stop_waiting();
        if let Some(holder) = self.multi_stage_session.as_mut() {
            holder.machine.mark_saved();
        }
        self.show_saved_exchange(saved)
    }

    fn show_saved_exchange(&mut self, saved: SavedExchange) -> bool {
        self.show_final_frame();
        let cleared = self.apply_multi_stage_repeat(None);
        self.finish_exchanged_contact(saved);
        cleared
    }

    fn end_repeat_with_nothing_new(&mut self, ending: RepeatEnding) -> bool {
        self.stop_waiting();
        if let Some(holder) = self.multi_stage_session.as_mut() {
            holder.machine.mark_kept();
        }
        self.show_final_frame();
        self.apply_multi_stage_repeat(Some(RepeatExchange::Ended(ending)))
    }
}
