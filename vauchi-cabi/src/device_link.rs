// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device linking C ABI — initiator protocol and relay transport.
//!
//! Mirrors the UniFFI exports in `vauchi-platform` for CABI consumers
//! (Windows, linux-qt).

use std::os::raw::c_char;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkRequest, ProximityProof, compute_confirmation_mac,
};
use vauchi_core::network::{HttpTransport, HttpTransportConfig, ProxyConfig};

use super::{VauchiApp, from_c_str, to_c_string};

/// Opaque handle to a device link initiator.
pub struct VauchiDeviceLinkInitiator {
    inner: Mutex<DeviceLinkInitiator>,
    pending_request: Mutex<Option<DeviceLinkRequest>>,
}

// ── Initiator lifecycle ───────────────────────────────────────────────

/// Start a device link as the existing device (initiator).
///
/// Creates an initiator from the app's identity and device registry.
/// Returns null if no identity exists or on error.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_start(
    handle: *mut VauchiApp,
) -> *mut VauchiDeviceLinkInitiator {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let app = unsafe { &*handle };
        let engine = match app.engine.lock() {
            Ok(e) => e,
            Err(_) => return std::ptr::null_mut(),
        };
        let vauchi = engine.vauchi();

        let identity = match vauchi.identity() {
            Some(id) => id,
            None => return std::ptr::null_mut(),
        };

        let registry = vauchi
            .storage()
            .load_device_registry()
            .ok()
            .flatten()
            .unwrap_or_else(|| identity.initial_device_registry());

        let initiator = identity.create_device_link_initiator(registry);

        Box::into_raw(Box::new(VauchiDeviceLinkInitiator {
            inner: Mutex::new(initiator),
            pending_request: Mutex::new(None),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy a device link initiator.
///
/// # Safety
/// `initiator` must be a pointer returned by `vauchi_device_link_start`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_initiator_destroy(
    initiator: *mut VauchiDeviceLinkInitiator,
) {
    unsafe {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !initiator.is_null() {
                drop(Box::from_raw(initiator));
            }
        }));
    }
}

/// Get the QR data string from the initiator.
///
/// # Safety
/// `initiator` must be a valid initiator handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_qr_data(
    initiator: *mut VauchiDeviceLinkInitiator,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if initiator.is_null() {
            return std::ptr::null_mut();
        }
        let init = unsafe { &*initiator };
        match init.inner.lock() {
            Ok(guard) => to_c_string(&guard.qr().to_data_string()),
            Err(_) => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Get the expiry timestamp (Unix seconds) of the QR code.
///
/// Returns 0 on error.
///
/// # Safety
/// `initiator` must be a valid initiator handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_expires_at(
    initiator: *mut VauchiDeviceLinkInitiator,
) -> u64 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if initiator.is_null() {
            return 0;
        }
        let init = unsafe { &*initiator };
        init.inner.lock().map(|g| g.qr().expires_at()).unwrap_or(0)
    }))
    .unwrap_or(0)
}

// ── Protocol operations ───────────────────────────────────────────────

/// Decrypt an incoming link request and return confirmation details.
///
/// `encrypted_request_b64` is the base64-encoded encrypted request from
/// the new device. Returns a JSON string:
/// `{"device_name":"...","confirmation_code":"...","identity_fingerprint":"..."}`
/// or `{"error":"..."}` on failure. Returns null on null inputs.
///
/// # Safety
/// `initiator` must be a valid initiator handle or null.
/// `encrypted_request_b64` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_prepare_confirmation(
    initiator: *mut VauchiDeviceLinkInitiator,
    encrypted_request_b64: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if initiator.is_null() {
            return std::ptr::null_mut();
        }
        let req_b64 = match from_c_str(encrypted_request_b64) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let encrypted = match BASE64.decode(&req_b64) {
            Ok(bytes) => bytes,
            Err(e) => return to_c_string(&format!(r#"{{"error":"base64 decode: {e}"}}"#)),
        };

        let init = unsafe { &*initiator };
        let guard = match init.inner.lock() {
            Ok(g) => g,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match guard.prepare_confirmation(&encrypted) {
            Ok((confirmation, request)) => {
                // Store pending request for confirm step
                if let Ok(mut pending) = init.pending_request.lock() {
                    *pending = Some(request);
                }
                let json = serde_json::json!({
                    "device_name": confirmation.device_name,
                    "confirmation_code": confirmation.confirmation_code,
                    "identity_fingerprint": confirmation.identity_fingerprint,
                });
                to_c_string(&json.to_string())
            }
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Confirm the device link with manual code verification.
///
/// Must call `vauchi_device_link_prepare_confirmation` first.
/// `confirmation_code` is the human-readable code (e.g. "123-456").
/// Rust computes the HMAC internally — the link key never crosses FFI.
/// `confirmed_at` is the Unix timestamp (seconds).
///
/// Returns JSON: `{"encrypted_response":"base64...","device_name":"...","device_index":N}`
/// or `{"error":"..."}`. Returns null on null inputs.
///
/// # Safety
/// `initiator` must be a valid initiator handle or null.
/// `confirmation_code` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_confirm_manual(
    initiator: *mut VauchiDeviceLinkInitiator,
    confirmation_code: *const c_char,
    confirmed_at: u64,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if initiator.is_null() {
            return std::ptr::null_mut();
        }
        let code = match from_c_str(confirmation_code) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        let init = unsafe { &*initiator };

        // Compute HMAC from confirmation code + link key
        let mac = {
            let guard = match init.inner.lock() {
                Ok(g) => g,
                Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
            };
            compute_confirmation_mac(guard.qr().link_key(), &code)
        };

        let proof = ProximityProof::ManualConfirmation {
            confirmation_code_mac: mac,
            confirmed_at,
        };

        // Take pending request
        let request = match init.pending_request.lock() {
            Ok(mut pending) => match pending.take() {
                Some(r) => r,
                None => {
                    return to_c_string(
                        r#"{"error":"no pending request — call prepare_confirmation first"}"#,
                    );
                }
            },
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        let guard = match init.inner.lock() {
            Ok(g) => g,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match guard.confirm_link(&request, &proof) {
            Ok((encrypted_response, _registry, device_info)) => {
                let json = serde_json::json!({
                    "encrypted_response": BASE64.encode(&encrypted_response),
                    "device_name": device_info.device_name(),
                    "device_index": device_info.device_index(),
                });
                to_c_string(&json.to_string())
            }
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

// ── Relay transport ───────────────────────────────────────────────────

fn ws_to_http(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("ws://") {
        format!("http://{rest}")
    } else {
        url.to_string()
    }
}

fn create_transport(relay_url: &str) -> HttpTransport {
    let http_url = ws_to_http(relay_url);
    HttpTransport::new(HttpTransportConfig {
        relay_url: http_url,
        timeout_ms: 10_000,
        proxy: ProxyConfig::None,
        allow_direct: true,
    })
}

/// Claim payload sent by the new device.
#[derive(serde::Serialize, serde::Deserialize)]
struct ClaimPayload {
    request: Vec<u8>,
    response_code: String,
}

/// Listen for an incoming device link request via relay (blocking).
///
/// Creates an exchange offer with the identity, then polls until the new
/// device claims it. Blocks up to `timeout_secs` seconds.
///
/// Returns JSON: `{"encrypted_payload":"base64...","sender_token":"..."}`
/// or `{"error":"..."}`. Returns null on null handle.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_listen(
    handle: *mut VauchiApp,
    timeout_secs: u64,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let app = unsafe { &*handle };

        // Extract identity_id and relay_url under lock, then release
        let (identity_id, relay_url) = {
            let engine = match app.engine.lock() {
                Ok(e) => e,
                Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
            };
            let vauchi = engine.vauchi();
            let identity = match vauchi.identity() {
                Some(id) => id,
                None => return to_c_string(r#"{"error":"no identity"}"#),
            };
            (
                hex::encode(identity.signing_public_key()),
                vauchi.config().relay.server_url.clone(),
            )
        };

        let transport = create_transport(&relay_url);

        // 1. Create offer with identity info
        let code = match transport
            .exchange_offer(&BASE64.encode(identity_id.as_bytes()), Some(timeout_secs))
        {
            Ok(c) => c,
            Err(e) => return to_c_string(&format!(r#"{{"error":"offer failed: {e}"}}"#)),
        };

        // 2. Poll until claimed
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        loop {
            if Instant::now() >= deadline {
                return to_c_string(r#"{"error":"timeout"}"#);
            }
            match transport.exchange_complete(&code) {
                Ok(Some(claim_b64)) => {
                    let claim_bytes = match BASE64.decode(&claim_b64) {
                        Ok(b) => b,
                        Err(e) => return to_c_string(&format!(r#"{{"error":"decode: {e}"}}"#)),
                    };
                    let claim: ClaimPayload = match serde_json::from_slice(&claim_bytes) {
                        Ok(c) => c,
                        Err(e) => return to_c_string(&format!(r#"{{"error":"parse: {e}"}}"#)),
                    };
                    let json = serde_json::json!({
                        "encrypted_payload": BASE64.encode(&claim.request),
                        "sender_token": claim.response_code,
                    });
                    return to_c_string(&json.to_string());
                }
                Ok(None) => {
                    thread::sleep(Duration::from_secs(1));
                }
                Err(e) => {
                    return to_c_string(&format!(r#"{{"error":"poll: {e}"}}"#));
                }
            }
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Send device link response back via relay.
///
/// Claims the return channel created by the new device.
/// Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `sender_token` and `encrypted_response_b64` must be valid null-terminated
/// C strings, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_send_response(
    handle: *mut VauchiApp,
    sender_token: *const c_char,
    encrypted_response_b64: *const c_char,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return -1;
        }
        let token = match from_c_str(sender_token) {
            Some(s) => s,
            None => return -1,
        };
        let resp_b64 = match from_c_str(encrypted_response_b64) {
            Some(s) => s,
            None => return -1,
        };
        let response_bytes = match BASE64.decode(&resp_b64) {
            Ok(b) => b,
            Err(_) => return -1,
        };

        let app = unsafe { &*handle };
        let relay_url = {
            let engine = match app.engine.lock() {
                Ok(e) => e,
                Err(_) => return -1,
            };
            engine.vauchi().config().relay.server_url.clone()
        };

        let transport = create_transport(&relay_url);
        match transport.exchange_claim(&token, &BASE64.encode(&response_bytes)) {
            Ok(_) => 0,
            Err(_) => -1,
        }
    }))
    .unwrap_or(-1)
}
