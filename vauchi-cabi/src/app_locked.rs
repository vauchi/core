// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The desktop's locked start (vauchi/private#581).
//!
//! The keyring constructor no longer returns a null handle for a storage-key
//! problem. When the keyring holds the keys but does not answer, or no key
//! opens the data, the app starts on Core's storage-lock screens, served
//! through the same `vauchi_app_initial_commands` / `vauchi_app_dispatch`
//! calls, so no desktop shell changes (ADR-066).

use std::path::PathBuf;
use std::sync::Arc;

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{AppEngine, StorageLockPresentation, StorageLockReason, StorageLockStep};
use vauchi_core::api::storage_reset::delete_unreadable_data;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::storage::SecureStorage;
use vauchi_core::{Command, Event, StorageError, VauchiError};

use crate::VauchiApp;
use crate::app::{EventCallbackHandler, open_with_file_key, register_event_callback};

pub(crate) struct KeyringOpen {
    pub(crate) data_path: PathBuf,
    pub(crate) config: VauchiConfig,
    pub(crate) keyring: Option<Arc<dyn SecureStorage>>,
}

pub(crate) enum Opened {
    Engine(Box<Vauchi>),
    Locked(StorageLockReason),
}

/// Secure storage for an app without a keyring: holds nothing.
struct NoKeys;

impl SecureStorage for NoKeys {
    fn save_key(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Ok(())
    }
    fn load_key(&self, _: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(None)
    }
    fn delete_key(&self, _: &str) -> Result<(), StorageError> {
        Ok(())
    }
}

impl KeyringOpen {
    fn storage_path(&self) -> PathBuf {
        self.config.storage_path.clone()
    }

    /// Keyring first when it answers, then the file-backed key; a problem
    /// neither resolves becomes the screen that says what the person can do.
    pub(crate) fn open(&self) -> Opened {
        let answering = self
            .keyring
            .as_ref()
            .filter(|keyring| keyring.load_key("_probe").is_ok());
        let Some(keyring) = answering else {
            return match open_with_file_key(&self.data_path, self.config.clone()) {
                Ok(vauchi) => Opened::Engine(Box::new(vauchi)),
                // Data no file key opens while a keyring exists but does not
                // answer: most likely the keyring holds the key.
                Err(_) if self.keyring.is_some() => Opened::Locked(StorageLockReason::Unavailable),
                Err(VauchiError::Storage(StorageError::WrongKey)) => {
                    Opened::Locked(StorageLockReason::Unreadable)
                }
                Err(_) => Opened::Locked(StorageLockReason::Unavailable),
            };
        };
        match Vauchi::with_secure_storage(self.config.clone(), keyring.clone()) {
            Ok(vauchi) => Opened::Engine(Box::new(vauchi)),
            Err(VauchiError::Storage(StorageError::SecureStorageLocked)) => {
                Opened::Locked(StorageLockReason::Locked)
            }
            Err(VauchiError::Storage(StorageError::SecureStorageUnavailable)) => {
                Opened::Locked(StorageLockReason::Unavailable)
            }
            Err(_) => match open_with_file_key(&self.data_path, self.config.clone()) {
                Ok(vauchi) => Opened::Engine(Box::new(vauchi)),
                Err(VauchiError::Storage(StorageError::WrongKey)) => {
                    Opened::Locked(StorageLockReason::Unreadable)
                }
                Err(_) => Opened::Locked(StorageLockReason::Unavailable),
            },
        }
    }
}

pub(crate) struct LockedStart {
    presentation: StorageLockPresentation,
    params: KeyringOpen,
    /// A `vauchi_app_set_event_callback` made while locked, applied on open.
    pending_callback: Option<Option<EventCallbackHandler>>,
}

impl LockedStart {
    pub(crate) fn keep_event_callback(&mut self, handler: Option<EventCallbackHandler>) {
        self.pending_callback = Some(handler);
    }
}

/// Opens an app handle, on Core's storage-lock screens if storage stays
/// closed. Never null.
pub(crate) fn open_app(
    data_path: &std::path::Path,
    config: VauchiConfig,
    keyring: Option<Arc<dyn SecureStorage>>,
) -> *mut VauchiApp {
    let params = KeyringOpen {
        data_path: data_path.to_path_buf(),
        config,
        keyring,
    };
    let app = match params.open() {
        Opened::Engine(vauchi) => VauchiApp::opened(AppEngine::new(*vauchi)),
        Opened::Locked(reason) => VauchiApp::locked(LockedStart {
            presentation: StorageLockPresentation::new(reason, Locale::English),
            params,
            pending_callback: None,
        }),
    };
    Box::into_raw(Box::new(app))
}

impl VauchiApp {
    pub(crate) fn locked(start: LockedStart) -> Self {
        Self {
            engine: std::sync::Mutex::new(None),
            locked: std::sync::Mutex::new(Some(start)),
            event_handler_id: std::sync::Mutex::new(None),
        }
    }

    /// Keeps `handler` for when storage opens (`None`), or hands it back
    /// once storage is open.
    pub(crate) fn keep_event_callback_while_locked(
        &self,
        handler: Option<EventCallbackHandler>,
    ) -> Option<Option<EventCallbackHandler>> {
        let Ok(mut locked) = self.locked.lock() else {
            return Some(handler);
        };
        match locked.as_mut() {
            Some(start) => {
                start.keep_event_callback(handler);
                None
            }
            None => Some(handler),
        }
    }

    /// The locked start's first batch, or `None` once storage is open.
    pub(crate) fn locked_initial_commands(&self) -> Option<Vec<Command>> {
        let mut locked = self.locked.lock().ok()?;
        locked
            .as_mut()
            .map(|start| start.presentation.initial_commands())
    }

    /// Core's answer to undecodable event JSON while locked, or `None`.
    pub(crate) fn locked_reject_event_json(
        &self,
        error: &vauchi_core::EventJsonError,
    ) -> Option<Vec<Command>> {
        let locked = self.locked.lock().ok()?;
        locked
            .as_ref()
            .map(|start| start.presentation.reject_event_json(error))
    }

    /// Reduces `event` while locked, or `None` once storage is open.
    pub(crate) fn locked_dispatch(&self, event: Event) -> Option<Vec<Command>> {
        let mut locked = self.locked.lock().ok()?;
        let start = locked.as_mut()?;
        let commands = match start.presentation.dispatch(event) {
            StorageLockStep::Commands(commands) => commands,
            StorageLockStep::StartOverConfirmed => {
                let deleted = match &start.params.keyring {
                    Some(keyring) => {
                        delete_unreadable_data(&start.params.storage_path(), &**keyring)
                    }
                    None => delete_unreadable_data(&start.params.storage_path(), &NoKeys),
                };
                match deleted {
                    Ok(()) => return self.locked_retry(&mut locked),
                    Err(_) => start.presentation.failure(),
                }
            }
            StorageLockStep::RetryOpen => return self.locked_retry(&mut locked),
        };
        Some(commands)
    }

    fn locked_retry(&self, locked: &mut Option<LockedStart>) -> Option<Vec<Command>> {
        let start = locked.as_mut()?;
        let commands = match start.params.open() {
            Opened::Locked(reason) => start.presentation.show(reason),
            Opened::Engine(vauchi) => {
                let mut engine = AppEngine::new(*vauchi);
                let commands = engine
                    .initial_commands()
                    .unwrap_or_else(|_| start.presentation.failure());
                let pending_callback = start.pending_callback.take();
                if let Ok(mut slot) = self.engine.lock() {
                    *slot = Some(engine);
                }
                *locked = None;
                if let Some(handler) = pending_callback {
                    register_event_callback(self, handler);
                }
                commands
            }
        };
        Some(commands)
    }
}
