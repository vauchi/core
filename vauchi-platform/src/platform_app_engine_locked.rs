// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The engine's locked start (vauchi/private#580, ADR-043 Amendment 7 §3).
//!
//! When the keychain needs the person to authenticate, or no stored key
//! opens the data, `open_with_keychain` still returns the engine. Until
//! storage opens, `initial_commands_json` and `dispatch_json` answer from
//! Core's storage-lock screens; a retry or a confirmed start-over opens
//! storage and swaps the real engine in.

use std::path::PathBuf;
use std::sync::Arc;

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    AppEngine, RenderContext, StorageLockPresentation, StorageLockReason, StorageLockStep,
};
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::{Command, Event, StorageError, VauchiError};

use crate::error::MobileError;
use crate::{KeychainBridge, MobilePlatformKeychain, PlatformAppEngine, PlatformEventListener};

pub(crate) struct OpenParams {
    pub(crate) storage_path: PathBuf,
    pub(crate) relay_url: String,
    pub(crate) shell_storage_key: Option<SymmetricKey>,
    pub(crate) release_handle: Option<String>,
    pub(crate) keychain: Arc<dyn MobilePlatformKeychain>,
}

pub(crate) enum Opened {
    Engine(Box<Vauchi>),
    Locked(StorageLockReason),
}

impl OpenParams {
    pub(crate) fn open(&self) -> Result<Opened, MobileError> {
        let mut config =
            VauchiConfig::with_storage_path(&self.storage_path).with_relay_url(&self.relay_url);
        // A local relay in Settings needs a distinct OHTTP route, which
        // mobile Settings cannot name (vauchi/private#733 T4.2).
        #[cfg(feature = "dev-local-content")]
        if let Some(url) = option_env!("VAUCHI_DEV_OHTTP_RELAY_URL") {
            config = config.with_ohttp_relay_url(url);
        }
        if let Some(key) = &self.shell_storage_key {
            config = config.with_storage_key(key.clone());
        }
        let bridge = Arc::new(KeychainBridge {
            callback: self.keychain.clone(),
        });
        match Vauchi::with_secure_storage(config, bridge) {
            Ok(vauchi) => Ok(Opened::Engine(Box::new(vauchi))),
            Err(VauchiError::Storage(StorageError::SecureStorageLocked)) => {
                Ok(Opened::Locked(StorageLockReason::Locked))
            }
            Err(VauchiError::Storage(
                StorageError::SecureStorageKeyInvalidated | StorageError::WrongKey,
            )) => Ok(Opened::Locked(StorageLockReason::Unreadable)),
            Err(VauchiError::Storage(StorageError::SecureStorageUnavailable)) => {
                Ok(Opened::Locked(StorageLockReason::Unavailable))
            }
            Err(other) => Err(other.into()),
        }
    }

    /// Deletes the unreadable data; called only after the person confirmed
    /// on Core's start-over screen.
    fn delete_unreadable_data(&self) -> Result<(), MobileError> {
        let bridge = KeychainBridge {
            callback: self.keychain.clone(),
        };
        vauchi_core::api::storage_reset::delete_unreadable_data(&self.storage_path, &bridge)
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })
    }
}

/// Setup calls a shell makes right after construction, kept while storage
/// is closed and applied to the engine once it opens.
#[derive(Default)]
pub(crate) struct PendingSetup {
    pub(crate) render_context: Option<RenderContext>,
    pub(crate) capabilities: Option<DeviceCapabilities>,
    pub(crate) network_online: Option<bool>,
    /// The shell reports its window once; the opened engine needs it too.
    pub(crate) environment: Option<Event>,
    pub(crate) listener: Option<Arc<Box<dyn PlatformEventListener>>>,
}

pub(crate) struct LockedStart {
    presentation: StorageLockPresentation,
    params: OpenParams,
    pub(crate) pending: PendingSetup,
}

impl LockedStart {
    pub(crate) fn new(reason: StorageLockReason, params: OpenParams) -> Self {
        Self {
            presentation: lock_keeping_pick(reason, &params.storage_path),
            params,
            pending: PendingSetup::default(),
        }
    }

    pub(crate) fn keep_render_context(&mut self, context: RenderContext) {
        self.presentation.set_render_context(context.clone());
        self.pending.render_context = Some(context);
    }
}

impl PlatformAppEngine {
    /// Hands `value` to the locked start while storage is closed (`None`),
    /// or back to the caller once it is open.
    pub(crate) fn keep_while_locked<T>(
        &self,
        value: T,
        keep: impl FnOnce(&mut LockedStart, T),
    ) -> Result<Option<T>, MobileError> {
        let mut locked = self.lock_locked_start()?;
        match locked.as_mut() {
            Some(start) => {
                keep(start, value);
                Ok(None)
            }
            None => Ok(Some(value)),
        }
    }

    /// A scheduler run that finds storage locked leaves a record the next
    /// open shows (vauchi/private#1093); the run itself still fails, so the
    /// shell's retry and its chain of runs stay as they are (#603).
    pub(crate) fn record_missed_run_if_locked(&self) -> Result<(), MobileError> {
        let locked = self.lock_locked_start()?;
        let Some(data_dir) = locked
            .as_ref()
            .and_then(|start| start.params.storage_path.parent())
        else {
            return Ok(());
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        // Best effort: an unwritable data directory fails the open anyway.
        #[allow(clippy::let_underscore_must_use)]
        let _ = vauchi_app::ui::record_missed_background_run(data_dir, now);
        Ok(())
    }

    /// The release of a handed-over secret, once, after storage opened.
    pub(crate) fn take_release(&self) -> Result<Option<Command>, MobileError> {
        let mut release = self
            .pending_release
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
        Ok(release
            .take()
            .map(|handle| Command::ForgetStoredSecret { handle }))
    }

    /// The locked start's first batch, or `None` once storage is open.
    pub(crate) fn locked_initial_commands(&self) -> Result<Option<Vec<Command>>, MobileError> {
        let mut locked = self.lock_locked_start()?;
        Ok(locked
            .as_mut()
            .map(|start| start.presentation.initial_commands()))
    }

    /// Commands queued for the hardware shell by the lock screen, or by the
    /// engine a retry just opened (ADR-077).
    pub(crate) fn drain_hardware_after_locked(&self) -> Result<Vec<Command>, MobileError> {
        if let Some(start) = self.lock_locked_start()?.as_mut() {
            return Ok(start.presentation.drain_hardware_commands());
        }
        let mut engine_slot = self.lock_engine()?;
        Ok(engine_slot
            .as_mut()
            .map(|engine| engine.drain_hardware_commands())
            .unwrap_or_default())
    }

    /// Core's answer to undecodable event JSON while locked, or `None`.
    pub(crate) fn locked_reject_event_json(
        &self,
        error: &vauchi_core::EventJsonError,
    ) -> Result<Option<Vec<Command>>, MobileError> {
        let locked = self.lock_locked_start()?;
        Ok(locked
            .as_ref()
            .map(|start| start.presentation.reject_event_json(error)))
    }

    /// Reduces `event` while locked, or `None` once storage is open.
    pub(crate) fn locked_dispatch(
        &self,
        event: Event,
    ) -> Result<Option<Vec<Command>>, MobileError> {
        let mut locked = self.lock_locked_start()?;
        let Some(start) = locked.as_mut() else {
            return Ok(None);
        };
        if matches!(event, Event::PresentationEnvironmentChanged { .. }) {
            start.pending.environment = Some(event.clone());
        }
        let prompt_succeeded = matches!(event, Event::BiometricUnlockSucceeded);
        let outcome = match start.presentation.dispatch(event) {
            StorageLockStep::Commands(commands) => return Ok(Some(commands)),
            StorageLockStep::RetryOpen => self.retry_open(&mut locked, prompt_succeeded),
            StorageLockStep::StartOverConfirmed => start
                .params
                .delete_unreadable_data()
                .and_then(|()| self.retry_open(&mut locked, false)),
        };
        // ADR-045: Core answers a failure with its own alert; the shell never
        // sees the error value.
        Ok(Some(outcome.unwrap_or_else(|_| {
            locked
                .as_ref()
                .map(|start| start.presentation.failure())
                .unwrap_or_default()
        })))
    }

    /// Opens storage if the keychain now answers. When the unlock prompt's
    /// success opened it, the opened engine decides what that unlock means
    /// (an app password when a duress PIN is set up, ADR-032), as it would
    /// had it received the prompt itself.
    fn retry_open(
        &self,
        locked: &mut Option<LockedStart>,
        prompt_succeeded: bool,
    ) -> Result<Vec<Command>, MobileError> {
        let Some(start) = locked.as_mut() else {
            return Ok(Vec::new());
        };
        match start.params.open()? {
            Opened::Locked(reason) => Ok(start.presentation.show(reason)),
            Opened::Engine(vauchi) => {
                let mut engine = engine_keeping_pick(*vauchi, &start.params.storage_path);
                let pending = std::mem::take(&mut start.pending);
                if let Some(context) = pending.render_context {
                    engine.set_render_context(context);
                }
                if let Some(capabilities) = pending.capabilities {
                    engine.set_device_capabilities(capabilities);
                }
                if let Some(online) = pending.network_online {
                    engine.set_network_online(online);
                }
                if let Some(environment) = pending.environment {
                    engine
                        .dispatch(environment)
                        .map_err(|_| MobileError::Other {
                            detail: "Failed to apply the reported environment".into(),
                        })?;
                }
                let mut commands = Vec::new();
                if let Some(capabilities) = start.presentation.hardware_report().cloned() {
                    commands.extend(
                        engine
                            .dispatch(Event::HardwareCapabilitiesReported { capabilities })
                            .map_err(|_| MobileError::Other {
                                detail: "Failed to attach the hardware shell".into(),
                            })?,
                    );
                }
                commands.extend(engine.initial_commands().map_err(|e| MobileError::Other {
                    detail: format!("Failed to compose initial presentation: {e}"),
                })?);
                if prompt_succeeded {
                    commands.extend(engine.dispatch(Event::BiometricUnlockSucceeded).map_err(
                        |_| MobileError::Other {
                            detail: "Failed to decide the unlock".into(),
                        },
                    )?);
                }
                let release = start
                    .params
                    .release_handle
                    .take()
                    .map(|handle| Command::ForgetStoredSecret { handle });
                commands.extend(engine.route_to_shells(release.into_iter().collect()));
                *self.lock_engine()? = Some(engine);
                *locked = None;
                if let Some(listener) = pending.listener {
                    self.attach_event_listener(listener)?;
                }
                Ok(commands)
            }
        }
    }
}

/// Core keeps the language and theme pick beside the database, outside the
/// vault, so the shell stores nothing for it (vauchi/private#814).
pub(crate) fn engine_keeping_pick(vauchi: Vauchi, storage_path: &std::path::Path) -> AppEngine {
    let mut engine = AppEngine::new(vauchi);
    if let Some(data_dir) = storage_path.parent() {
        engine.keep_restorable_state_in(data_dir);
    }
    engine
}

fn lock_keeping_pick(
    reason: StorageLockReason,
    storage_path: &std::path::Path,
) -> StorageLockPresentation {
    let mut presentation = StorageLockPresentation::new(reason, Locale::ENGLISH);
    if let Some(data_dir) = storage_path.parent() {
        presentation.keep_restorable_state_in(data_dir);
    }
    presentation
}
