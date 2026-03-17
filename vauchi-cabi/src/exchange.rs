// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange session C ABI functions.

use std::os::raw::c_char;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::Zeroizing;

use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, ManualConfirmationVerifier,
    ProximityConfidence, ProximityError, ProximityVerifier, VerifierChain, VerifierMethod,
};
use vauchi_core::ContactCard;

use super::{from_c_str, to_c_string, VauchiApp, VauchiExchange};

/// Wrapper to share a `ManualConfirmationVerifier` via `Arc` while
/// implementing `ProximityVerifier` for the `VerifierChain`.
struct SharedManualVerifier(Arc<ManualConfirmationVerifier>);

impl ProximityVerifier for SharedManualVerifier {
    fn confidence_level(&self) -> ProximityConfidence {
        self.0.confidence_level()
    }
    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        self.0.emit_challenge(challenge)
    }
    fn listen_for_response(&self, timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        self.0.listen_for_response(timeout)
    }
    fn verify_response(&self, challenge: &[u8; 16], response: &[u8]) -> bool {
        self.0.verify_response(challenge, response)
    }
    fn verify_proximity_two_way(
        &self,
        emit_challenge: &[u8; 16],
        listen_challenge: &[u8; 16],
        timeout: Duration,
        is_initiator: bool,
    ) -> Result<(), ProximityError> {
        self.0
            .verify_proximity_two_way(emit_challenge, listen_challenge, timeout, is_initiator)
    }
}

/// Create a new QR exchange session using the app's identity.
///
/// Uses manual confirmation for proximity verification (suitable for
/// desktop platforms without audio proximity hardware).
///
/// Returns null if the app handle is null, identity is not created,
/// or initialization fails.
///
/// # Safety
/// `app` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_create(app: *mut VauchiApp) -> *mut VauchiExchange {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if app.is_null() {
            return std::ptr::null_mut();
        }
        let app_ref = &*app;
        let engine = match app_ref.engine.lock() {
            Ok(e) => e,
            Err(_) => return std::ptr::null_mut(),
        };

        let vauchi = engine.vauchi();
        let identity_ref = match vauchi.identity() {
            Some(id) => id,
            None => return std::ptr::null_mut(),
        };
        // Clone identity via storage serialization (ExchangeSession needs ownership).
        // Wrap in Zeroizing to scrub master_seed from heap on drop.
        let storage_bytes = Zeroizing::new(identity_ref.to_storage_bytes());
        let identity = match vauchi_core::identity::Identity::from_storage_bytes(&storage_bytes) {
            Ok(id) => id,
            Err(_) => return std::ptr::null_mut(),
        };
        let card = match vauchi.own_card() {
            Ok(Some(c)) => c,
            Ok(None) | Err(_) => return std::ptr::null_mut(),
        };

        let manual = Arc::new(ManualConfirmationVerifier::new());
        let mut chain = VerifierChain::new();
        chain.add(
            VerifierMethod::ManualConfirmation,
            Box::new(SharedManualVerifier(manual.clone())),
        );

        let session = ExchangeSession::new_qr(identity, card, chain);

        Box::into_raw(Box::new(VauchiExchange {
            session: Mutex::new(session),
            manual_verifier: manual,
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy an exchange session.
///
/// # Safety
/// `handle` must be a pointer returned by `vauchi_exchange_create`, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_destroy(handle: *mut VauchiExchange) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }));
}

/// Start QR generation and return the QR data string ("wb://...").
///
/// Returns error JSON if the session is in the wrong state.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_generate_qr(handle: *mut VauchiExchange) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        if let Err(e) = session.apply(ExchangeEvent::StartQR) {
            return to_c_string(&format!(r#"{{"error":"{}"}}"#, e));
        }

        match session.qr() {
            Some(qr) => to_c_string(&format!("wb://{}", qr.to_data_string())),
            None => to_c_string(r#"{"error":"QR not generated"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Process a scanned QR code from the peer.
///
/// `qr_data` should be the full QR string (with or without "wb://" prefix).
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
/// `qr_data` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_process_qr(
    handle: *mut VauchiExchange,
    qr_data: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let data = match from_c_str(qr_data) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null QR data"}"#),
        };

        let data_str = data.strip_prefix("wb://").unwrap_or(&data);
        let qr = match ExchangeQR::from_data_string(data_str) {
            Ok(q) => q,
            Err(_) => return to_c_string(r#"{"error":"invalid QR data"}"#),
        };

        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match session.apply(ExchangeEvent::ProcessQR(qr)) {
            Ok(()) => to_c_string(r#""ok""#),
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Helper: apply a simple event to an exchange session.
unsafe fn exchange_apply_event(handle: *mut VauchiExchange, event: ExchangeEvent) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match session.apply(event) {
            Ok(()) => to_c_string(r#""ok""#),
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Signal that the peer scanned our QR code.
///
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_they_scanned_our_qr(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    exchange_apply_event(handle, ExchangeEvent::TheyScannedOurQR)
}

/// Perform key agreement and proximity verification.
///
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_perform_key_agreement(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    exchange_apply_event(handle, ExchangeEvent::PerformKeyAgreement)
}

/// Complete the exchange with the peer's card name.
///
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
/// `their_name` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_complete(
    handle: *mut VauchiExchange,
    their_name: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let name = match from_c_str(their_name) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null name"}"#),
        };

        const MAX_NAME_LEN: usize = 256;
        if name.len() > MAX_NAME_LEN {
            return to_c_string(r#"{"error":"name too long"}"#);
        }

        let card = ContactCard::new(&name);
        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match session.apply(ExchangeEvent::CompleteExchange(card)) {
            Ok(()) => to_c_string(r#""ok""#),
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Confirm that the user verified proximity manually.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_confirm_proximity(handle: *mut VauchiExchange) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            let exchange = &*handle;
            exchange.manual_verifier.confirm();
        }
    }));
}

/// Get the current exchange state as a string label.
///
/// Returns one of: "idle", "displaying_qr", "peer_scanned",
/// "awaiting_key_agreement", "awaiting_card_exchange", "complete", "failed".
/// Returns null if the handle is null.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_state(handle: *mut VauchiExchange) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        let label = match session.state() {
            ExchangeState::Idle => "idle",
            ExchangeState::DisplayingQr { .. } => "displaying_qr",
            ExchangeState::PeerScanned { .. } => "peer_scanned",
            ExchangeState::AwaitingKeyAgreement { .. } => "awaiting_key_agreement",
            ExchangeState::AwaitingCardExchange { .. } => "awaiting_card_exchange",
            ExchangeState::AwaitingNfcTap => "awaiting_nfc_tap",
            ExchangeState::AwaitingBleConnection => "awaiting_ble_connection",
            ExchangeState::AwaitingBleVerification { .. } => "awaiting_ble_verification",
            ExchangeState::Complete { .. } => "complete",
            ExchangeState::Failed { .. } => "failed",
        };
        to_c_string(label)
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Check whether the exchange session has timed out.
///
/// Returns 1 if timed out, 0 if not, -1 on error.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_is_timed_out(handle: *mut VauchiExchange) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return -1;
        }
        let exchange = &*handle;
        match exchange.session.lock() {
            Ok(session) => i32::from(session.is_timed_out()),
            Err(_) => -1,
        }
    }))
    .unwrap_or(-1)
}

/// Get the peer's display name (from their QR code).
///
/// Returns the name string, or null if not yet known or handle is null.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_peer_display_name(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match session.their_display_name() {
            Some(name) => to_c_string(name),
            None => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Enable debug logging on the exchange session.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_enable_debug_log(handle: *mut VauchiExchange) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            let exchange = &*handle;
            if let Ok(mut session) = exchange.session.lock() {
                session.enable_debug_log();
            }
        }
    }));
}

/// Get the exchange debug log as JSONL.
///
/// Returns the JSONL string, or null if debug logging is not enabled.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_debug_jsonl(handle: *mut VauchiExchange) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match session.exchange_debug_log() {
            Some(log) => to_c_string(&log.to_jsonl()),
            None => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Get the exchange debug log as Markdown.
///
/// Returns the Markdown string, or null if debug logging is not enabled.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_debug_markdown(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match session.exchange_debug_log() {
            Some(log) => to_c_string(&log.to_markdown()),
            None => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}
