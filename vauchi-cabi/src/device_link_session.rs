// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link session C ABI — listener-driven orchestrator.
//!
//! Mirrors `vauchi-platform`'s `MobileDeviceLinkSession` for CABI
//! consumers (Windows, linux-qt). Wraps
//! `vauchi_app::orchestrator::device_link_session::DeviceLinkSession`
//! and adapts the platform's Box<dyn Listener> shape onto a struct of
//! C function pointers + opaque `user_data`. The cycle thread,
//! listener slot, and protocol logic all live in vauchi-app — this
//! module is the C-ABI seam.
//!
//! # Lifecycle
//!
//! ```c
//! VauchiDeviceLinkSession *s = vauchi_device_link_session_create(app);
//! VauchiDeviceLinkListener listener = { /* function pointers + user_data */ };
//! vauchi_device_link_session_set_listener(s, listener);
//! vauchi_device_link_session_start(s);
//! /* on user tap: */
//! vauchi_device_link_session_confirm_manual(s, code, now);
//! /* on screen close: */
//! vauchi_device_link_session_cancel(s);
//! vauchi_device_link_session_destroy(s);
//! ```
//!
//! All listener callbacks fire from the
//! `vauchi-device-link-cycle` thread. C# / C++ consumers must marshal
//! to their UI thread before touching UI state (see
//! `windows/Vauchi/DeviceLinkBridge.cs` for the
//! `DispatcherQueue.TryEnqueue` pattern).

use std::ffi::{CString, c_void};
use std::os::raw::c_char;
use std::sync::Arc;

use vauchi_app::orchestrator::device_link_session::{DeviceLinkSession, DeviceLinkSessionListener};

use super::{VauchiApp, from_c_str};

// === Listener struct (C ABI) ===

/// C-callable listener for device-link session events.
///
/// Each field is an `Option<unsafe extern "C" fn(...)>` so consumers
/// may leave individual callbacks null. `user_data` is an opaque
/// pointer forwarded to every callback unchanged. The Rust side
/// never inspects or frees `user_data` — its lifetime is the
/// caller's responsibility (typically a `GCHandle` on .NET, a
/// `void*` to a C++ struct, etc).
///
/// # String/byte ownership
///
/// All `*const c_char` and `*const u8` arguments are valid only for
/// the duration of the callback invocation. The Rust side
/// constructs a temporary `CString` / borrows from a `Vec<u8>` and
/// drops it once the callback returns; consumers must copy bytes
/// they need to retain.
#[repr(C)]
pub struct VauchiDeviceLinkListener {
    pub on_qr_ready: Option<
        unsafe extern "C" fn(qr_data: *const c_char, expires_at_unix: u64, user_data: *mut c_void),
    >,
    pub on_confirmation_required: Option<
        unsafe extern "C" fn(
            device_name: *const c_char,
            confirmation_code: *const c_char,
            identity_fingerprint: *const c_char,
            proximity_challenge: *const u8,
            proximity_challenge_len: usize,
            user_data: *mut c_void,
        ),
    >,
    pub on_request_sent:
        Option<unsafe extern "C" fn(confirmation_code: *const c_char, user_data: *mut c_void)>,
    pub on_completed: Option<
        unsafe extern "C" fn(device_name: *const c_char, device_index: u32, user_data: *mut c_void),
    >,
    pub on_failed: Option<unsafe extern "C" fn(reason: *const c_char, user_data: *mut c_void)>,
    pub on_session_ended: Option<unsafe extern "C" fn(user_data: *mut c_void)>,
    pub user_data: *mut c_void,
}

// SAFETY: callers of `vauchi_device_link_session_set_listener`
// guarantee that the function pointers and `user_data` remain valid
// + thread-safe for the session lifetime. Mirrors the
// `EventCallbackHandler` pattern in `app.rs`.
unsafe impl Send for VauchiDeviceLinkListener {}
unsafe impl Sync for VauchiDeviceLinkListener {}

// === Adapter (C-listener → core trait) ===

/// Adapter that forwards each [`DeviceLinkSessionListener`] call onto
/// the C function-pointer set. Held inside an `Arc<dyn ...>` by the
/// inner session's listener slot.
struct CallbackAdapter(VauchiDeviceLinkListener);

impl DeviceLinkSessionListener for CallbackAdapter {
    fn on_qr_ready(&self, qr_data: String, expires_at_unix: u64) {
        if let Some(cb) = self.0.on_qr_ready
            && let Ok(c_qr) = CString::new(qr_data)
        {
            unsafe { cb(c_qr.as_ptr(), expires_at_unix, self.0.user_data) };
        }
    }

    fn on_confirmation_required(
        &self,
        device_name: String,
        confirmation_code: String,
        identity_fingerprint: String,
        proximity_challenge: Vec<u8>,
    ) {
        let Some(cb) = self.0.on_confirmation_required else {
            return;
        };
        let Ok(c_name) = CString::new(device_name) else {
            return;
        };
        let Ok(c_code) = CString::new(confirmation_code) else {
            return;
        };
        let Ok(c_fp) = CString::new(identity_fingerprint) else {
            return;
        };
        unsafe {
            cb(
                c_name.as_ptr(),
                c_code.as_ptr(),
                c_fp.as_ptr(),
                proximity_challenge.as_ptr(),
                proximity_challenge.len(),
                self.0.user_data,
            )
        };
    }

    fn on_request_sent(&self, confirmation_code: String) {
        if let Some(cb) = self.0.on_request_sent
            && let Ok(c_code) = CString::new(confirmation_code)
        {
            unsafe { cb(c_code.as_ptr(), self.0.user_data) };
        }
    }

    fn on_completed(&self, device_name: String, device_index: u32) {
        if let Some(cb) = self.0.on_completed
            && let Ok(c_name) = CString::new(device_name)
        {
            unsafe { cb(c_name.as_ptr(), device_index, self.0.user_data) };
        }
    }

    fn on_failed(&self, reason: String) {
        if let Some(cb) = self.0.on_failed
            && let Ok(c_reason) = CString::new(reason)
        {
            unsafe { cb(c_reason.as_ptr(), self.0.user_data) };
        }
    }

    fn on_session_ended(&self) {
        if let Some(cb) = self.0.on_session_ended {
            unsafe { cb(self.0.user_data) };
        }
    }
}

// === Session opaque handle ===

/// Opaque handle to a device-link session.
///
/// Wraps `Arc<DeviceLinkSession>`; the inner session's cycle thread
/// holds a clone of the listener `Arc` so callbacks remain live as
/// long as the session is alive.
pub struct VauchiDeviceLinkSession {
    inner: Arc<DeviceLinkSession>,
}

// === Lifecycle exports ===

/// Create a device-link session as the existing device (initiator).
///
/// Returns null on null `handle`, on missing identity, on
/// storage-key absence, or on any panic. Caller takes ownership and
/// must call [`vauchi_device_link_session_destroy`] exactly once.
///
/// # Safety
///
/// `handle` must be a valid `VauchiApp` pointer or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_create(
    handle: *mut VauchiApp,
) -> *mut VauchiDeviceLinkSession {
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
        let identity_id = hex::encode(identity.signing_public_key());

        let relay_url = vauchi.config().relay.server_url.clone();
        let transport = vauchi.build_relay_transport(relay_url, 10_000);

        let storage_path = vauchi.config().storage_path.clone();
        let storage_key = match vauchi.config().storage_key.clone() {
            Some(k) => k,
            None => return std::ptr::null_mut(),
        };

        // ADR-035: device-link QR expiry is 300 s. Same value as the
        // relay-listen budget so the cycle thread's deadline aligns
        // with the QR expiry observed by the peer.
        const RELAY_TIMEOUT_SECS: u64 = 300;

        let session = DeviceLinkSession::with_persistence_initiator(
            initiator,
            transport,
            identity_id,
            RELAY_TIMEOUT_SECS,
            storage_path,
            storage_key,
        );

        Box::into_raw(Box::new(VauchiDeviceLinkSession {
            inner: Arc::new(session),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Register or replace the session listener.
///
/// Wraps the C-callable listener struct in an adapter that
/// implements the orchestrator's plain-Rust trait, then forwards to
/// the inner session. Safe to call before or after
/// [`vauchi_device_link_session_start`]; subsequent callbacks route
/// to the most recently installed listener.
///
/// No-op on null `session`.
///
/// # Safety
///
/// `session` must be a valid pointer returned by
/// [`vauchi_device_link_session_create`] or null. The function
/// pointers in `listener` (when non-null) must remain valid +
/// thread-safe for the session lifetime, and `user_data` must
/// remain valid + thread-safe for the duration of every callback
/// invocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_set_listener(
    session: *mut VauchiDeviceLinkSession,
    listener: VauchiDeviceLinkListener,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session.is_null() {
            return;
        }
        let s = unsafe { &*session };
        s.inner.set_listener(Box::new(CallbackAdapter(listener)));
    }));
}

/// Spawn the cycle thread. Idempotent — a second call while the
/// thread is running is a no-op.
///
/// No-op on null `session`.
///
/// # Safety
///
/// `session` must be a valid pointer returned by
/// [`vauchi_device_link_session_create`] or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_start(session: *mut VauchiDeviceLinkSession) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session.is_null() {
            return;
        }
        let s = unsafe { &*session };
        s.inner.start();
    }));
}

/// User confirmed the codes match (manual / non-ultrasonic path).
///
/// Returns 0 on success, -1 if `session` or `confirmation_code` is
/// null.
///
/// # Safety
///
/// `session` must be a valid pointer or null. `confirmation_code`
/// must be a valid null-terminated C string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_confirm_manual(
    session: *mut VauchiDeviceLinkSession,
    confirmation_code: *const c_char,
    confirmed_at: u64,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session.is_null() {
            return -1;
        }
        let code = match from_c_str(confirmation_code) {
            Some(s) => s,
            None => return -1,
        };
        let s = unsafe { &*session };
        match s.inner.confirm_manual(code, confirmed_at) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }))
    .unwrap_or(-1)
}

/// User completed ultrasonic proximity verification.
///
/// Returns 0 on success, -1 on null pointer, -2 on length validation
/// failure (challenge_response must be exactly 16 bytes).
///
/// # Safety
///
/// `session` must be a valid pointer or null. If
/// `challenge_response_len > 0` then `challenge_response` must point
/// to at least that many bytes of valid memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_confirm_ultrasonic(
    session: *mut VauchiDeviceLinkSession,
    challenge_response: *const u8,
    challenge_response_len: usize,
    verified_at: u64,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session.is_null() || challenge_response.is_null() {
            return -1;
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(challenge_response, challenge_response_len) }
                .to_vec();
        let s = unsafe { &*session };
        match s.inner.confirm_ultrasonic(bytes, verified_at) {
            Ok(()) => 0,
            Err(_) => -2,
        }
    }))
    .unwrap_or(-1)
}

/// User denied the link.
///
/// No-op on null `session`.
///
/// # Safety
///
/// `session` must be a valid pointer or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_deny(session: *mut VauchiDeviceLinkSession) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session.is_null() {
            return;
        }
        let s = unsafe { &*session };
        s.inner.deny();
    }));
}

/// Cancel the session and join the cycle thread.
///
/// No-op on null `session`. Idempotent — safe to call multiple
/// times.
///
/// # Safety
///
/// `session` must be a valid pointer or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_cancel(session: *mut VauchiDeviceLinkSession) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session.is_null() {
            return;
        }
        let s = unsafe { &*session };
        s.inner.cancel();
    }));
}

/// Destroy the session and free all associated resources.
///
/// Calls `cancel()` first so the cycle thread joins before the
/// session is deallocated.
///
/// No-op on null `session`. Each session must be destroyed exactly
/// once.
///
/// # Safety
///
/// `session` must be a pointer returned by
/// [`vauchi_device_link_session_create`] or null. After this call,
/// `session` is invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_device_link_session_destroy(session: *mut VauchiDeviceLinkSession) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session.is_null() {
            return;
        }
        let boxed = unsafe { Box::from_raw(session) };
        boxed.inner.cancel();
        // boxed dropped here; Arc<DeviceLinkSession> goes to zero
        // refcount once the cycle thread (already joined by cancel)
        // has released its own clone, freeing the listener adapter.
    }));
}

// INLINE_TEST_REQUIRED: tests verify the C-callback adapter's
// marshalling behaviour (CString construction, byte-slice handling,
// null-callback safety, user_data pass-through) without needing a
// full VauchiApp — the lifecycle exports get integration coverage
// via Phase 3's Windows DeviceLinkBridge tests.
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::sync::Mutex;

    /// (device_name, confirmation_code, identity_fingerprint,
    /// proximity_challenge) — the four arguments of
    /// `on_confirmation_required` packed into one record so the
    /// Recorder field stays under the clippy `type_complexity` cap.
    type ConfirmationRequiredRecord = (String, String, String, Vec<u8>);

    /// Recorder accessed from the C-trampoline callbacks via
    /// `user_data`. Mirrors the `RecordingListener` pattern from the
    /// vauchi-platform listener tests.
    #[derive(Default)]
    struct Recorder {
        qr_ready: Mutex<Vec<(String, u64)>>,
        confirmation_required: Mutex<Vec<ConfirmationRequiredRecord>>,
        request_sent: Mutex<Vec<String>>,
        completed: Mutex<Vec<(String, u32)>>,
        failed: Mutex<Vec<String>>,
        session_ended_count: Mutex<u32>,
    }

    fn install(recorder: &Recorder) -> VauchiDeviceLinkListener {
        let user_data = recorder as *const Recorder as *mut c_void;
        VauchiDeviceLinkListener {
            on_qr_ready: Some(rec_qr_ready),
            on_confirmation_required: Some(rec_confirmation_required),
            on_request_sent: Some(rec_request_sent),
            on_completed: Some(rec_completed),
            on_failed: Some(rec_failed),
            on_session_ended: Some(rec_session_ended),
            user_data,
        }
    }

    fn cstr(ptr: *const c_char) -> String {
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    unsafe fn recorder_from(user_data: *mut c_void) -> &'static Recorder {
        unsafe { &*(user_data as *const Recorder) }
    }

    extern "C" fn rec_qr_ready(qr: *const c_char, expires: u64, user_data: *mut c_void) {
        let r = unsafe { recorder_from(user_data) };
        r.qr_ready.lock().unwrap().push((cstr(qr), expires));
    }

    extern "C" fn rec_confirmation_required(
        device_name: *const c_char,
        confirmation_code: *const c_char,
        identity_fingerprint: *const c_char,
        proximity_challenge: *const u8,
        proximity_challenge_len: usize,
        user_data: *mut c_void,
    ) {
        let r = unsafe { recorder_from(user_data) };
        let challenge =
            unsafe { std::slice::from_raw_parts(proximity_challenge, proximity_challenge_len) }
                .to_vec();
        r.confirmation_required.lock().unwrap().push((
            cstr(device_name),
            cstr(confirmation_code),
            cstr(identity_fingerprint),
            challenge,
        ));
    }

    extern "C" fn rec_request_sent(code: *const c_char, user_data: *mut c_void) {
        let r = unsafe { recorder_from(user_data) };
        r.request_sent.lock().unwrap().push(cstr(code));
    }

    extern "C" fn rec_completed(name: *const c_char, index: u32, user_data: *mut c_void) {
        let r = unsafe { recorder_from(user_data) };
        r.completed.lock().unwrap().push((cstr(name), index));
    }

    extern "C" fn rec_failed(reason: *const c_char, user_data: *mut c_void) {
        let r = unsafe { recorder_from(user_data) };
        r.failed.lock().unwrap().push(cstr(reason));
    }

    extern "C" fn rec_session_ended(user_data: *mut c_void) {
        let r = unsafe { recorder_from(user_data) };
        *r.session_ended_count.lock().unwrap() += 1;
    }

    // ── Forwarding correctness ────────────────────────────────────

    // @scenario: device_link:CABI listener forwards on_qr_ready
    #[test]
    fn callback_adapter_forwards_qr_ready() {
        let recorder = Recorder::default();
        let adapter = CallbackAdapter(install(&recorder));

        adapter.on_qr_ready("test-qr-data".to_string(), 1_700_000_000);

        let captured = recorder.qr_ready.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "test-qr-data");
        assert_eq!(captured[0].1, 1_700_000_000);
    }

    // @scenario: device_link:CABI listener forwards on_confirmation_required with byte payload
    #[test]
    fn callback_adapter_forwards_confirmation_required_with_bytes() {
        let recorder = Recorder::default();
        let adapter = CallbackAdapter(install(&recorder));

        let challenge = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0xFF];
        adapter.on_confirmation_required(
            "Pixel 7".to_string(),
            "123-456".to_string(),
            "fp-deadbeef".to_string(),
            challenge.clone(),
        );

        let captured = recorder.confirmation_required.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "Pixel 7");
        assert_eq!(captured[0].1, "123-456");
        assert_eq!(captured[0].2, "fp-deadbeef");
        assert_eq!(
            captured[0].3, challenge,
            "byte payload must round-trip through the C boundary unchanged"
        );
    }

    // @scenario: device_link:CABI listener forwards on_completed
    #[test]
    fn callback_adapter_forwards_completed() {
        let recorder = Recorder::default();
        let adapter = CallbackAdapter(install(&recorder));

        adapter.on_completed("MacBook Pro".to_string(), 2);

        let captured = recorder.completed.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "MacBook Pro");
        assert_eq!(captured[0].1, 2);
    }

    // @scenario: device_link:CABI listener forwards on_failed
    #[test]
    fn callback_adapter_forwards_failed() {
        let recorder = Recorder::default();
        let adapter = CallbackAdapter(install(&recorder));

        adapter.on_failed("qr_expired".to_string());

        let captured = recorder.failed.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0], "qr_expired");
    }

    // @scenario: device_link:CABI listener counts on_session_ended
    #[test]
    fn callback_adapter_forwards_session_ended() {
        let recorder = Recorder::default();
        let adapter = CallbackAdapter(install(&recorder));

        adapter.on_session_ended();
        adapter.on_session_ended();

        assert_eq!(*recorder.session_ended_count.lock().unwrap(), 2);
    }

    // @scenario: device_link:CABI listener forwards on_request_sent
    #[test]
    fn callback_adapter_forwards_request_sent() {
        let recorder = Recorder::default();
        let adapter = CallbackAdapter(install(&recorder));

        adapter.on_request_sent("ack-789".to_string());

        let captured = recorder.request_sent.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0], "ack-789");
    }

    // ── Null-callback safety ──────────────────────────────────────

    // @scenario: device_link:CABI listener with all null callbacks is no-op
    #[test]
    fn callback_adapter_with_all_null_callbacks_is_noop() {
        // Recorder is plumbed via user_data so that if any
        // `if let Some(cb)` guard regressed and accidentally invoked
        // a stale function pointer, we would observe a side-effect.
        // With every callback field None, every method must early-
        // return — the recorder stays empty.
        let recorder = Recorder::default();
        let listener = VauchiDeviceLinkListener {
            on_qr_ready: None,
            on_confirmation_required: None,
            on_request_sent: None,
            on_completed: None,
            on_failed: None,
            on_session_ended: None,
            user_data: &recorder as *const Recorder as *mut c_void,
        };
        let adapter = CallbackAdapter(listener);

        adapter.on_qr_ready("ignored".to_string(), 0);
        adapter.on_confirmation_required("n".into(), "c".into(), "f".into(), vec![1, 2, 3]);
        adapter.on_request_sent("c".into());
        adapter.on_completed("d".into(), 1);
        adapter.on_failed("r".into());
        adapter.on_session_ended();

        assert_eq!(
            recorder.qr_ready.lock().unwrap().len(),
            0,
            "no callback fires when on_qr_ready field is None"
        );
        assert_eq!(
            recorder.confirmation_required.lock().unwrap().len(),
            0,
            "no callback fires when on_confirmation_required field is None"
        );
        assert_eq!(
            recorder.request_sent.lock().unwrap().len(),
            0,
            "no callback fires when on_request_sent field is None"
        );
        assert_eq!(
            recorder.completed.lock().unwrap().len(),
            0,
            "no callback fires when on_completed field is None"
        );
        assert_eq!(
            recorder.failed.lock().unwrap().len(),
            0,
            "no callback fires when on_failed field is None"
        );
        assert_eq!(
            *recorder.session_ended_count.lock().unwrap(),
            0,
            "no callback fires when on_session_ended field is None"
        );
    }

    // @scenario: device_link:CABI listener with selective nulls only fires non-null callbacks
    #[test]
    fn callback_adapter_with_selective_nulls_fires_only_set_callbacks() {
        let recorder = Recorder::default();
        // Only on_qr_ready and on_session_ended set; rest are None.
        let listener = VauchiDeviceLinkListener {
            on_qr_ready: Some(rec_qr_ready),
            on_confirmation_required: None,
            on_request_sent: None,
            on_completed: None,
            on_failed: None,
            on_session_ended: Some(rec_session_ended),
            user_data: &recorder as *const Recorder as *mut c_void,
        };
        let adapter = CallbackAdapter(listener);

        adapter.on_qr_ready("q".into(), 7);
        adapter.on_failed("dropped".into()); // None — must no-op
        adapter.on_completed("dropped".into(), 9); // None — must no-op
        adapter.on_session_ended();

        assert_eq!(recorder.qr_ready.lock().unwrap().len(), 1);
        assert_eq!(*recorder.session_ended_count.lock().unwrap(), 1);
        assert_eq!(
            recorder.failed.lock().unwrap().len(),
            0,
            "on_failed callback was None — must not record"
        );
        assert_eq!(
            recorder.completed.lock().unwrap().len(),
            0,
            "on_completed callback was None — must not record"
        );
    }

    // ── Bad string payloads ───────────────────────────────────────

    // @scenario: device_link:CABI listener silently drops strings with interior null bytes
    #[test]
    fn callback_adapter_drops_strings_with_interior_nul_bytes() {
        let recorder = Recorder::default();
        let adapter = CallbackAdapter(install(&recorder));

        // CString::new fails on interior nul. Each affected callback
        // must early-return (no callback fired) rather than panic.
        adapter.on_qr_ready("bad\0qr".to_string(), 0);
        adapter.on_failed("bad\0reason".to_string());
        adapter.on_completed("bad\0name".to_string(), 1);

        assert_eq!(
            recorder.qr_ready.lock().unwrap().len(),
            0,
            "interior nul drops the on_qr_ready callback"
        );
        assert_eq!(recorder.failed.lock().unwrap().len(), 0);
        assert_eq!(recorder.completed.lock().unwrap().len(), 0);

        // A subsequent valid call still fires — the previous failures
        // must not have left the adapter in a broken state.
        adapter.on_qr_ready("ok".into(), 42);
        assert_eq!(recorder.qr_ready.lock().unwrap().len(), 1);
    }

    // ── Null-pointer export safety ────────────────────────────────

    // @scenario: device_link:CABI exports tolerate null session pointer
    #[test]
    fn null_session_lifecycle_exports_are_safe() {
        // Each export must early-return on null without crashing.
        unsafe {
            vauchi_device_link_session_set_listener(
                std::ptr::null_mut(),
                VauchiDeviceLinkListener {
                    on_qr_ready: None,
                    on_confirmation_required: None,
                    on_request_sent: None,
                    on_completed: None,
                    on_failed: None,
                    on_session_ended: None,
                    user_data: std::ptr::null_mut(),
                },
            );
            vauchi_device_link_session_start(std::ptr::null_mut());
            vauchi_device_link_session_deny(std::ptr::null_mut());
            vauchi_device_link_session_cancel(std::ptr::null_mut());
            vauchi_device_link_session_destroy(std::ptr::null_mut());

            let r1 = vauchi_device_link_session_confirm_manual(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
            );
            assert_eq!(r1, -1, "confirm_manual returns -1 on null session");

            let bytes = [0u8; 16];
            let r2 = vauchi_device_link_session_confirm_ultrasonic(
                std::ptr::null_mut(),
                bytes.as_ptr(),
                bytes.len(),
                0,
            );
            assert_eq!(r2, -1, "confirm_ultrasonic returns -1 on null session");
        }
    }
}
