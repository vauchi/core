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
#[cfg(feature = "network-http")]
mod escrow_exchange;
mod exchange;
#[cfg(feature = "network-http")]
mod exchange_relay;
mod features;
mod identity;
mod import;
mod merge;
mod onboarding;
mod propagation;
#[cfg(feature = "network-http")]
mod receive_routing;
#[cfg(feature = "network-http")]
mod recovery;
mod recovery_offline;
mod security;
mod setup;
#[cfg(feature = "network-http")]
mod sync_http;
mod visibility;

pub use builder::VauchiBuilder;
pub use devices::{DeviceInfo, DeviceLinkResult};
pub use exchange::ExchangeQrData;
#[cfg(feature = "network-http")]
pub use exchange_relay::{RelayExchangeOffer, RelayExchangeResult};
pub use import::{ImportResult, ImportWarning};
#[cfg(feature = "network-http")]
#[allow(unused_imports)]
// re-exported for integration tests; lint can't see external consumers
pub use receive_routing::{BlobOutcome, process_received_blobs};
pub use security::BIOMETRIC_UNLOCK_MIN_DURATION;
pub use setup::SetupProgress;
#[cfg(feature = "network-http")]
pub use sync_http::{PERIODIC_SYNC_INTERVAL_SECONDS, PERIODIC_SYNC_MAX_RETRIES};

use std::sync::{Arc, Mutex};

use crate::clock::{Clock, SystemClock};
use crate::crypto::{ShreddingMasterKey, SymmetricKey};
use crate::identity::Identity;
use crate::monotonic::{MonotonicClock, SystemMonotonicClock};
use crate::rng::{OsSecureRng, SecureRng};
use crate::sleeper::{Sleeper, SystemSleeper};
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
#[non_exhaustive]
pub enum AuthMode {
    /// The normal (real) password was used — show real contacts.
    Normal,
    /// The duress PIN was used — show decoy contacts only.
    Duress,
    /// No password is set — backward-compatible, show real contacts.
    Unauthenticated,
}

/// Outcome of [`Vauchi::biometric_unlock_check`].
///
/// Returned after a successful platform biometric authentication
/// (LAContext on iOS, BiometricPrompt on Android). The variant tells
/// the frontend which screen to render next:
///
/// - `Unlocked`: biometric proves the real user; transition to the
///   post-auth screen.
/// - `PromptForDuressPin`: a duress PIN is configured, so the user
///   must enter the PIN — that PIN check determines `Normal` vs
///   `Duress` mode via [`Vauchi::authenticate`].
///
/// The dispatcher is constant-time: the wall-clock duration of the
/// containing call is at least
/// [`security::BIOMETRIC_UNLOCK_MIN_DURATION`] regardless of which
/// outcome is returned, so an observer cannot infer whether duress is
/// configured by timing the unlock animation.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum BiometricUnlockOutcome {
    /// Biometric authentication succeeded and no duress PIN is
    /// configured — the user is fully unlocked. `auth_mode` is set to
    /// [`AuthMode::Normal`].
    Unlocked,
    /// Biometric authentication succeeded but a duress PIN is
    /// configured — the frontend must present the PIN entry screen so
    /// the user enters either the real PIN or the duress PIN. The
    /// subsequent [`Vauchi::authenticate`] call sets the final
    /// [`AuthMode`].
    PromptForDuressPin,
}

/// Outcome of a `Vauchi::sync()` call.
#[derive(Debug)]
pub enum VauchiSyncOutcome {
    /// Sync completed — received and sent counts.
    Ok {
        received: usize,
        sent: usize,
        acknowledged: usize,
        errors: Vec<String>,
        /// Version policy from relay response headers (if any).
        version_policy: Option<crate::version::VersionPolicy>,
    },
    /// Called too soon (C1 post-exchange or C2 jitter).
    TooSoon,
    /// Not connected — call connect() first.
    NotConnected,
    /// Identity not created yet.
    NoIdentity,
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
    /// Explicit-time seam (Phase 1 / Task 1.1 of the pure-functional-core
    /// program). Every `SystemTime::now` callsite under `vauchi-core`
    /// migrates to `self.clock.now()` cluster by cluster — Step 3 follow-up
    /// MRs. Default is `SystemClock::shared()`; tests pass a `FakeClock`
    /// via `Vauchi::new_with`.
    clock: Arc<dyn Clock>,
    /// Explicit-randomness seam (Phase 1 / Task 1.2 of the pure-
    /// functional-core program). Non-crypto random draws inside
    /// `vauchi-core` will migrate to `self.rng.*` cluster by cluster
    /// — replacing the transitional `crate::rng::non_crypto_rng()`
    /// helper. Default is `OsSecureRng::shared()`; tests pass a
    /// `DeterministicRng` via `Vauchi::new_with` /
    /// `Vauchi::in_memory_with_clock_and_rng` so the engine becomes
    /// a deterministic function of `(state, input)`.
    rng: Arc<dyn SecureRng>,
    /// Explicit-suspension seam (Phase 1 / Task 1.3 of the pure-
    /// functional-core program). The last remaining suspension
    /// site under `vauchi-core/src/` — the constant-time floor in
    /// `biometric_unlock_check` — routes through `self.sleeper.sleep`.
    /// Default is `SystemSleeper::shared()` (real wall-clock floor —
    /// security-critical, see `BIOMETRIC_UNLOCK_MIN_DURATION`); tests
    /// inject a `FakeSleeper` via `Vauchi::with_sleeper` to skip
    /// the 300 ms pad while still asserting the floor was requested.
    sleeper: Arc<dyn Sleeper>,
    /// Explicit-monotonic-time seam (Phase 1 / Task 1.1b of the pure-
    /// functional-core program). Every `Instant::now()` read that
    /// drives a timeout, deadline, or retry window under `vauchi-core`
    /// migrates to `self.monotonic.now()` cluster by cluster. Default
    /// is `SystemMonotonicClock::shared()`; tests inject a
    /// `FakeMonotonicClock` via `Vauchi::with_monotonic` to advance
    /// monotonic time deterministically. Diagnostic perf-timers are
    /// exempt (see `crate::monotonic` module docs).
    monotonic: Arc<dyn MonotonicClock>,
    /// In-memory queue of duress alerts waiting to be sent.
    ///
    /// Populated when `authenticate()` detects a duress PIN. Alerts are
    /// drained by the sync system and sent as card updates to trusted
    /// contacts, indistinguishable from normal sync traffic.
    duress_alerts: Vec<DuressAlert>,

    /// Cached OHTTP key (loaded from Storage on connect).
    #[cfg(feature = "network-http")]
    ohttp_key: Option<crate::network::OhttpClient>,
    /// Deadline before which sync() returns TooSoon.
    #[cfg(feature = "network-http")]
    next_sync_allowed: Option<std::time::Instant>,
    /// Timestamp of last successful exchange (for C1 post-exchange delay).
    #[cfg(feature = "network-http")]
    last_exchange_time: Option<std::time::Instant>,
    /// Wall-clock unix seconds of the last successful sync, captured via
    /// `clock().unix_seconds()` inside `update_timing_after_sync`. Resets
    /// on process restart — in-memory only. Surfaced via `last_sync_time()`
    /// so MyInfoEngine can render "Last synced X ago" without the frontend
    /// owning the timestamp (humble-UI follow-up to ios!472).
    last_sync_unix_seconds: Option<u64>,
}

impl Vauchi {
    /// Creates a new Vauchi instance with mock transport (for testing).
    pub fn new(config: VauchiConfig) -> VauchiResult<Self> {
        Self::init(config, None, None, None)
    }

    /// Creates a new Vauchi instance using SMK from SecureStorage for encryption.
    ///
    /// Boot flow (DP-1): Load SMK from SecureStorage → derive SEK → open Storage.
    /// Falls back to `config.storage_key` if SMK is not found in SecureStorage.
    pub fn with_secure_storage(
        config: VauchiConfig,
        secure_storage: Arc<dyn SecureStorage>,
    ) -> VauchiResult<Self> {
        Self::init(config, Some(secure_storage), None, None)
    }

    /// Creates a new Vauchi instance with an explicit [`Clock`] and
    /// optional [`SecureStorage`].
    ///
    /// Phase 1 / Task 1.1 of the pure-functional-core program plan —
    /// the test seam for the wall-clock. Production code keeps using
    /// [`Vauchi::new`] / [`Vauchi::with_secure_storage`], which wrap
    /// [`SystemClock::shared`] internally. Tests pass a `FakeClock` so
    /// the state machine becomes a deterministic function of
    /// `(state, input)` — the headline goal of the program.
    pub fn new_with(
        config: VauchiConfig,
        clock: Arc<dyn Clock>,
        rng: Arc<dyn SecureRng>,
        secure_storage: Option<Arc<dyn SecureStorage>>,
    ) -> VauchiResult<Self> {
        Self::init(config, secure_storage, Some(clock), Some(rng))
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
        Self::init(config, None, None, None)
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
        Self::init(config, secure_storage, None, None)
    }

    /// Internal initializer shared by all constructors.
    fn init(
        config: VauchiConfig,
        secure_storage: Option<Arc<dyn SecureStorage>>,
        clock: Option<Arc<dyn Clock>>,
        rng: Option<Arc<dyn SecureRng>>,
    ) -> VauchiResult<Self> {
        let clock = clock.unwrap_or_else(SystemClock::shared);
        let rng = rng.unwrap_or_else(OsSecureRng::shared);
        let sleeper = SystemSleeper::shared();
        let monotonic = SystemMonotonicClock::shared();

        // Determine the storage encryption key
        let storage_key = Self::resolve_storage_key(&config, secure_storage.as_deref())?;

        // Open or create storage
        let storage = if config.storage_path.exists() {
            Storage::open(&config.storage_path, storage_key)?.with_clock(clock.clone())
        } else {
            // Create parent directories if needed
            if let Some(parent) = config.storage_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| VauchiError::Configuration(e.to_string()))?;
            }
            Storage::open(&config.storage_path, storage_key)?.with_clock(clock.clone())
        };

        let events = Arc::new(EventDispatcher::new());

        // Try to load a persisted identity from storage
        let identity = match storage.load_identity() {
            Ok(Some((bytes, _display_name))) => {
                Identity::from_storage_bytes(&bytes, clock.unix_seconds()).ok()
            }
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
            clock,
            rng,
            sleeper,
            monotonic,
            #[cfg(feature = "network-http")]
            ohttp_key: None,
            #[cfg(feature = "network-http")]
            next_sync_allowed: None,
            #[cfg(feature = "network-http")]
            last_exchange_time: None,
            last_sync_unix_seconds: None,
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
        if let Some(ss) = secure_storage
            && let Some(smk_bytes) = ss.load_key(SMK_KEY_NAME).map_err(|e| {
                VauchiError::Configuration(format!("Failed to load SMK from SecureStorage: {}", e))
            })?
        {
            // F5 audit fix: wrap in Zeroizing so the Vec is cleared on drop
            let smk_bytes = zeroize::Zeroizing::new(smk_bytes);
            let smk_array: [u8; 32] = smk_bytes.as_slice().try_into().map_err(|_| {
                VauchiError::Configuration("SMK in SecureStorage has invalid length".into())
            })?;
            let smk = ShreddingMasterKey::from_bytes(smk_array);
            return Ok(smk.derive_sek());
        }

        // Fall back to config storage key or generate random
        Ok(config
            .storage_key
            .clone()
            .unwrap_or_else(SymmetricKey::generate))
    }

    /// Borrow the explicit-time seam. Tests pass a `FakeClock`;
    /// production wraps [`SystemClock::shared`]. See `Vauchi::new_with`.
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// The configured relay server URL. The link-mode builders embed it in
    /// the v2 bootstrap (ADR-050) so the peer knows where to reach our
    /// update channel, without exposing the whole `VauchiConfig`.
    pub fn relay_server_url(&self) -> &str {
        &self.config.relay.server_url
    }

    /// Wall-clock unix seconds of the last successful sync. `None` until
    /// the first sync completes after process start (in-memory only — no
    /// storage migration). Returns `None` in builds without `network-http`.
    pub fn last_sync_time(&self) -> Option<u64> {
        self.last_sync_unix_seconds
    }

    /// Borrow the explicit-randomness seam. Tests pass a
    /// `DeterministicRng`; production wraps [`OsSecureRng::shared`].
    /// See `Vauchi::new_with` / `Vauchi::in_memory_with_clock_and_rng`.
    pub fn rng(&self) -> &Arc<dyn SecureRng> {
        &self.rng
    }

    /// Borrow the explicit-suspension seam. Tests pass a
    /// `FakeSleeper`; production wraps [`SystemSleeper::shared`].
    /// See [`Vauchi::with_sleeper`].
    pub fn sleeper(&self) -> &Arc<dyn Sleeper> {
        &self.sleeper
    }

    /// Replace the `Sleeper` used by this `Vauchi`. The default
    /// (set in every constructor) is [`SystemSleeper::shared`] —
    /// real wall-clock suspension, required for the
    /// `BIOMETRIC_UNLOCK_MIN_DURATION` floor to actually defend
    /// against the duress side-channel.
    ///
    /// Calling this in a production binary erases that defense
    /// (a `FakeSleeper` returns instantly). The method is therefore
    /// a **test-only** seam — gate construction of the substitute
    /// behind `#[cfg(any(test, feature = "testing"))]` (the
    /// `FakeSleeper` type itself is already gated that way).
    #[must_use]
    pub fn with_sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    /// Borrow the explicit-monotonic-time seam. Tests inject a
    /// `FakeMonotonicClock`; production wraps
    /// [`SystemMonotonicClock::shared`]. See [`Vauchi::with_monotonic`].
    pub fn monotonic(&self) -> &Arc<dyn MonotonicClock> {
        &self.monotonic
    }

    /// Replace the [`MonotonicClock`] used by this `Vauchi`. The
    /// default (set in every constructor) is
    /// [`SystemMonotonicClock::shared`] — the real OS monotonic clock.
    ///
    /// Test-only seam: inject a `FakeMonotonicClock` (itself gated
    /// behind `feature = "testing"`) to advance session-timeout,
    /// deadline, and retry-window time deterministically without real
    /// wall-clock waits.
    #[must_use]
    pub fn with_monotonic(mut self, monotonic: Arc<dyn MonotonicClock>) -> Self {
        self.monotonic = monotonic;
        self
    }

    /// Returns the current Unix timestamp in seconds.
    ///
    /// First callsite migrated from `SystemTime::now` to
    /// `self.clock.unix_seconds()` — Phase 1 / Task 1.1 / Step 3
    /// of the pure-functional-core program plan. The remaining
    /// callsites under `vauchi-core/src/` migrate in follow-up MRs
    /// cluster by cluster.
    fn now_timestamp(&self) -> u64 {
        self.clock.unix_seconds()
    }

    /// Records a sync item for inter-device synchronization.
    ///
    /// Silently succeeds if:
    /// - No identity is loaded
    /// - No device registry exists
    /// - Only one device is registered (nothing to sync to)
    ///
    /// This is called automatically by mutation methods (card updates,
    /// contact deletions, visibility changes, etc.) so that all frontends
    /// — CLI, TUI, mobile — get sync recording for free.
    pub(crate) fn record_sync_item(&self, item: crate::sync::SyncItem) {
        let identity = match &self.identity {
            Some(id) => id,
            None => return,
        };

        let registry = match self.storage.load_device_registry() {
            Ok(Some(r)) if r.device_count() > 1 => r,
            _ => return,
        };

        let mut orchestrator = crate::api::sync::DeviceSyncOrchestrator::load(
            &self.storage,
            identity.create_device_info(self.clock.unix_seconds()),
            registry.clone(),
        )
        .unwrap_or_else(|_| {
            crate::api::sync::DeviceSyncOrchestrator::new(
                &self.storage,
                identity.create_device_info(self.clock.unix_seconds()),
                registry,
            )
        });

        // Best-effort — sync recording failures should not break mutations
        #[allow(clippy::let_underscore_must_use)]
        let _ = orchestrator.record_local_change(item);
    }

    /// Creates a new Vauchi instance with in-memory storage (for testing).
    pub fn in_memory() -> VauchiResult<Self> {
        Self::in_memory_with_clock_and_rng(SystemClock::shared(), OsSecureRng::shared())
    }

    /// In-memory Vauchi with an injected [`Clock`] (production
    /// [`SecureRng`]). Use for tests that need `FakeClock` but
    /// keep ambient OS entropy.
    pub fn in_memory_with_clock(clock: Arc<dyn Clock>) -> VauchiResult<Self> {
        Self::in_memory_with_clock_and_rng(clock, OsSecureRng::shared())
    }

    /// In-memory Vauchi with both seams injected — the deterministic
    /// triple referenced by Task 1.4 of the pure-functional-core
    /// program. Pass `FakeClock` + `DeterministicRng` for a fully
    /// deterministic `(state, input) -> (state, output)` test
    /// fixture.
    pub fn in_memory_with_clock_and_rng(
        clock: Arc<dyn Clock>,
        rng: Arc<dyn SecureRng>,
    ) -> VauchiResult<Self> {
        let storage_key = SymmetricKey::generate();
        let storage = Storage::in_memory(storage_key)?.with_clock(clock.clone());
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
            clock,
            rng,
            sleeper: SystemSleeper::shared(),
            monotonic: SystemMonotonicClock::shared(),
            #[cfg(feature = "network-http")]
            ohttp_key: None,
            #[cfg(feature = "network-http")]
            next_sync_allowed: None,
            #[cfg(feature = "network-http")]
            last_exchange_time: None,
            last_sync_unix_seconds: None,
        })
    }
}
