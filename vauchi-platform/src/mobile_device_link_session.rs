// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI adapter for the device-link orchestrator.
//!
//! Thin wrapper around
//! [`vauchi_app::orchestrator::device_link_session::DeviceLinkSession`].
//! All cycle-thread, listener-slot, persistence, and protocol logic
//! lives in vauchi-app — this module exposes the surface to UniFFI
//! consumers (iOS, macOS, Android) and adapts the platform's
//! `Box<dyn DeviceLinkSessionListener>` to vauchi-app's plain trait.
//!
//! Same surface as before the orchestrator extraction: every UniFFI
//! symbol that iOS/macOS/Android consume keeps identical name,
//! signature, and semantics.

use std::path::PathBuf;
use std::sync::Arc;

use vauchi_app::orchestrator::device_link_session as core_session;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::device_link::DeviceLinkInitiator;
use vauchi_core::network::HttpTransport;

use crate::error::MobileError;

// === Listener trait (UniFFI callback_interface) ===

/// Push-based callback interface for device-link session events.
///
/// Mirrored from `vauchi_app::orchestrator::device_link_session::DeviceLinkSessionListener`
/// because UniFFI's `callback_interface` macros must be applied at
/// the binding crate's level. Implementations on the Swift / Kotlin
/// side flow through this trait; the adapter below forwards calls
/// onto the plain-Rust trait that the cycle thread invokes.
///
/// See the underlying core trait for the callback contract and
/// threading rules.
#[uniffi::export(callback_interface)]
pub trait DeviceLinkSessionListener: Send + Sync {
    fn on_qr_ready(&self, qr_data: String, expires_at_unix: u64);

    fn on_confirmation_required(
        &self,
        device_name: String,
        confirmation_code: String,
        identity_fingerprint: String,
        proximity_challenge: Vec<u8>,
    );

    fn on_request_sent(&self, confirmation_code: String);

    fn on_completed(&self, device_name: String, device_index: u32);

    fn on_failed(&self, reason: String);

    fn on_session_ended(&self);
}

/// Adapter from the UniFFI-bound listener to the plain-Rust trait
/// the orchestrator consumes. Holds the consumer's
/// `Box<dyn DeviceLinkSessionListener>` and forwards every call
/// untouched.
struct UniffiListenerAdapter(Box<dyn DeviceLinkSessionListener>);

impl core_session::DeviceLinkSessionListener for UniffiListenerAdapter {
    fn on_qr_ready(&self, qr_data: String, expires_at_unix: u64) {
        self.0.on_qr_ready(qr_data, expires_at_unix);
    }

    fn on_confirmation_required(
        &self,
        device_name: String,
        confirmation_code: String,
        identity_fingerprint: String,
        proximity_challenge: Vec<u8>,
    ) {
        self.0.on_confirmation_required(
            device_name,
            confirmation_code,
            identity_fingerprint,
            proximity_challenge,
        );
    }

    fn on_request_sent(&self, confirmation_code: String) {
        self.0.on_request_sent(confirmation_code);
    }

    fn on_completed(&self, device_name: String, device_index: u32) {
        self.0.on_completed(device_name, device_index);
    }

    fn on_failed(&self, reason: String) {
        self.0.on_failed(reason);
    }

    fn on_session_ended(&self) {
        self.0.on_session_ended();
    }
}

// === Session struct (UniFFI Object) ===

/// UniFFI-bound device-link session handle.
///
/// Wraps `vauchi_app::orchestrator::device_link_session::DeviceLinkSession`.
/// All real work happens in the inner core session; this struct
/// exists to expose the lifecycle methods to UniFFI consumers.
#[derive(uniffi::Object)]
pub struct MobileDeviceLinkSession {
    inner: Arc<core_session::DeviceLinkSession>,
}

#[uniffi::export]
impl MobileDeviceLinkSession {
    /// Register or replace the session listener. Wraps the boxed
    /// UniFFI listener in an adapter and forwards to the inner
    /// session.
    pub fn set_listener(&self, listener: Box<dyn DeviceLinkSessionListener>) {
        self.inner
            .set_listener(Box::new(UniffiListenerAdapter(listener)));
    }

    /// Spawn the cycle thread. Idempotent.
    pub fn start(&self) {
        self.inner.start();
    }

    /// User confirmed the codes match (manual / non-ultrasonic
    /// path).
    pub fn confirm_manual(
        &self,
        confirmation_code: String,
        confirmed_at: u64,
    ) -> Result<(), MobileError> {
        self.inner
            .confirm_manual(confirmation_code, confirmed_at)
            .map_err(map_session_error)
    }

    /// User completed ultrasonic proximity verification.
    pub fn confirm_ultrasonic(
        &self,
        challenge_response: Vec<u8>,
        verified_at: u64,
    ) -> Result<(), MobileError> {
        self.inner
            .confirm_ultrasonic(challenge_response, verified_at)
            .map_err(map_session_error)
    }

    /// User denied the link.
    pub fn deny(&self) {
        self.inner.deny();
    }

    /// Cancel the session and join the cycle thread.
    pub fn cancel(&self) {
        self.inner.cancel();
    }
}

impl MobileDeviceLinkSession {
    /// Production constructor — used by
    /// `VauchiPlatform::create_device_link_session_initiator`.
    pub(crate) fn with_persistence_initiator(
        initiator: DeviceLinkInitiator,
        transport: HttpTransport,
        identity_id: String,
        relay_timeout_secs: u64,
        storage_path: PathBuf,
        storage_key: SymmetricKey,
    ) -> Self {
        Self {
            inner: Arc::new(core_session::DeviceLinkSession::with_persistence_initiator(
                initiator,
                transport,
                identity_id,
                relay_timeout_secs,
                storage_path,
                storage_key,
            )),
        }
    }

    /// Integration-test harness constructor.
    #[doc(hidden)]
    pub fn new_initiator_for_test(
        initiator: DeviceLinkInitiator,
        transport: HttpTransport,
        identity_id: String,
        relay_timeout_secs: u64,
    ) -> Self {
        Self {
            inner: Arc::new(core_session::DeviceLinkSession::new_initiator_for_test(
                initiator,
                transport,
                identity_id,
                relay_timeout_secs,
            )),
        }
    }

    /// Integration-test hook: shorten the user-action poll cadence.
    #[doc(hidden)]
    pub fn set_user_action_poll_override_ms_for_test(&self, override_ms: u32) {
        self.inner
            .set_user_action_poll_override_ms_for_test(override_ms);
    }

    /// Integration-test hook: cycle-thread liveness probe.
    #[doc(hidden)]
    pub fn cycle_thread_finished_for_test(&self) -> bool {
        self.inner.cycle_thread_finished_for_test()
    }
}

fn map_session_error(err: core_session::DeviceLinkSessionError) -> MobileError {
    match err {
        core_session::DeviceLinkSessionError::InvalidInput(detail) => MobileError::Other { detail },
    }
}
