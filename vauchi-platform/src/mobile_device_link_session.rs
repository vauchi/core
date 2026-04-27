// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for the device-link orchestration session.
//!
//! Wraps the existing `DeviceLinkInitiator` building block in a
//! cycle-thread + listener-trait shape that mirrors
//! [`MobileMultiStageSession`](crate::MobileMultiStageSession) (G4 Phase
//! 2.5). Frontends register a [`DeviceLinkSessionListener`] and call
//! [`start`](MobileDeviceLinkSession::start); the session owns the
//! relay-poll loop, the QR-expiry deadline, and the user-confirm
//! gate, surfacing every transition through listener callbacks.
//!
//! # Lifecycle
//!
//! ```text
//! let session = vauchi.create_device_link_session_initiator()?;
//! session.set_listener(Box::new(my_listener));
//! session.start();
//! // when user taps "Codes Match":
//! session.confirm_manual(code, now_unix);
//! // on cancel / leaving the screen:
//! session.cancel();
//! ```
//!
//! All listener callbacks fire from a `vauchi-device-link-cycle`
//! thread; consumers must dispatch to their UI thread before touching
//! UI state. Callback ordering: `on_qr_ready` → optional
//! `on_confirmation_required` → terminal `on_completed` xor
//! `on_failed` → `on_session_ended` (always last, exactly once per
//! session).
//!
//! # Phase 1 scope (2026-04-26)
//!
//! Initiator-only. The 6-event listener trait keeps `on_request_sent`
//! in its surface for the future responder addition (no frontend
//! consumes the responder mobile path today — `finish_join` is only
//! exercised in `lib.rs` unit tests). Plan:
//! `_private/docs/problems/2026-04-25-device-link-orchestrator/implementation-plan.md`.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::device_link::DeviceLinkInitiator;
use vauchi_core::exchange::{ProximityProof, compute_confirmation_mac};
use vauchi_core::network::HttpTransport;
use vauchi_core::storage::Storage;

use vauchi_app::orchestrator::device_link_relay::{
    DeviceLinkError, create_offer, poll_for_claim, send_response,
};

use crate::error::MobileError;

// === Listener trait ===

/// Push-based callback interface for device-link session events.
///
/// Frontends implement this trait (in Swift/Kotlin via UniFFI) and
/// register it with [`MobileDeviceLinkSession::set_listener`] before
/// calling [`MobileDeviceLinkSession::start`]. Once `start()` is
/// called, the cycle thread drives the relay-poll loop and
/// user-confirm gate, invoking these callbacks as state advances.
///
/// # Threading
///
/// Callbacks fire from the cycle thread, **not** the main/UI thread.
/// Consumers must marshal to their platform's UI thread before
/// touching UI state.
///
/// # Callback contract (initiator side, Phase 1)
///
/// 1. [`on_qr_ready`](Self::on_qr_ready) — fires once after `start()`.
///    Carries the QR data string and Unix timestamp when the QR
///    expires (per ADR-035, `qr.timestamp + LINK_QR_EXPIRY_SECONDS`).
/// 2. [`on_confirmation_required`](Self::on_confirmation_required) —
///    fires when a peer claims the QR. Carries the device name,
///    confirmation code, identity fingerprint, and proximity
///    challenge bytes. The frontend displays these and waits for the
///    user to call [`confirm_manual`](MobileDeviceLinkSession::confirm_manual)
///    / [`confirm_ultrasonic`](MobileDeviceLinkSession::confirm_ultrasonic)
///    / [`deny`](MobileDeviceLinkSession::deny).
/// 3. [`on_completed`](Self::on_completed) **xor**
///    [`on_failed`](Self::on_failed) — terminal callback. Fires once.
/// 4. [`on_session_ended`](Self::on_session_ended) — always last,
///    fires exactly once per session lifetime regardless of exit
///    path (success / failure / cancel / expiry / user-deny).
///
/// # `on_request_sent` (responder, deferred)
///
/// Reserved for the responder-side flow once a frontend wires it.
/// Not fired by Phase 1 implementations.
#[uniffi::export(callback_interface)]
pub trait DeviceLinkSessionListener: Send + Sync {
    /// QR is ready for display. `expires_at_unix` is the protocol-
    /// defined expiry deadline.
    fn on_qr_ready(&self, qr_data: String, expires_at_unix: u64);

    /// Peer claimed the QR. Frontend displays `device_name` +
    /// `confirmation_code` and awaits the user's confirm/deny input.
    fn on_confirmation_required(
        &self,
        device_name: String,
        confirmation_code: String,
        identity_fingerprint: String,
        proximity_challenge: Vec<u8>,
    );

    /// Responder-side: request POSTed; waiting for the existing
    /// device's confirmation + response. Reserved for a future
    /// responder cycle thread; never fires from Phase 1 code.
    fn on_request_sent(&self, confirmation_code: String);

    /// Terminal success.
    fn on_completed(&self, device_name: String, device_index: u32);

    /// Terminal failure. `reason` is a stable identifier for known
    /// cases (`"qr_expired"`, `"user_denied"`, `"user_confirm_timeout"`,
    /// `"cancelled"`) or a free-form description for unexpected
    /// errors (relay, decode, proof-rejection).
    fn on_failed(&self, reason: String);

    /// Always-last callback. Fires exactly once per session
    /// lifetime. Mirrors `MultiStageSessionListener::on_session_ended`.
    fn on_session_ended(&self);
}

// === Persistence + transport plumbing ===

/// Persistence handle the cycle thread uses to save the updated
/// device registry on successful link.
///
/// Reuses the storage-path + key shape of
/// [`crate::multistage_exchange::PersistenceContext`] but stays
/// type-private to avoid coupling the two unrelated session modules.
#[derive(Clone)]
pub(crate) struct DeviceLinkPersistence {
    pub(crate) storage_path: PathBuf,
    pub(crate) storage_key: SymmetricKey,
}

/// User-action message sent from the UniFFI thread into the cycle
/// thread via a bounded channel of capacity 1.
#[derive(Debug)]
enum UserAction {
    ConfirmManual { code: String, at: u64 },
    ConfirmUltrasonic { response: Vec<u8>, at: u64 },
    Deny,
}

type ListenerSlot = Arc<Mutex<Option<Arc<dyn DeviceLinkSessionListener>>>>;

/// Default cycle iteration sleep when the test harness has not
/// installed an override.
const DEFAULT_USER_ACTION_POLL_MS: u64 = 250;

/// Default user-confirm window. After the confirmation prompt fires,
/// the user has this long to tap confirm/deny before the session
/// fails with `"user_confirm_timeout"`. Intentionally generous to
/// allow the user to read codes off both screens.
const DEFAULT_USER_CONFIRM_TIMEOUT_S: u64 = 60;

// === Session struct ===

/// Device-link session handle. See module docs for lifecycle.
#[derive(uniffi::Object)]
pub struct MobileDeviceLinkSession {
    /// Initiator state machine. `None` after the cycle thread takes
    /// it on `start()`; `Some` again is never reset (sessions are
    /// single-use — call `cancel()` and create a new session for a
    /// retry, mirroring G4).
    initiator: Mutex<Option<DeviceLinkInitiator>>,
    /// Relay transport. Built at construction time so the cycle
    /// thread does not need to hold a `Vauchi` instance, and moved
    /// into the thread on `start()`.
    transport: Mutex<Option<HttpTransport>>,
    /// Identity ID hex string used as the broker `exchange_offer`
    /// info field on the initiator side.
    identity_id: String,
    /// Relay listen budget in seconds (also bounds the QR expiry
    /// deadline; both default to ADR-035's 300 s).
    relay_timeout_secs: u64,
    listener: ListenerSlot,
    cancel_flag: Arc<AtomicBool>,
    thread_handle: Mutex<Option<JoinHandle<()>>>,
    /// User-action channel. Bounded capacity 1 — a duplicate tap
    /// returns `Err(TrySendError::Full)` and is silently ignored
    /// (idempotency at the orchestrator boundary).
    action_tx: SyncSender<UserAction>,
    action_rx: Mutex<Option<Receiver<UserAction>>>,
    persistence: Option<DeviceLinkPersistence>,
    /// Test-only sleep override (ms) for the user-action wait loop.
    /// `0` means production cadence (250 ms).
    user_action_poll_override_ms: Arc<AtomicU32>,
}

#[uniffi::export]
impl MobileDeviceLinkSession {
    /// Register or replace the session listener. Safe to call before
    /// or after `start()`; subsequent callbacks route to the most
    /// recently installed listener.
    pub fn set_listener(&self, listener: Box<dyn DeviceLinkSessionListener>) {
        if let Ok(mut slot) = self.listener.lock() {
            *slot = Some(Arc::from(listener));
        }
    }

    /// Spawn the cycle thread. Idempotent — a second call while the
    /// thread is running is a no-op. Without a registered listener
    /// the thread runs but every callback is dropped.
    pub fn start(&self) {
        let Ok(mut handle_slot) = self.thread_handle.lock() else {
            return;
        };
        if handle_slot.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        *handle_slot = None;

        // Take the initiator + transport + action_rx into the cycle
        // thread. After this point the session struct holds None for
        // these — start() is single-shot.
        let Ok(mut init_slot) = self.initiator.lock() else {
            return;
        };
        let Some(initiator) = init_slot.take() else {
            // Already started (or never constructed correctly); no-op.
            return;
        };
        drop(init_slot);

        let Ok(mut tx_slot) = self.transport.lock() else {
            return;
        };
        let Some(transport) = tx_slot.take() else {
            return;
        };
        drop(tx_slot);

        let Ok(mut rx_slot) = self.action_rx.lock() else {
            return;
        };
        let Some(action_rx) = rx_slot.take() else {
            return;
        };
        drop(rx_slot);

        self.cancel_flag.store(false, Ordering::Relaxed);

        let listener = Arc::clone(&self.listener);
        let cancel_flag = Arc::clone(&self.cancel_flag);
        let identity_id = self.identity_id.clone();
        let relay_timeout_secs = self.relay_timeout_secs;
        let persistence = self.persistence.clone();
        let user_action_poll_override = Arc::clone(&self.user_action_poll_override_ms);

        let spawn_result = thread::Builder::new()
            .name("vauchi-device-link-cycle".into())
            .spawn(move || {
                cycle_loop(
                    initiator,
                    transport,
                    identity_id,
                    relay_timeout_secs,
                    listener,
                    cancel_flag,
                    action_rx,
                    persistence,
                    user_action_poll_override,
                );
            });
        if let Ok(handle) = spawn_result {
            *handle_slot = Some(handle);
        }
    }

    /// User confirmed the codes match (manual / non-ultrasonic
    /// path). Sends a `ConfirmManual` action to the cycle thread; a
    /// duplicate tap returns Ok-but-no-effect (channel already
    /// full).
    pub fn confirm_manual(
        &self,
        confirmation_code: String,
        confirmed_at: u64,
    ) -> Result<(), MobileError> {
        let _ = self.action_tx.try_send(UserAction::ConfirmManual {
            code: confirmation_code,
            at: confirmed_at,
        });
        Ok(())
    }

    /// User completed ultrasonic proximity verification. Sends a
    /// `ConfirmUltrasonic` action to the cycle thread.
    pub fn confirm_ultrasonic(
        &self,
        challenge_response: Vec<u8>,
        verified_at: u64,
    ) -> Result<(), MobileError> {
        if challenge_response.len() != 16 {
            return Err(MobileError::Other {
                detail: "challenge_response must be exactly 16 bytes".into(),
            });
        }
        let _ = self.action_tx.try_send(UserAction::ConfirmUltrasonic {
            response: challenge_response,
            at: verified_at,
        });
        Ok(())
    }

    /// User denied the link (codes did not match, or rejected the
    /// request). Cycle thread emits `on_failed("user_denied")` then
    /// `on_session_ended()`.
    pub fn deny(&self) {
        let _ = self.action_tx.try_send(UserAction::Deny);
    }

    /// Cancel the session. Sets the cancellation flag, waits for
    /// the cycle thread to exit, drops the listener. Idempotent.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);

        // Send a Deny so a cycle thread waiting on action_rx wakes
        // up. The thread checks cancel_flag first and exits cleanly.
        let _ = self.action_tx.try_send(UserAction::Deny);

        let handle_opt = self
            .thread_handle
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(handle) = handle_opt {
            let _ = handle.join();
        }

        if let Ok(mut slot) = self.listener.lock() {
            *slot = None;
        }
    }
}

impl MobileDeviceLinkSession {
    /// Production constructor — used by
    /// `VauchiPlatform::create_device_link_session_initiator`. Holds
    /// the persistence context so the cycle thread can save the
    /// updated device registry on successful confirm.
    pub(crate) fn with_persistence_initiator(
        initiator: DeviceLinkInitiator,
        transport: HttpTransport,
        identity_id: String,
        relay_timeout_secs: u64,
        storage_path: PathBuf,
        storage_key: SymmetricKey,
    ) -> Self {
        Self::build_initiator(
            initiator,
            transport,
            identity_id,
            relay_timeout_secs,
            Some(DeviceLinkPersistence {
                storage_path,
                storage_key,
            }),
        )
    }

    fn build_initiator(
        initiator: DeviceLinkInitiator,
        transport: HttpTransport,
        identity_id: String,
        relay_timeout_secs: u64,
        persistence: Option<DeviceLinkPersistence>,
    ) -> Self {
        // Capacity 1 — duplicate user-action taps surface as
        // TrySendError::Full and are silently dropped. The cycle
        // thread is the sole receiver.
        let (action_tx, action_rx) = sync_channel::<UserAction>(1);
        Self {
            initiator: Mutex::new(Some(initiator)),
            transport: Mutex::new(Some(transport)),
            identity_id,
            relay_timeout_secs,
            listener: Arc::new(Mutex::new(None)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: Mutex::new(None),
            action_tx,
            action_rx: Mutex::new(Some(action_rx)),
            persistence,
            user_action_poll_override_ms: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Integration-test harness constructor. Mirrors G4's
    /// `MobileMultiStageSession::new(local_card)` shape: takes a
    /// pre-built `DeviceLinkInitiator` plus an `HttpTransport`
    /// (typically pointed at a dead/stub URL via
    /// `HttpTransportConfig::for_testing`), no persistence. Not part
    /// of the UniFFI surface — `_for_test` is the load-bearing
    /// contract with callers, and `DeviceLinkInitiator` /
    /// `HttpTransport` are not UniFFI types.
    #[doc(hidden)]
    pub fn new_initiator_for_test(
        initiator: DeviceLinkInitiator,
        transport: HttpTransport,
        identity_id: String,
        relay_timeout_secs: u64,
    ) -> Self {
        Self::build_initiator(initiator, transport, identity_id, relay_timeout_secs, None)
    }

    /// Integration-test hook: shorten the user-action poll cadence
    /// so listener tests do not have to wait the production 250 ms
    /// per iteration. `0` restores production behaviour. Exposed
    /// `pub` (rather than `pub(crate)`) because integration tests
    /// live in a separate crate.
    #[doc(hidden)]
    pub fn set_user_action_poll_override_ms_for_test(&self, override_ms: u32) {
        self.user_action_poll_override_ms
            .store(override_ms, Ordering::Relaxed);
    }

    /// Integration-test hook: returns true if the cycle thread has
    /// exited.
    #[doc(hidden)]
    pub fn cycle_thread_finished_for_test(&self) -> bool {
        self.thread_handle
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|h| h.is_finished()))
            .unwrap_or(true)
    }
}

// === Cycle loop ===

#[allow(clippy::too_many_arguments)]
fn cycle_loop(
    initiator: DeviceLinkInitiator,
    transport: HttpTransport,
    identity_id: String,
    relay_timeout_secs: u64,
    listener_slot: ListenerSlot,
    cancel_flag: Arc<AtomicBool>,
    action_rx: Receiver<UserAction>,
    persistence: Option<DeviceLinkPersistence>,
    user_action_poll_override_ms: Arc<AtomicU32>,
) {
    let mut session_ended_fired = false;

    let outcome = run_initiator_cycle(
        &initiator,
        &transport,
        &identity_id,
        relay_timeout_secs,
        &listener_slot,
        &cancel_flag,
        &action_rx,
        persistence.as_ref(),
        &user_action_poll_override_ms,
    );

    match outcome {
        CycleOutcome::Completed {
            device_name,
            device_index,
        } => {
            if let Some(listener) = current_listener(&listener_slot) {
                listener.on_completed(device_name, device_index);
            }
        }
        CycleOutcome::Failed(reason) => {
            if let Some(listener) = current_listener(&listener_slot) {
                listener.on_failed(reason);
            }
        }
        CycleOutcome::Cancelled => {
            // No terminal callback — cancel() was the user's
            // explicit signal that they no longer care about the
            // outcome. on_session_ended still fires below.
        }
    }

    if let Some(listener) = current_listener(&listener_slot) {
        listener.on_session_ended();
        session_ended_fired = true;
    }

    // Defensive: if the listener slot was poisoned the unwrap above
    // would have skipped on_session_ended. Nothing else we can do —
    // the listener handle is gone.
    let _ = session_ended_fired;
}

enum CycleOutcome {
    Completed {
        device_name: String,
        device_index: u32,
    },
    Failed(String),
    Cancelled,
}

#[allow(clippy::too_many_arguments)]
fn run_initiator_cycle(
    initiator: &DeviceLinkInitiator,
    transport: &HttpTransport,
    identity_id: &str,
    relay_timeout_secs: u64,
    listener_slot: &ListenerSlot,
    cancel_flag: &AtomicBool,
    action_rx: &Receiver<UserAction>,
    persistence: Option<&DeviceLinkPersistence>,
    user_action_poll_override_ms: &AtomicU32,
) -> CycleOutcome {
    let qr = initiator.qr();
    let qr_data = qr.to_data_string();
    let expires_at_unix = qr.expires_at();

    if let Some(listener) = current_listener(listener_slot) {
        listener.on_qr_ready(qr_data, expires_at_unix);
    }

    if cancel_flag.load(Ordering::Relaxed) {
        return CycleOutcome::Cancelled;
    }

    // Phase 1 — create offer + poll for claim.
    let broker_code = match create_offer(transport, identity_id, relay_timeout_secs) {
        Ok(code) => code,
        Err(e) => return CycleOutcome::Failed(format!("relay offer failed: {e}")),
    };
    let deadline = Instant::now() + Duration::from_secs(relay_timeout_secs);
    let (request_payload, sender_token) =
        match poll_for_claim(transport, &broker_code, deadline, cancel_flag) {
            Ok(pair) => pair,
            Err(DeviceLinkError::RequestTimeout) => {
                if cancel_flag.load(Ordering::Relaxed) {
                    return CycleOutcome::Cancelled;
                }
                return CycleOutcome::Failed("qr_expired".into());
            }
            Err(e) => return CycleOutcome::Failed(format!("relay poll failed: {e}")),
        };

    // Phase 2 — confirmation prompt.
    let (confirmation, request) = match initiator.prepare_confirmation(&request_payload) {
        Ok(pair) => pair,
        Err(e) => return CycleOutcome::Failed(format!("prepare_confirmation: {e}")),
    };
    let challenge = initiator.proximity_challenge();
    if let Some(listener) = current_listener(listener_slot) {
        listener.on_confirmation_required(
            confirmation.device_name.clone(),
            confirmation.confirmation_code.clone(),
            confirmation.identity_fingerprint.clone(),
            challenge.to_vec(),
        );
    }

    // Phase 3 — wait for user action with cancel observation.
    let action = match wait_for_user_action(
        action_rx,
        cancel_flag,
        Duration::from_secs(DEFAULT_USER_CONFIRM_TIMEOUT_S),
        user_action_poll_override_ms,
    ) {
        WaitOutcome::Action(a) => a,
        WaitOutcome::Cancelled => return CycleOutcome::Cancelled,
        WaitOutcome::Timeout => return CycleOutcome::Failed("user_confirm_timeout".into()),
    };

    let proof = match action {
        UserAction::ConfirmManual { code, at } => {
            let mac = compute_confirmation_mac(initiator.qr().link_key(), &code);
            ProximityProof::ManualConfirmation {
                confirmation_code_mac: mac,
                confirmed_at: at,
            }
        }
        UserAction::ConfirmUltrasonic { response, at } => {
            let bytes: [u8; 16] = match response.as_slice().try_into() {
                Ok(b) => b,
                Err(_) => {
                    return CycleOutcome::Failed(
                        "challenge_response must be exactly 16 bytes".into(),
                    );
                }
            };
            ProximityProof::Ultrasonic {
                challenge_response: bytes,
                verified_at: at,
            }
        }
        UserAction::Deny => return CycleOutcome::Failed("user_denied".into()),
    };

    // Phase 4 — confirm + persist + send response.
    let (encrypted_response, registry, device_info) = match initiator.confirm_link(&request, &proof)
    {
        Ok(triple) => triple,
        Err(e) => return CycleOutcome::Failed(format!("confirm_link: {e}")),
    };

    if let Some(ctx) = persistence {
        let storage = match Storage::open(&ctx.storage_path, ctx.storage_key.clone()) {
            Ok(s) => s,
            Err(e) => return CycleOutcome::Failed(format!("storage open: {e}")),
        };
        if let Err(e) = storage.save_device_registry(&registry) {
            return CycleOutcome::Failed(format!("save_device_registry: {e}"));
        }
    }

    if let Err(e) = send_response(transport, &sender_token, encrypted_response) {
        return CycleOutcome::Failed(format!("send_response: {e}"));
    }

    CycleOutcome::Completed {
        device_name: device_info.device_name().to_string(),
        device_index: device_info.device_index(),
    }
}

/// Outcome of waiting on `action_rx` with cancel + timeout
/// observation.
enum WaitOutcome {
    Action(UserAction),
    Cancelled,
    Timeout,
}

/// Poll the user-action channel in short slices so cancel is
/// observed promptly. Returns Cancelled if the cancel flag flips
/// during the wait, Timeout if `total_timeout` elapses before any
/// action arrives, Action otherwise.
fn wait_for_user_action(
    action_rx: &Receiver<UserAction>,
    cancel_flag: &AtomicBool,
    total_timeout: Duration,
    poll_override_ms: &AtomicU32,
) -> WaitOutcome {
    let slice = match poll_override_ms.load(Ordering::Relaxed) {
        0 => Duration::from_millis(DEFAULT_USER_ACTION_POLL_MS),
        ms => Duration::from_millis(ms.into()),
    };

    let deadline = Instant::now() + total_timeout;
    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            return WaitOutcome::Cancelled;
        }
        let now = Instant::now();
        if now >= deadline {
            return WaitOutcome::Timeout;
        }
        let remaining = deadline.saturating_duration_since(now);
        let wait = slice.min(remaining);
        match action_rx.recv_timeout(wait) {
            Ok(action) => {
                if cancel_flag.load(Ordering::Relaxed) {
                    return WaitOutcome::Cancelled;
                }
                return WaitOutcome::Action(action);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // Sender dropped — only happens if the session is
                // dropped mid-flight, which shouldn't occur because
                // the session owns both ends. Treat as cancellation.
                return WaitOutcome::Cancelled;
            }
        }
    }
}

fn current_listener(slot: &ListenerSlot) -> Option<Arc<dyn DeviceLinkSessionListener>> {
    slot.lock().ok().and_then(|guard| guard.clone())
}
