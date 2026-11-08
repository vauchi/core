// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Under duress the real profile syncs as a background worker: its own
//! connection, identity and event dispatcher, which nothing in the UI holds
//! (ADR-080 points 2 and 5). It runs in duress mode, so it syncs exactly as a
//! duress session does today.

use std::sync::Arc;

use super::{AuthMode, ProfileEngine, Vauchi, VauchiSyncOutcome};
use crate::api::{BackgroundBudget, EventDispatcher, VauchiResult};
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
        if !self.foreground.in_duress_mode() {
            return run(&mut self.foreground);
        }
        // Built for each sync: a connection loads the sealed mask database
        // when it opens, so a long-lived worker would size cover from a stale
        // duress own card.
        let Some(mut worker) = self.foreground.duress_worker()? else {
            return run(&mut self.foreground);
        };
        worker.take_sync_state_from(&self.foreground);
        let outcome = run(&mut worker);
        self.foreground.take_sync_state_from(&worker);
        outcome
    }
}

impl ProfileEngine {
    /// The real profile opened again on its own connection, with its own
    /// identity and an event dispatcher no handler listens on; it shares the
    /// device's clock, RNG, sleeper and relay transport. `None` for an
    /// in-memory database, which a second connection cannot open.
    fn duress_worker(&self) -> VauchiResult<Option<ProfileEngine>> {
        let (Some(path), Some(key)) = (self.storage.db_path(), self.config.storage_key.clone())
        else {
            return Ok(None);
        };
        let storage = Storage::open(path, key)?.with_clock(self.clock.clone());
        let identity = storage
            .identity()
            .load_identity()?
            .and_then(|bytes| Identity::from_storage_bytes(&bytes, self.clock.unix_seconds()).ok());
        Ok(Some(ProfileEngine {
            config: self.config.clone(),
            identity,
            storage,
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
            last_emergency_broadcast_unix_seconds: None,
        }))
    }

    /// Takes the cover a duress edit owes and the last sync time, so each
    /// sync continues where the other engine's left off.
    fn take_sync_state_from(&mut self, other: &ProfileEngine) {
        self.concealed_duress().owed_cover =
            std::mem::take(&mut other.concealed_duress().owed_cover);
        self.last_sync_unix_seconds = other.last_sync_unix_seconds;
    }
}
