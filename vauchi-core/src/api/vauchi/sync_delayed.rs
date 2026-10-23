// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A sync that waits for its own delayed answers (vauchi/private#628,
//! KU-F7). Pending answers live in memory only (design D-F4), so a process
//! that runs one sync and exits, like the CLI, would never collect what it
//! posted; it waits in the call instead.

use std::time::Duration;

use super::{Vauchi, VauchiSyncOutcome};
use crate::api::error::VauchiResult;
use crate::network::DelayWindowAdvert;

/// Shortest pause between collects, so a zero interval cannot spin.
const MIN_POLL: Duration = Duration::from_millis(100);

impl Vauchi {
    /// Syncs, then, when fetches go through the delayed path, keeps syncing
    /// every `poll` until the outer relay's delay window has passed and a fetch answer has been collected, or `max_wait` of pauses
    /// has passed. A lost answer is posted again and waited for. Counts are
    /// summed over the cycles. Without delayed mode this is [`Self::sync`].
    ///
    /// Stopping at the first answer would end the last connection within one
    /// poll of the forward the gateway sees, which links the client's IP to
    /// its token (#628 step 2); after the window, the stop says nothing more
    /// than the post did.
    pub fn sync_collecting_delayed(
        &mut self,
        poll: Duration,
        max_wait: Duration,
    ) -> VauchiResult<VauchiSyncOutcome> {
        let answered_before = self.answered_delayed_fetches();
        let mut outcome = self.sync()?;
        let Some(window) = self.delayed_window() else {
            return Ok(outcome);
        };
        if !matches!(outcome, VauchiSyncOutcome::Ok { .. }) {
            return Ok(outcome);
        }
        let poll = poll.max(MIN_POLL);
        let mut waited = Duration::ZERO;
        while (waited < window || self.answered_delayed_fetches() == answered_before)
            && waited < max_wait
        {
            self.sleeper.sleep(poll);
            waited += poll;
            let next = self.sync_inner()?;
            self.update_timing_after_sync();
            outcome = summed(outcome, next);
        }
        Ok(outcome)
    }

    /// The window of the delayed path when fetches take it: the one the
    /// outer relay advertised (D-F6 amendment 2026-10-16), else the
    /// configured one when `RelayConfig::ohttp_delayed` forces the path on.
    pub(super) fn delayed_window(&self) -> Option<Duration> {
        let advertised = self
            .storage
            .ohttp_cache()
            .load_delay_window(&self.config.relay.ohttp_endpoint())
            .ok()
            .flatten();
        advertised.or_else(|| {
            self.config
                .relay
                .ohttp_delayed
                .then_some(self.config.relay.ohttp_delay_window)
        })
    }

    /// Keeps or forgets the outer relay's window as its last answer said.
    pub(super) fn keep_delay_window_advert(&self, advert: Option<DelayWindowAdvert>) {
        let relay = self.config.relay.ohttp_endpoint();
        let cache = self.storage.ohttp_cache();
        // Best-effort: a failed write only defers the switch to a later sync.
        #[allow(clippy::let_underscore_must_use)]
        let _ = match advert {
            Some(DelayWindowAdvert::Advertised(window)) => cache.save_delay_window(&relay, window),
            Some(DelayWindowAdvert::Withdrawn) => cache.clear_delay_window(&relay),
            None => Ok(()),
        };
    }

    pub(super) fn answered_delayed_fetches(&self) -> u64 {
        self.delayed_ohttp
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .answered_fetches()
    }
}

fn summed(first: VauchiSyncOutcome, next: VauchiSyncOutcome) -> VauchiSyncOutcome {
    match (first, next) {
        (
            VauchiSyncOutcome::Ok {
                received,
                fetched,
                rejected,
                unresolved,
                reject_reasons,
                sent,
                acknowledged,
                mut errors,
                version_policy,
                mut aha_moments,
            },
            VauchiSyncOutcome::Ok {
                received: next_received,
                fetched: next_fetched,
                rejected: next_rejected,
                unresolved: next_unresolved,
                reject_reasons: next_reasons,
                sent: next_sent,
                acknowledged: next_acknowledged,
                errors: next_errors,
                version_policy: next_policy,
                aha_moments: next_aha,
            },
        ) => {
            errors.extend(next_errors);
            aha_moments.extend(next_aha);
            VauchiSyncOutcome::Ok {
                received: received + next_received,
                fetched: fetched + next_fetched,
                rejected: rejected + next_rejected,
                unresolved: unresolved + next_unresolved,
                reject_reasons: [reject_reasons, next_reasons]
                    .into_iter()
                    .filter(|reasons| !reasons.is_empty())
                    .collect::<Vec<_>>()
                    .join(","),
                sent: sent + next_sent,
                acknowledged: acknowledged + next_acknowledged,
                errors,
                version_policy: next_policy.or(version_policy),
                aha_moments,
            }
        }
        (_, next) => next,
    }
}
