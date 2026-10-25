// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! OS-notification pipeline for `AppEngine`: draining buffered events
//! into the activity log and evaluating log entries into
//! `PendingNotification`s. Split out of `mod.rs` (file-size, VRS04).

use super::AppEngine;
use crate::activity_log_writer::{ActivityLogWriter, RETENTION_SECS};
use crate::notification_emitter::NotificationEmitter;
use crate::notification_types::{ActivityLogEntry, NotificationPreferences, PendingNotification};

impl AppEngine {
    /// Drain pending OS notifications.
    ///
    /// Persists buffered events and claims pending activity rows through the
    /// same preference-aware collector as polling. Claims survive reopening;
    /// each retained activity row is handed off at most once, not acknowledged
    /// as displayed by the OS.
    ///
    /// Frontends should call this after receiving an event callback.
    pub fn drain_pending_notifications(&mut self) -> Vec<PendingNotification> {
        self.collect_notifications()
    }

    /// Poll the activity log and produce pending OS notifications.
    /// Public so PlatformAppEngine can expose it via UniFFI.
    pub fn poll_notifications(&mut self) -> Vec<PendingNotification> {
        // Dev instrumentation (dev-logging only; no PII — a count and a
        // gap). This is the one route into `advance_multi_stage_session`, so
        // whatever calls it sets the pace at which a Hover QR advances. On
        // device the display moved every 2-4 s while core asked for ~300 ms,
        // and the foreground heartbeat that was supposed to drive it turned
        // out never to run — so the real driver is unidentified, and only a
        // counter here is agnostic about who it is
        // (`2026-08-18-hover-transfer-stalls-on-the-last-chunk`).
        if self.multi_stage_session_active() {
            let now_ms = self.vauchi.clock().unix_millis();
            let gap = self.last_poll_ms.map(|t| now_ms.saturating_sub(t));
            self.last_poll_ms = Some(now_ms);
            self.poll_count = self.poll_count.saturating_add(1);
            tracing::info!(
                "[MSX] poll n={} gap={}ms",
                self.poll_count,
                gap.map_or(-1i64, |g| g as i64)
            );
        }
        self.advance_relay_sessions();
        self.collect_notifications()
    }

    fn collect_notifications(&mut self) -> Vec<PendingNotification> {
        self.drain_events_to_log();
        let store = self.vauchi.storage().activity_log();
        let rows = match store
            .activity_log_prune(self.vauchi.clock().unix_seconds(), RETENTION_SECS)
            .and_then(|_| store.claim_notifications())
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("collect_notifications: activity claim failed: {e}");
                return Vec::new();
            }
        };

        let entries: Vec<_> = rows
            .into_iter()
            .filter_map(|row| {
                let entry = serde_json::from_str::<ActivityLogEntry>(&row.payload).ok()?;
                Some((row.event_key, entry))
            })
            .collect();
        let entries = self.without_ignored_card_updates(entries);

        if entries.is_empty() {
            return Vec::new();
        }

        let prefs = NotificationPreferences {
            contact_added_enabled: self.vauchi.config().contact_added_notifications,
            card_update_enabled: self.vauchi.config().card_update_notifications,
        };
        let locale = self.render_context.resolved_locale();

        let name_resolver = |contact_id: &str| {
            self.vauchi
                .get_contact(contact_id)
                .ok()
                .flatten()
                .map(|c| c.display_name().to_string())
                .unwrap_or_else(|| {
                    format!("Contact {}", contact_id.chars().take(8).collect::<String>())
                })
        };

        NotificationEmitter::evaluate(&entries, &prefs, locale, name_resolver)
    }

    /// Drain the event receiver and write all events to the activity log.
    ///
    /// Returns newly inserted `(event_key, ActivityLogEntry)` pairs.
    /// Called before operations that read from the activity log (notifications)
    /// or when data mutations may have occurred (user actions).
    pub(super) fn drain_events_to_log(&mut self) -> Vec<(String, ActivityLogEntry)> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }

        if events.is_empty() {
            return Vec::new();
        }

        let now = self.vauchi.clock().unix_seconds();

        match ActivityLogWriter::write(self.vauchi.storage(), &events, now) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!("drain_events_to_log: ActivityLogWriter::write failed: {e}");
                Vec::new()
            }
        }
    }

    /// Drops card-update entries from ignored contacts before notification
    /// evaluation (ADR-072): the update is applied and logged, but never
    /// surfaces as an OS notification. Safety alerts are left untouched.
    fn without_ignored_card_updates(
        &self,
        entries: Vec<(String, ActivityLogEntry)>,
    ) -> Vec<(String, ActivityLogEntry)> {
        entries
            .into_iter()
            .filter(|(_, entry)| {
                let ActivityLogEntry::CardUpdateReceived { contact_id, .. } = entry else {
                    return true;
                };
                !self
                    .vauchi
                    .get_contact(contact_id)
                    .ok()
                    .flatten()
                    .is_some_and(|c| c.is_ignored())
            })
            .collect()
    }
}
