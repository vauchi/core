// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for the link-mode responder session.
//!
//! Wraps [`LinkResponderSession`] in a thread-safe handle and exposes the
//! callback-based listener interface frontends drive after the
//! [`crate::DeepLinkConsentEngine`] grant action navigates to
//! `AppScreen::DeepLinkResponder`.
//!
//! # Lifecycle
//!
//! ```text
//! let session = MobileLinkResponderSession::new(parsed, our_card)?;
//! session.set_listener(Box::new(my_listener));
//! session.start();              // spawns vauchi-link-responder-cycle
//! // Frontend dispatches RelayEscrow* commands from drain_pending_commands();
//! // hardware events flow back via apply_hardware_event(...).
//! // …
//! session.cancel();             // idempotent; joins cycle thread
//! ```
//!
//! All listener callbacks fire from the cycle thread, **not** the main /
//! UI thread. Consumers must marshal to their platform's UI thread
//! before touching UI state. Mirrors the threading contract documented
//! on [`crate::MultiStageSessionListener`].
//!
//! See `_private/docs/problems/2026-04-27-deep-link-responder-flow/`
//! for the full design.

use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::MobileError;
use crate::exchange::MobileCommand;
use crate::exchange::MobileEvent;

use vauchi_core::exchange::link_mode::{
    LinkModeError, ParsedLinkUrl, responder_respond_with_card_bytes,
};
use vauchi_core::exchange::link_responder::{
    LinkResponderFailureReason, LinkResponderSession, LinkResponderState,
};
use vauchi_core::sleeper::SystemSleeper;

/// UniFFI-friendly mirror of [`LinkResponderState`]. The cycle thread
/// emits one `on_state_changed` callback per transition.
#[derive(uniffi::Enum, Debug, Clone, PartialEq)]
pub enum MobileLinkResponderState {
    /// Deposits dispatched (2 × `RelayEscrowDeposit` + 1 × `RelayEscrowCheck`),
    /// waiting on `RelayEscrowReady` from the relay or for the polling
    /// deadline to expire.
    Polling,
    /// `RelayEscrowReady` arrived; `RelayEscrowRetrieve` dispatched;
    /// waiting on `RelayEscrowBlobReceived`.
    Retrieving,
}

impl MobileLinkResponderState {
    /// Map the core state to the public-surface enum, returning `None`
    /// for the terminal `Finalized` / `Failed` states (those drive
    /// `on_finalized` / `on_failed` instead).
    fn from_core(state: &LinkResponderState) -> Option<Self> {
        match state {
            LinkResponderState::Polling => Some(MobileLinkResponderState::Polling),
            LinkResponderState::Retrieving => Some(MobileLinkResponderState::Retrieving),
            LinkResponderState::Finalized { .. } | LinkResponderState::Failed(_) => None,
        }
    }
}

/// Why the responder cycle ended without a finalized contact.
#[derive(uniffi::Enum, Debug, Clone, PartialEq)]
pub enum MobileLinkResponderFailureReason {
    /// Polling exhausted the deadline without a `RelayEscrowReady`.
    PollingTimedOut,
    /// Card retrieved but symmetric decryption failed. The detail is
    /// the underlying error's `Display` so logs are useful.
    DecryptError { detail: String },
    /// `RelayEscrowFailed` arrived for our gate. Most often the same
    /// user re-opened a link they had already accepted (slot already
    /// occupied — see R2 in the investigation's risk register).
    DepositRejected,
    /// User-initiated cancel via the Polling screen's Cancel action,
    /// or the engine's `Drop` impl on navigate-back.
    Cancelled,
}

impl From<LinkResponderFailureReason> for MobileLinkResponderFailureReason {
    fn from(reason: LinkResponderFailureReason) -> Self {
        match reason {
            LinkResponderFailureReason::PollingTimedOut => Self::PollingTimedOut,
            LinkResponderFailureReason::DecryptError { detail } => Self::DecryptError { detail },
            LinkResponderFailureReason::DepositRejected => Self::DepositRejected,
            LinkResponderFailureReason::Cancelled => Self::Cancelled,
        }
    }
}

/// Push-based callback interface for link-mode responder events.
///
/// Frontends implement this trait (in Swift / Kotlin via UniFFI) and
/// register it with [`MobileLinkResponderSession::set_listener`] before
/// calling [`MobileLinkResponderSession::start`]. Once `start()` is
/// called, an internal `vauchi-link-responder-cycle` thread drives the
/// state machine clock and invokes these callbacks as state advances.
///
/// # Threading
///
/// Callbacks fire from the cycle thread, **not** the main / UI thread.
/// Consumers must marshal to their platform's UI thread before touching
/// UI state.
///
/// # Callback ordering contract
///
/// - Success path: `on_state_changed(*) → on_state_changed(Retrieving) → on_finalized → on_session_ended`
/// - Failure path: `on_state_changed(*) → on_failed(reason) → on_session_ended`
/// - Cancel path: `on_failed(Cancelled) → on_session_ended`
///
/// `on_finalized` and `on_failed` are mutually exclusive — exactly one
/// fires per session. `on_session_ended` is always last.
#[uniffi::export(callback_interface)]
pub trait LinkResponderSessionListener: Send + Sync {
    /// State machine transitioned. Fires once per actual transition;
    /// state values are the user-facing subset (`Polling`, `Retrieving`).
    fn on_state_changed(&self, state: MobileLinkResponderState);

    /// New commands the frontend should dispatch via its existing
    /// `CommandHandler` (or platform equivalent). The cycle
    /// thread emits these whenever the state machine adds entries
    /// to its pending queue. The frontend feeds the resulting hardware
    /// events back via [`MobileLinkResponderSession::apply_hardware_event`].
    fn on_commands(&self, commands: Vec<MobileCommand>);

    /// Exchange finalized successfully. `card_bytes` carries the
    /// decrypted card payload the frontend hands to its persistence
    /// layer. Fires exactly once on the success path, before
    /// [`on_session_ended`](Self::on_session_ended).
    fn on_finalized(&self, card_bytes: Vec<u8>);

    /// Exchange ended in failure. `reason` is typed so the frontend
    /// can render a specific toast / error state. Fires at most once
    /// before [`on_session_ended`](Self::on_session_ended).
    fn on_failed(&self, reason: MobileLinkResponderFailureReason);

    /// Session has fully ended — last callback. Always fires after
    /// `on_finalized` or `on_failed`, or as the sole callback after
    /// [`MobileLinkResponderSession::cancel`].
    fn on_session_ended(&self);
}

type ListenerSlot = Arc<Mutex<Option<Arc<dyn LinkResponderSessionListener>>>>;

/// Default polling interval (ms) the cycle thread sleeps between state
/// inspections. Short enough to stay responsive to cancellation, long
/// enough not to busy-loop.
const CYCLE_POLL_INTERVAL_MS: u64 = 200;

/// Default polling deadline — 5 minutes from `start()`. Past this
/// instant, an unfinished `Polling` state transitions to
/// `Failed(PollingTimedOut)`. Mirrors the parent record's R1.
const POLLING_DEADLINE_SECS: u64 = 300;

/// Link-mode responder session handle for mobile platforms.
#[derive(uniffi::Object)]
pub struct MobileLinkResponderSession {
    inner: Arc<Mutex<LinkResponderSession>>,
    listener: ListenerSlot,
    cancel_flag: Arc<AtomicBool>,
    thread_handle: Mutex<Option<JoinHandle<()>>>,
}

#[uniffi::export]
impl MobileLinkResponderSession {
    /// Create a new responder session for the given deep-link URL and
    /// the responder's own raw card-payload bytes.
    ///
    /// The session derives `EscrowKeys` via DH, encrypts `our_card_bytes`
    /// with the derived `card_key`, builds the two deposit commands +
    /// a `RelayEscrowCheck`, and lands in `Polling`.
    ///
    /// Returns a typed [`MobileError`] if the URL is malformed, the DH
    /// output is non-contributory (small-order point), or the AEAD
    /// encryption RNG fails.
    ///
    /// `set_listener` must be called before `start`; otherwise the
    /// cycle thread runs but every callback is dropped.
    #[uniffi::constructor]
    pub fn new(parsed_url: String, our_card_bytes: Vec<u8>) -> Result<Arc<Self>, MobileError> {
        let parsed = parse_url(&parsed_url)?;
        Self::from_parsed(&parsed, our_card_bytes)
    }

    /// Register (or replace) the event listener. Safe to call before or
    /// after [`start`](Self::start).
    pub fn set_listener(&self, listener: Box<dyn LinkResponderSessionListener>) {
        if let Ok(mut slot) = self.listener.lock() {
            *slot = Some(Arc::from(listener));
        }
    }

    /// Spawn the cycle thread. Idempotent — a second call while the
    /// thread is running is a no-op. The thread:
    ///
    /// 1. Drains pending commands → fires `on_commands(...)`.
    /// 2. Polls for state transitions and calls `on_state_changed(...)`
    ///    on every change.
    /// 3. Calls `on_finalized` / `on_failed` on terminal transitions.
    /// 4. Sleeps `CYCLE_POLL_INTERVAL_MS` between iterations until
    ///    cancellation or terminal state.
    /// 5. Calls `on_session_ended` exactly once and exits.
    pub fn start(&self) {
        let Ok(mut handle_slot) = self.thread_handle.lock() else {
            return;
        };
        if handle_slot.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        *handle_slot = None;
        self.cancel_flag.store(false, Ordering::Relaxed);

        let inner = Arc::clone(&self.inner);
        let listener = Arc::clone(&self.listener);
        let cancel_flag = Arc::clone(&self.cancel_flag);

        let spawn_result = thread::Builder::new()
            .name("vauchi-link-responder-cycle".into())
            .spawn(move || {
                cycle_loop(inner, listener, cancel_flag);
            });
        if let Ok(handle) = spawn_result {
            *handle_slot = Some(handle);
        }
    }

    /// Decoded gate-hash bytes — the relay address the responder is
    /// polling. Frontends use this to construct
    /// `RelayEscrowReady` / `RelayEscrowBlobReceived` /
    /// `RelayEscrowFailed` events with the matching `gate_hash`.
    pub fn gate_hash_bytes(&self) -> Vec<u8> {
        self.inner
            .lock()
            .map(|s| s.gate_hash_bytes())
            .unwrap_or_default()
    }

    /// Apply a hardware event from the relay layer. Threadsafe — the
    /// cycle thread holds the same `Mutex` so events serialize with
    /// inspection.
    pub fn apply_hardware_event(&self, event: MobileEvent) {
        let core_event = event.into();
        if let Ok(mut session) = self.inner.lock() {
            session.apply_hardware_event(core_event);
        }
    }

    /// Cancel the session. Sets the cancel flag, waits for the cycle
    /// thread to exit, and drops the listener. Idempotent.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);

        if let Ok(mut session) = self.inner.lock() {
            session.cancel();
        }

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

impl MobileLinkResponderSession {
    /// Construct a session directly from an already-parsed URL. Used by
    /// `PlatformAppEngine::ensure_link_responder_session` to avoid
    /// round-tripping the URL through the string form when the engine
    /// already holds a typed [`DeepLinkPayload`] / [`ParsedLinkUrl`].
    /// Not exposed via UniFFI; frontends call the public string-form
    /// constructor or — preferably — fetch the engine-owned session
    /// via `PlatformAppEngine::current_link_responder_session`.
    pub(crate) fn from_parsed(
        parsed: &ParsedLinkUrl,
        our_card_bytes: Vec<u8>,
    ) -> Result<Arc<Self>, MobileError> {
        let (keys, deposits) = responder_respond_with_card_bytes(parsed, &our_card_bytes)
            .map_err(map_link_mode_error)?;
        let deadline = Instant::now() + Duration::from_secs(POLLING_DEADLINE_SECS);
        let session = LinkResponderSession::new(keys, deposits, deadline);
        Ok(Arc::new(Self {
            inner: Arc::new(Mutex::new(session)),
            listener: Arc::new(Mutex::new(None)),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: Mutex::new(None),
        }))
    }
}

fn parse_url(url: &str) -> Result<ParsedLinkUrl, MobileError> {
    vauchi_core::exchange::link_mode::parse_link_url(url).ok_or_else(|| MobileError::Other {
        detail: "invalid link URL".into(),
    })
}

fn map_link_mode_error(err: LinkModeError) -> MobileError {
    MobileError::Other {
        detail: err.to_string(),
    }
}

/// Body of the cycle thread.
fn cycle_loop(
    inner: Arc<Mutex<LinkResponderSession>>,
    listener_slot: ListenerSlot,
    cancel_flag: Arc<AtomicBool>,
) {
    let mut prev_state: Option<MobileLinkResponderState> = None;
    let mut terminal_emitted = false;
    let sleeper = SystemSleeper::shared();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            // Surface the cancel as a terminal failure so the listener
            // sees the typed reason — `LinkResponderSession::cancel`
            // already flipped the inner state, but we must still emit
            // the callbacks before joining.
            break;
        }

        // Snapshot state + drain commands under the inner lock, then
        // release before firing callbacks (callbacks may call back
        // into the session via `apply_hardware_event` and would
        // deadlock if we held the lock).
        let (commands, current_state, terminal) = {
            let Ok(mut session) = inner.lock() else {
                break;
            };
            let commands = session.drain_pending_commands();
            let core_state = session.current_state().clone();
            let terminal = match &core_state {
                LinkResponderState::Finalized { card_bytes } => {
                    Some(Terminal::Finalized(card_bytes.clone()))
                }
                LinkResponderState::Failed(reason) => Some(Terminal::Failed(reason.clone().into())),
                _ => None,
            };
            let mobile_state = MobileLinkResponderState::from_core(&core_state);
            (commands, mobile_state, terminal)
        };

        let listener = listener_slot.lock().ok().and_then(|g| g.clone());

        // Emit pending commands (deposits + check on first iteration,
        // retrieve on first Polling → Retrieving transition).
        if !commands.is_empty()
            && let Some(listener) = listener.as_ref()
        {
            let mobile_commands: Vec<MobileCommand> =
                commands.into_iter().map(MobileCommand::from).collect();
            listener.on_commands(mobile_commands);
        }

        // Emit state-changed for transitions only.
        let state_changed = match (&prev_state, &current_state) {
            (None, Some(s)) => Some(s.clone()),
            (Some(prev), Some(s)) if prev != s => Some(s.clone()),
            _ => None,
        };
        if let (Some(listener), Some(state)) = (listener.as_ref(), state_changed.clone()) {
            listener.on_state_changed(state);
        }
        prev_state = current_state;

        // Terminal callbacks: on_finalized / on_failed, then on_session_ended.
        if let Some(terminal) = terminal {
            if !terminal_emitted && let Some(listener) = listener.as_ref() {
                match terminal {
                    Terminal::Finalized(bytes) => listener.on_finalized(bytes),
                    Terminal::Failed(reason) => listener.on_failed(reason),
                }
                listener.on_session_ended();
            }
            terminal_emitted = true;
            break;
        }

        // Tick the deadline, then sleep until the next iteration.
        if let Ok(mut session) = inner.lock() {
            session.tick(Instant::now());
        }

        sleeper.sleep(Duration::from_millis(CYCLE_POLL_INTERVAL_MS));
    }

    // If we broke out due to cancellation before the inner state
    // observed it, surface the terminal callbacks here — `cancel()`
    // already flipped the state via `LinkResponderSession::cancel`.
    if !terminal_emitted {
        let listener = listener_slot.lock().ok().and_then(|g| g.clone());
        if let Some(listener) = listener {
            // Re-read state in case `cancel()` raced with the loop.
            let final_reason = inner.lock().ok().and_then(|s| match s.current_state() {
                LinkResponderState::Failed(reason) => Some(reason.clone().into()),
                _ => None,
            });
            if let Some(reason) = final_reason {
                listener.on_failed(reason);
            }
            listener.on_session_ended();
        }
    }
}

enum Terminal {
    Finalized(Vec<u8>),
    Failed(MobileLinkResponderFailureReason),
}
