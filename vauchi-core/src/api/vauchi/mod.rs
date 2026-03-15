// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Orchestrator
//!
//! Main entry point for the Vauchi API.

mod builder;
mod contacts;
mod devices;
mod emergency;
mod exchange;
mod features;
mod identity;
mod merge;
mod onboarding;
mod propagation;
mod security;
mod setup;
mod visibility;

pub use builder::VauchiBuilder;
pub use devices::{DeviceInfo, DeviceLinkResult};
pub use exchange::ExchangeQrData;
pub use setup::SetupProgress;

use std::sync::{Arc, Mutex};

use crate::crypto::{ShreddingMasterKey, SymmetricKey};
use crate::identity::Identity;
use crate::storage::{SecureStorage, Storage};
use crate::sync::state::ReplayDetector;

use super::config::VauchiConfig;
use super::duress::DuressAlert;
use super::error::{VauchiError, VauchiResult};
use super::events::EventDispatcher;

/// Recovery readiness assessment.
///
/// Aggregates the count of recovery-trusted contacts against the configured
/// threshold, so clients can display readiness without computing it inline.
#[derive(Debug, Clone)]
pub struct RecoveryReadiness {
    /// Number of contacts marked as recovery-trusted.
    pub trusted_count: usize,
    /// The recovery threshold from configuration.
    pub threshold: u32,
    /// Whether the user has enough trusted contacts for recovery.
    pub is_ready: bool,
    /// How many more trusted contacts are needed (`threshold - trusted_count`, saturating).
    pub shortfall: usize,
}

/// Authentication mode for the Vauchi instance.
///
/// Determines which data is shown to the user. The password system is
/// opt-in: without a password, the mode is `Unauthenticated` and behaves
/// identically to the legacy (pre-password) behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// The normal (real) password was used — show real contacts.
    Normal,
    /// The duress PIN was used — show decoy contacts only.
    Duress,
    /// No password is set — backward-compatible, show real contacts.
    Unauthenticated,
}

/// Main Vauchi orchestrator.
///
/// This is the primary entry point for using Vauchi. It coordinates:
/// - Identity management
/// - Contact management
/// - Synchronization
/// - Event dispatching
///
/// # Example
///
/// ```ignore
/// use vauchi_core::api::{Vauchi, VauchiConfig};
///
/// // Create Vauchi with default config
/// let mut wb = Vauchi::new(VauchiConfig::default())?;
///
/// // Create identity
/// wb.create_identity("Alice")?;
///
/// // Add event handler
/// wb.add_event_handler(|event| {
///     println!("Event: {:?}", event);
/// });
///
/// // Update contact card
/// let mut card = wb.own_card()?.unwrap();
/// card.add_field(ContactField::new(FieldType::Email, "email", "alice@example.com"));
/// wb.update_own_card(&card)?;
///
/// // Connect and sync
/// wb.connect()?;
/// wb.sync()?;
/// ```
/// Key name used to store SMK in SecureStorage.
const SMK_KEY_NAME: &str = "smk";

/// Main API entry point for the Vauchi contact card system.
///
/// Coordinates identity management, contact exchange, synchronization, and event dispatching.
///
/// Transport is type-erased (ADR-030): callers no longer need to carry a
/// generic `<T: Transport>` through the call chain. `with_transport_factory()`
/// is retained for call-site compatibility but does not invoke the factory —
/// connection management will be handled separately.
pub struct Vauchi {
    config: VauchiConfig,
    identity: Option<Identity>,
    storage: Storage,
    events: Arc<EventDispatcher>,
    secure_storage: Option<Arc<dyn SecureStorage>>,
    replay_detector: Mutex<ReplayDetector>,
    auth_mode: AuthMode,
    /// In-memory queue of duress alerts waiting to be sent.
    ///
    /// Populated when `authenticate()` detects a duress PIN. Alerts are
    /// drained by the sync system and sent as card updates to trusted
    /// contacts, indistinguishable from normal sync traffic.
    duress_alerts: Vec<DuressAlert>,
}

impl Vauchi {
    /// Creates a new Vauchi instance with mock transport (for testing).
    pub fn new(config: VauchiConfig) -> VauchiResult<Self> {
        Self::init(config, None)
    }

    /// Creates a new Vauchi instance using SMK from SecureStorage for encryption.
    ///
    /// Boot flow (DP-1): Load SMK from SecureStorage → derive SEK → open Storage.
    /// Falls back to `config.storage_key` if SMK is not found in SecureStorage.
    pub fn with_secure_storage(
        config: VauchiConfig,
        secure_storage: Arc<dyn SecureStorage>,
    ) -> VauchiResult<Self> {
        Self::init(config, Some(secure_storage))
    }

    /// Creates a new Vauchi instance (transport factory accepted but not invoked).
    ///
    /// **ADR-030**: The factory parameter is retained for call-site compatibility
    /// only. The closure is never called and no transport is created or stored.
    /// Use [`Vauchi::new()`] or [`Vauchi::with_secure_storage()`] instead.
    #[deprecated(
        since = "0.15.0",
        note = "ADR-030: transport factory is not invoked. Use Vauchi::new() or Vauchi::with_secure_storage(). Transport will be reconnected in a future phase."
    )]
    pub fn with_transport_factory<T: crate::network::Transport, F>(
        config: VauchiConfig,
        _transport_factory: F,
    ) -> VauchiResult<Self>
    where
        F: FnOnce() -> T,
    {
        Self::init(config, None)
    }

    /// Creates a new Vauchi instance with optional SecureStorage (transport factory not invoked).
    ///
    /// **ADR-030**: The factory parameter is retained for call-site compatibility only.
    /// Use [`Vauchi::with_secure_storage()`] instead.
    #[deprecated(
        since = "0.15.0",
        note = "ADR-030: transport factory is not invoked. Use Vauchi::with_secure_storage(). Transport will be reconnected in a future phase."
    )]
    pub fn with_transport_and_secure_storage<T: crate::network::Transport, F>(
        config: VauchiConfig,
        _transport_factory: F,
        secure_storage: Option<Arc<dyn SecureStorage>>,
    ) -> VauchiResult<Self>
    where
        F: FnOnce() -> T,
    {
        Self::init(config, secure_storage)
    }

    /// Internal initializer shared by all constructors.
    fn init(
        config: VauchiConfig,
        secure_storage: Option<Arc<dyn SecureStorage>>,
    ) -> VauchiResult<Self> {
        // Determine the storage encryption key
        let storage_key = Self::resolve_storage_key(&config, secure_storage.as_deref())?;

        // Open or create storage
        let storage = if config.storage_path.exists() {
            Storage::open(&config.storage_path, storage_key)?
        } else {
            // Create parent directories if needed
            if let Some(parent) = config.storage_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| VauchiError::Configuration(e.to_string()))?;
            }
            Storage::open(&config.storage_path, storage_key)?
        };

        let events = Arc::new(EventDispatcher::new());

        // Try to load a persisted identity from storage
        let identity = match storage.load_identity() {
            Ok(Some((bytes, _display_name))) => Identity::from_storage_bytes(&bytes).ok(),
            _ => None,
        };

        Ok(Vauchi {
            config,
            identity,
            storage,
            events,
            secure_storage,
            replay_detector: Mutex::new(ReplayDetector::default_tolerance()),
            auth_mode: AuthMode::Unauthenticated,
            duress_alerts: Vec::new(),
        })
    }

    /// Resolves the storage encryption key from available sources.
    ///
    /// Priority:
    /// 1. SMK from SecureStorage → derive SEK
    /// 2. Explicit storage_key from config
    /// 3. Generate random key (ephemeral, not persistent)
    fn resolve_storage_key(
        config: &VauchiConfig,
        secure_storage: Option<&dyn SecureStorage>,
    ) -> VauchiResult<SymmetricKey> {
        // Try loading SMK from SecureStorage
        if let Some(ss) = secure_storage {
            if let Some(smk_bytes) = ss.load_key(SMK_KEY_NAME).map_err(|e| {
                VauchiError::Configuration(format!("Failed to load SMK from SecureStorage: {}", e))
            })? {
                let smk_array: [u8; 32] = smk_bytes.try_into().map_err(|_| {
                    VauchiError::Configuration("SMK in SecureStorage has invalid length".into())
                })?;
                let smk = ShreddingMasterKey::from_bytes(smk_array);
                return Ok(smk.derive_sek());
            }
        }

        // Fall back to config storage key or generate random
        Ok(config
            .storage_key
            .clone()
            .unwrap_or_else(SymmetricKey::generate))
    }

    /// Creates a new Vauchi instance with in-memory storage (for testing).
    pub fn in_memory() -> VauchiResult<Self> {
        let storage_key = SymmetricKey::generate();
        let storage = Storage::in_memory(storage_key)?;
        let events = Arc::new(EventDispatcher::new());

        Ok(Vauchi {
            config: VauchiConfig::default(),
            identity: None,
            storage,
            events,
            secure_storage: None,
            replay_detector: Mutex::new(ReplayDetector::default_tolerance()),
            auth_mode: AuthMode::Unauthenticated,
            duress_alerts: Vec::new(),
        })
    }
}
