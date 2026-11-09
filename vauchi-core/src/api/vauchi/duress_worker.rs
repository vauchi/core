// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Under duress the real profile syncs as a background worker: its own
//! connection, identity and event dispatcher, which nothing in the UI holds
//! (ADR-080 points 2 and 5). It runs in duress mode, so it syncs exactly as a
//! duress session does today.

use std::sync::{Arc, Mutex};

use super::{AuthMode, ProfileEngine, Vauchi, VauchiSyncOutcome};
use crate::api::{ActivityLogWriter, BackgroundBudget, EventDispatcher, VauchiEvent, VauchiResult};
use crate::identity::Identity;
use crate::storage::Storage;

impl Vauchi {
    /// Syncs the foreground profile, or under duress the real profile's
    /// worker.
    pub fn sync(&mut self) -> VauchiResult<VauchiSyncOutcome> {
        self.with_syncing_engine(ProfileEngine::sync)
    }

    /// [`ProfileEngine::periodic_sync_tick`] on the engine that syncs now.
    pub fn periodic_sync_tick(&mut self) -> VauchiResult<VauchiSyncOutcome> {
        self.with_syncing_engine(ProfileEngine::periodic_sync_tick)
    }

    /// [`ProfileEngine::background_sync_tick`] on the engine that syncs now.
    pub fn background_sync_tick(
        &mut self,
        budget: Option<BackgroundBudget>,
    ) -> VauchiResult<VauchiSyncOutcome> {
        self.with_syncing_engine(|engine| engine.background_sync_tick(budget))
    }

    fn with_syncing_engine(
        &mut self,
        run: impl FnOnce(&mut ProfileEngine) -> VauchiResult<VauchiSyncOutcome>,
    ) -> VauchiResult<VauchiSyncOutcome> {
        let Some(held) = self.held.as_mut() else {
            return run(&mut self.foreground);
        };
        // Built for each sync: a connection loads the sealed mask database
        // when it opens, so a long-lived worker would size cover from a stale
        // duress own card.
        if let Some(mut worker) = held.duress_worker()? {
            return worker.sync_as_worker(&mut self.foreground, run);
        }
        // An in-memory profile cannot be opened again: the held engine syncs,
        // with the mask handed back so it sizes cover from the current card.
        let mask = self.foreground.hand_back_storage()?;
        held.storage.take_back_mask_profile(mask)?;
        let events = std::mem::replace(&mut held.events, Arc::new(EventDispatcher::new()));
        let outcome = held.sync_as_worker(&mut self.foreground, run);
        held.events = events;
        self.foreground.storage = held.storage.lend_mask_profile()?;
        outcome
    }
}

impl ProfileEngine {
    /// Runs this real engine's sync for the mask `foreground`: first it
    /// queues the cover the mask's edits owe (ADR-080 point 5, amended
    /// 2026-11-09), then syncs, and records what the sync raised without a
    /// notification.
    fn sync_as_worker(
        &mut self,
        foreground: &mut ProfileEngine,
        run: impl FnOnce(&mut ProfileEngine) -> VauchiResult<VauchiSyncOutcome>,
    ) -> VauchiResult<VauchiSyncOutcome> {
        // Before any relay call, so the cover is queued even when the sync
        // itself then fails, as a duress edit's cover was before the mask.
        self.queue_owed_cover_of(foreground);
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        self.events.add_handler(Arc::new(move |event| {
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(event)
        }));
        let outcome = run(self);
        foreground.last_sync_unix_seconds = self.last_sync_unix_seconds;
        self.record_without_notifying(&received);
        outcome
    }

    /// The real profile opened again on its own connection, with its own
    /// identity and an event dispatcher no handler listens on; it shares the
    /// device's clock, RNG, sleeper and relay transport. `None` for an
    /// in-memory database, which a second connection cannot open.
    fn duress_worker(&self) -> VauchiResult<Option<ProfileEngine>> {
        let (Some(path), Some(key)) = (self.storage.db_path(), self.config.storage_key.clone())
        else {
            return Ok(None);
        };
        let mut storage = Storage::open(path, key)?.with_clock(self.clock.clone());
        // The real profile's ratchets are under the Background Receive Key
        // (vauchi/private#1093), so the worker's storage needs it as every
        // other open of the real profile gets it.
        if let Some(secure) = &self.secure_storage {
            super::storage_keys::attach_background_receive_key(&mut storage, secure.as_ref());
        }
        let identity = storage
            .identity()
            .load_identity()?
            .and_then(|bytes| Identity::from_storage_bytes(&bytes, self.clock.unix_seconds()).ok());
        Ok(Some(ProfileEngine {
            config: self.config.clone(),
            identity,
            shown: None,
            session_password: None,
            storage,
            pending_wipe_database: None,
            events: Arc::new(EventDispatcher::new()),
            secure_storage: self.secure_storage.clone(),
            auth_mode: AuthMode::Duress,
            concealed_duress: Default::default(),
            clock: self.clock.clone(),
            rng: self.rng.clone(),
            sleeper: self.sleeper.clone(),
            monotonic: self.monotonic.clone(),
            relay: self.relay.clone(),
            wipe_requested: std::sync::atomic::AtomicBool::new(false),
            wiped_by_another_device: false,
            last_sync_unix_seconds: self.last_sync_unix_seconds,
            missed_background_run_unix_seconds: None,
            last_emergency_broadcast_unix_seconds: None,
        }))
    }

    /// Records what the worker's sync raised in the real activity log, so
    /// the next real unlock shows it, without a notification now (owner,
    /// 2026-11-08). Best-effort: a failed write must not fail the sync.
    fn record_without_notifying(&self, received: &Mutex<Vec<VauchiEvent>>) {
        let events = std::mem::take(
            &mut *received
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if let Err(error) = ActivityLogWriter::write_without_notifying(
            &self.storage,
            &events,
            self.clock.unix_seconds(),
        ) {
            tracing::warn!(%error, "[Worker] recording received activity failed");
        }
    }
}
