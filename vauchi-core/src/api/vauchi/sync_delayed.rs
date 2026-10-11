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

/// Shortest pause between collects, so a zero interval cannot spin.
const MIN_POLL: Duration = Duration::from_millis(100);

impl Vauchi {
    /// Syncs, then, when fetches go through the delayed path
    /// (`RelayConfig::ohttp_delayed`), keeps syncing every `poll` until a
    /// fetch answer has been collected or `max_wait` of pauses has passed.
    /// A lost answer is posted again and waited for. Counts are summed over
    /// the cycles. Without delayed mode this is [`Self::sync`].
    pub fn sync_collecting_delayed(
        &mut self,
        poll: Duration,
        max_wait: Duration,
    ) -> VauchiResult<VauchiSyncOutcome> {
        let answered_before = self.answered_delayed_fetches();
        let mut outcome = self.sync()?;
        if !self.config.relay.ohttp_delayed || !matches!(outcome, VauchiSyncOutcome::Ok { .. }) {
            return Ok(outcome);
        }
        let poll = poll.max(MIN_POLL);
        let mut waited = Duration::ZERO;
        while self.answered_delayed_fetches() == answered_before && waited < max_wait {
            self.sleeper.sleep(poll);
            waited += poll;
            let next = self.sync_inner()?;
            self.update_timing_after_sync();
            outcome = summed(outcome, next);
        }
        Ok(outcome)
    }

    fn answered_delayed_fetches(&self) -> u64 {
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
