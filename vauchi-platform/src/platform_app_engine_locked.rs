// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
    pub(crate) listener: Option<Box<dyn PlatformEventListener>>,
}

pub(crate) struct LockedStart {
    presentation: StorageLockPresentation,
    params: OpenParams,
    pub(crate) pending: PendingSetup,
}

impl LockedStart {
    pub(crate) fn new(reason: StorageLockReason, params: OpenParams) -> Self {
        Self {
            presentation: StorageLockPresentation::new(reason, Locale::English),
            params,
            pending: PendingSetup::default(),
        }
    }

    pub(crate) fn keep_render_context(&mut self, context: RenderContext) {
        self.presentation.set_locale(context.resolved_locale());
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
        let outcome = match start.presentation.dispatch(event) {
            StorageLockStep::Commands(commands) => return Ok(Some(commands)),
            StorageLockStep::RetryOpen => self.retry_open(&mut locked),
            StorageLockStep::StartOverConfirmed => start
                .params
                .delete_unreadable_data()
                .and_then(|()| self.retry_open(&mut locked)),
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

    fn retry_open(&self, locked: &mut Option<LockedStart>) -> Result<Vec<Command>, MobileError> {
        let Some(start) = locked.as_mut() else {
            return Ok(Vec::new());
        };
        match start.params.open()? {
            Opened::Locked(reason) => Ok(start.presentation.show(reason)),
            Opened::Engine(vauchi) => {
                let mut engine = AppEngine::new(*vauchi);
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
                let commands = engine.initial_commands().map_err(|e| MobileError::Other {
                    detail: format!("Failed to compose initial presentation: {e}"),
                })?;
                let mut commands = commands;
                commands.extend(
                    start
                        .params
                        .release_handle
                        .take()
                        .map(|handle| Command::ForgetStoredSecret { handle }),
                );
                *self.lock_engine()? = Some(engine);
                *locked = None;
                if let Some(listener) = pending.listener {
                    self.register_event_listener(listener)?;
                }
                Ok(commands)
            }
        }
    }
}
