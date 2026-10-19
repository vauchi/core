// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! When the app wakes and syncs: the heartbeat's next `ScheduleWakeup`, the
//! platform scheduler's sync tick, and the live-session checks that set the
//! cadence (ADR-044 Am2a, #603).
use super::AppEngine;
use crate::notification_types::PendingNotification;
use crate::ui::WorkflowEngine;

impl AppEngine {
    /// Keep the OS's allowance of background runs the shell reported, for
    /// the scheduler's next draws. Out of range is refused and changes
    /// nothing (it comes from the shell, CC-14); `runs` 0 clears it.
    pub(super) fn observe_background_budget(
        &mut self,
        window_secs: u32,
        runs: u32,
    ) -> Result<Vec<vauchi_core::Command>, super::presentation_protocol::AppPresentationError> {
        const MAX_WINDOW_SECS: u32 = 7 * 24 * 3600;
        const MAX_RUNS: u32 = 100_000;
        if !(1..=MAX_WINDOW_SECS).contains(&window_secs) || runs > MAX_RUNS {
            return Err(
                super::presentation_protocol::AppPresentationError::InvalidBackgroundBudget,
            );
        }
        self.background_budget = (runs > 0).then(|| vauchi_core::api::BackgroundBudget {
            window: std::time::Duration::from_secs(u64::from(window_secs)),
            runs,
        });
        Ok(Vec::new())
    }

    /// The platform scheduler's sync tick, and the `ScheduleWakeup` for its
    /// next run. In the background it draws the next deadline from the
    /// background schedule and wakes at it; an open app keeps its 60 s
    /// schedule, so a tick cannot stall its syncs, and the scheduler's next
    /// run still comes at a background gap rather than at the heartbeat's
    /// 30 s beat (#603 option 2).
    #[cfg(feature = "network-http")]
    pub fn scheduler_sync_tick(
        &mut self,
    ) -> vauchi_core::VauchiResult<(vauchi_core::VauchiSyncOutcome, vauchi_core::Command)> {
        let budget = self.background_budget;
        let (outcome, gap) = if self.in_foreground {
            let outcome = self.vauchi.periodic_sync_tick()?;
            (outcome, self.vauchi.draw_background_sync_gap(budget))
        } else {
            let outcome = self.vauchi.background_sync_tick(budget)?;
            let gap = self
                .vauchi
                .time_until_sync_due()
                .filter(|until| !until.is_zero())
                .unwrap_or_else(|| self.vauchi.draw_background_sync_gap(budget));
            (outcome, gap)
        };
        let millis = u32::try_from(gap.as_millis()).unwrap_or(u32::MAX).max(1);
        let secs = millis.div_ceil(1000);
        Ok((
            outcome,
            vauchi_core::Command::schedule_wakeup(secs, secs, 1, Some(millis)),
        ))
    }

    /// The shell's platform wakeup fired — a desktop in-process interval, an
    /// iOS `BGAppRefreshTask`, or an Android `WorkManager` task, scheduled
    /// from a prior [`vauchi_core::Command::ScheduleWakeup`]. Run all work now
    /// due (the same relay/exchange advance + activity-log poll as
    /// [`Self::poll_notifications`]) and emit the *next* `ScheduleWakeup` so
    /// the shell re-arms. Core owns *when* the heartbeat is due (ADR-044 Am2a
    /// Option C); the shell owns only the native wakeup mechanism.
    ///
    /// Returns the OS notifications to post; the next schedule rides
    /// `pending_commands` — the caller drains it via
    /// [`Self::drain_pending_commands`] after this returns. Elapsed-based and
    /// idempotent: a delayed, coalesced, or skipped wake is safe (the
    /// underlying advances/poll are no-ops when nothing is due). Public so
    /// `PlatformAppEngine` can expose it via UniFFI. The frontend bootstraps
    /// the loop by calling this once at launch.
    pub fn on_wakeup(&mut self) -> Vec<PendingNotification> {
        let before = self.current_screen();
        self.sync_if_due();
        let notifications = self.poll_notifications();
        if self.current_screen() != before {
            self.re_present_after_heartbeat();
        }
        self.pending_commands.push_back(self.compute_next_wakeup());
        notifications
    }

    /// Tell the shell the heartbeat cadence changed now, not at the end of
    /// its current sleep. `on_wakeup` is the only other place a
    /// `ScheduleWakeup` is emitted, and a shell sleeps until the previous one
    /// comes due: 30 s when idle. A session that goes live in between waited
    /// that sleep out before its first tick, with its QR frozen on the first
    /// frame (`2026-10-02-exchange-first-tick-waits-for-idle-heartbeat`).
    pub(super) fn reschedule_wakeup_for_new_session(&mut self) {
        self.pending_commands.push_back(self.compute_next_wakeup());
    }

    /// Compute the next app-heartbeat wakeup window (ADR-044 Am2a Option C).
    /// Core is the single authority on *when*; the shell only executes the
    /// resulting `ScheduleWakeup`. First cut: a fixed ~30 s foreground cadence
    /// (matching the historical notification poll). The elapsed-based due-math
    /// (wall `Clock` for calendar-relative token rotation, `MonotonicClock`
    /// for runtime-elapsed) refines the *values* without changing the command
    /// shape or the shell contract.
    pub(super) fn compute_next_wakeup(&self) -> vauchi_core::Command {
        // A live multi-stage exchange is driven entirely by this heartbeat:
        // `advance_multi_stage_session` runs inside `poll_notifications`, which
        // runs only on wakeup. At the idle cadence the QR advanced once every
        // 30 s while the protocol is built for a ~300 ms frame — device-observed
        // as one displayed frame against 110 camera decodes, 40 peer INITs
        // dropped before our own QR existed, and 43 s from "Exchange started" to
        // any state change
        // (`2026-08-18-hover-transfer-stalls-on-the-last-chunk`).
        //
        // One second is this command's floor, not the right cadence: the frame
        // dwell core already computes cannot be expressed in whole seconds.
        if self.multi_stage_session_active() {
            // The frame currently on screen names its own dwell, so the
            // display runs at the protocol's cadence rather than the
            // coarsest interval a whole-second field can express.
            let frame_dwell_ms = self.multi_stage_frame_ms().unwrap_or(300);
            return vauchi_core::Command::schedule_wakeup(1, 1, 1, Some(frame_dwell_ms));
        }
        // A Link rendezvous is several dependent relay round trips (presence
        // deposit, peer epk, card deposit, peer card), each waiting on one
        // side's heartbeat. At the idle cadence two terminals converged in
        // ~90 s; the escrow phase is meant to poll about once a second.
        if self.link_session_active() {
            return vauchi_core::Command::schedule_wakeup(1, 1, 1, None);
        }
        // In the background the scheduler wakes once, at the drawn gap, so
        // the wakeup names all of it (#603 option 2).
        #[cfg(feature = "network-http")]
        if !self.in_foreground
            && let Some(until) = self.vauchi.time_until_sync_due()
            && !until.is_zero()
            && self.has_someone_to_sync_with()
        {
            let millis = u32::try_from(until.as_millis()).unwrap_or(u32::MAX).max(1);
            let secs = millis.div_ceil(1000);
            return vauchi_core::Command::schedule_wakeup(secs, secs, 1, Some(millis));
        }
        // Wake when the drawn sync gap ends, not on the next 30 s beat: on
        // the beat, every gap the relay saw was a multiple of 30 s (KU-9,
        // #603), which undoes the exponential schedule.
        #[cfg(feature = "network-http")]
        if let Some(until) = self.vauchi.time_until_sync_due()
            && until < std::time::Duration::from_secs(30)
            && self.has_someone_to_sync_with()
        {
            let millis = u32::try_from(until.as_millis()).unwrap_or(u32::MAX).max(1);
            return vauchi_core::Command::schedule_wakeup(
                millis.div_ceil(1000),
                90,
                1,
                Some(millis),
            );
        }
        vauchi_core::Command::schedule_wakeup(30, 90, 30, None)
    }

    /// A Link-mode rendezvous (initiator on the share screen or responder on
    /// the deep-link screen) is still waiting on the relay. Terminal machines
    /// are dropped by their advance, so "held" means "live".
    pub fn link_session_active(&self) -> bool {
        #[cfg(all(feature = "network-http", feature = "storage"))]
        {
            [&self.link_initiator, &self.link_responder]
                .iter()
                .any(|exchange| exchange.as_ref().is_some_and(|e| e.is_live()))
        }
        #[cfg(not(all(feature = "network-http", feature = "storage")))]
        false
    }
}
