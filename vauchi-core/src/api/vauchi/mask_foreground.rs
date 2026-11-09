// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A duress unlock puts the mask engine in the foreground: the mask profile
//! the real `Storage` lends it, the mask seed's identity, and the real public
//! ID, fingerprint and name as what it shows. `Vauchi` holds the real engine
//! meanwhile, and an unlock that is not duress puts it back (ADR-080 point 4;
//! owner, 2026-11-08).

use std::sync::Arc;

use super::{AuthMode, BiometricUnlockOutcome, ProfileEngine, Vauchi};
use crate::api::{AppPasswordConfig, EventDispatcher, VauchiError, VauchiResult};
use crate::identity::Identity;
use crate::storage::Storage;

impl Vauchi {
    /// [`ProfileEngine::authenticate`] on the real engine. A duress unlock
    /// then puts the mask engine in the foreground.
    pub fn authenticate(&mut self, password: &str) -> VauchiResult<AuthMode> {
        self.bring_the_real_engine_back()?;
        let mode = self.foreground.authenticate(password)?;
        if mode == AuthMode::Duress {
            self.put_the_mask_in_front()?;
        }
        Ok(mode)
    }

    /// [`ProfileEngine::biometric_unlock_check`], decided by the real
    /// setup. An unlock brings the real engine back; a prompt for the PIN
    /// leaves the mask in front until the PIN decides.
    pub fn biometric_unlock_check(&mut self) -> VauchiResult<BiometricUnlockOutcome> {
        let outcome = self.real_engine_mut().biometric_unlock_check()?;
        if outcome == BiometricUnlockOutcome::Unlocked {
            self.bring_the_real_engine_back()?;
        }
        Ok(outcome)
    }

    pub(super) fn real_engine_mut(&mut self) -> &mut ProfileEngine {
        self.held.as_mut().unwrap_or(&mut self.foreground)
    }

    fn put_the_mask_in_front(&mut self) -> VauchiResult<()> {
        let real = &mut self.foreground;
        if let Some(identity) = &real.identity {
            real.storage.keep_mask_public_values(identity)?;
        }
        let session_password = real
            .password_config()?
            .ok_or_else(|| VauchiError::InvalidState("no password configured".into()))?
            .duress_session_password()?;
        let mask = real.storage.lend_mask_profile()?;
        let engine = match real.mask_engine(mask, session_password) {
            Ok(engine) => engine,
            Err((error, mask)) => {
                self.foreground.storage.take_back_mask_profile(*mask)?;
                return Err(error);
            }
        };
        self.held = Some(std::mem::replace(&mut self.foreground, engine));
        Ok(())
    }

    fn bring_the_real_engine_back(&mut self) -> VauchiResult<()> {
        let Some(real) = self.held.take() else {
            return Ok(());
        };
        let mut mask_engine = std::mem::replace(&mut self.foreground, real);
        let mask = mask_engine.hand_back_storage()?;
        self.foreground.storage.take_back_mask_profile(mask)?;
        // Still in duress mode here: the real engine runs again, so it
        // queues what the session's edits owe before the unlock decides.
        self.foreground.queue_owed_cover_of(&mut mask_engine);
        Ok(())
    }
}

impl ProfileEngine {
    /// The engine of the mask profile `mask`. It holds neither the storage
    /// key nor secure storage, and runs in duress mode until the per-method
    /// guards go (plan task 7.4). On failure `mask` comes back with the
    /// error, so the lender can take it back.
    fn mask_engine(
        &self,
        mask: Storage,
        session_password: AppPasswordConfig,
    ) -> Result<ProfileEngine, (VauchiError, Box<Storage>)> {
        let loaded = (|| {
            let seed = mask.mask_seed().load()?;
            let shown = mask.mask_public_values().load()?;
            Ok::<_, crate::storage::StorageError>((seed, shown))
        })();
        let (seed, shown) = match loaded {
            Ok((Some(seed), Some(shown))) => (seed, shown),
            Ok(_) => {
                let missing = VauchiError::InvalidState(
                    "the mask profile has no seed or public values".into(),
                );
                return Err((missing, Box::new(mask)));
            }
            Err(error) => return Err((error.into(), Box::new(mask))),
        };
        let identity = Identity::for_mask(
            seed.as_bytes(),
            shown.display_name(),
            self.clock.unix_seconds(),
        );
        let mut config = self.config.clone();
        config.storage_key = None;
        Ok(ProfileEngine {
            config,
            identity: Some(identity),
            shown: Some(shown),
            session_password: Some(session_password),
            storage: mask,
            pending_wipe_database: None,
            events: Arc::new(EventDispatcher::new()),
            secure_storage: None,
            auth_mode: AuthMode::Duress,
            concealed_duress: Default::default(),
            clock: self.clock.clone(),
            rng: self.rng.clone(),
            sleeper: self.sleeper.clone(),
            monotonic: self.monotonic.clone(),
            #[cfg(feature = "network-http")]
            relay: self.relay.clone(),
            wipe_requested: std::sync::atomic::AtomicBool::new(false),
            wiped_by_another_device: false,
            last_sync_unix_seconds: self.last_sync_unix_seconds,
            missed_background_run_unix_seconds: None,
            last_emergency_broadcast_unix_seconds: None,
        })
    }

    /// Takes the cover a duress edit owes and the last sync time, so each
    /// sync continues where the other engine's left off.
    pub(super) fn take_sync_state_from(&mut self, other: &ProfileEngine) {
        let (owed_cover, owed_journal) = {
            let mut theirs = other.concealed_duress();
            (
                std::mem::take(&mut theirs.owed_cover),
                std::mem::take(&mut theirs.owed_journal_cover),
            )
        };
        let mut ours = self.concealed_duress();
        ours.owed_cover = owed_cover;
        ours.owed_journal_cover.extend(owed_journal);
        drop(ours);
        self.last_sync_unix_seconds = other.last_sync_unix_seconds;
    }

    /// Queues, on this real engine, the cover the mask engine's edits owe:
    /// the journal items for the other devices and the owed card pass for
    /// contacts (ADR-080 point 5, amended 2026-11-09). What fails to queue
    /// stays owed on the mask.
    pub(super) fn queue_owed_cover_of(&mut self, mask: &mut ProfileEngine) {
        self.take_sync_state_from(mask);
        self.journal_owed_cover();
        if let Err(error) = self.run_owed_cover() {
            tracing::warn!(%error, "[Mask] queueing owed cover failed");
        }
        mask.take_sync_state_from(self);
    }

    /// This engine's `Storage`, an empty stand-in left in its place.
    pub(super) fn hand_back_storage(&mut self) -> VauchiResult<Storage> {
        Ok(std::mem::replace(&mut self.storage, Storage::stand_in()?))
    }
}
