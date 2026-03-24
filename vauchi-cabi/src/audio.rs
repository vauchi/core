// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! C ABI wrappers for CpalAudioBackend (ultrasonic proximity verification).
//!
//! These functions are available when the `audio` feature is enabled.
//! A single backend instance is shared across calls so that `stop()` can
//! cancel an in-flight `emit()` or `listen()`. Callers should invoke
//! emit/listen from a background thread — they block until audio I/O
//! completes or the timeout expires.

use std::os::raw::c_char;

#[cfg(feature = "audio")]
use super::to_c_string;
#[cfg(feature = "audio")]
use std::sync::Mutex;
#[cfg(feature = "audio")]
use std::time::Duration;

#[cfg(feature = "audio")]
static AUDIO_BACKEND: Mutex<Option<vauchi_core::exchange::CpalAudioBackend>> = Mutex::new(None);

/// Lazily initialize the shared audio backend.
#[cfg(feature = "audio")]
fn with_backend<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&vauchi_core::exchange::CpalAudioBackend) -> R,
{
    let mut guard = AUDIO_BACKEND.lock().ok()?;
    if guard.is_none() {
        *guard = vauchi_core::exchange::CpalAudioBackend::new().ok();
    }
    guard.as_ref().map(f)
}

/// Check if audio proximity verification is available on this platform.
///
/// Returns 1 if cpal can enumerate at least one output and one input device,
/// 0 otherwise. Always safe to call (never panics).
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_audio_is_available() -> i32 {
    #[cfg(feature = "audio")]
    {
        match std::panic::catch_unwind(|| {
            use vauchi_core::exchange::{AudioBackend, AudioCapability};
            with_backend(|b| matches!(b.check_capability(), AudioCapability::Available))
                .unwrap_or(false) as i32
        }) {
            Ok(result) => result,
            Err(_) => 0,
        }
    }
    #[cfg(not(feature = "audio"))]
    {
        0
    }
}

/// Emit an ultrasonic challenge signal containing `data`.
///
/// Blocks until the signal has been emitted. Returns 1 on success, 0 on failure.
/// `data` must point to at least `data_len` valid bytes.
///
/// # Safety
/// `data` must be a valid pointer to `data_len` bytes, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_audio_emit(data: *const u8, data_len: usize) -> i32 {
    #[cfg(feature = "audio")]
    {
        // SAFETY: data is checked non-null, len comes from the C caller. The buffer must be valid for data_len bytes.
        unsafe {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                if data.is_null() || data_len == 0 {
                    return 0;
                }
                let slice = std::slice::from_raw_parts(data, data_len);
                use vauchi_core::exchange::{AudioBackend, AudioConfig};
                let config = AudioConfig::default();
                with_backend(|b| b.emit_signal(slice, &config).is_ok()).unwrap_or(false) as i32
            })) {
                Ok(result) => result,
                Err(_) => 0,
            }
        }
    }
    #[cfg(not(feature = "audio"))]
    {
        let _ = (data, data_len);
        0
    }
}

/// Listen for an ultrasonic response within `timeout_ms` milliseconds.
///
/// Blocks until a response is received or the timeout expires.
/// Returns a JSON string `{"data":[1,2,3,...]}` on success, or null on
/// failure/timeout. The caller must free the returned string with
/// `vauchi_string_free`.
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_audio_listen(timeout_ms: u64) -> *mut c_char {
    #[cfg(feature = "audio")]
    {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            use vauchi_core::exchange::{AudioBackend, AudioConfig};
            let config = AudioConfig::default();
            let timeout = Duration::from_millis(timeout_ms);
            with_backend(|b| {
                b.receive_signal(timeout, &config).ok().map(|data| {
                    let json = serde_json::json!({ "data": data });
                    to_c_string(&json.to_string())
                })
            })
            .flatten()
            .unwrap_or(std::ptr::null_mut())
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
    #[cfg(not(feature = "audio"))]
    {
        let _ = timeout_ms;
        std::ptr::null_mut()
    }
}

/// Stop all audio operations. Cancels any in-flight emit or listen.
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_audio_stop() {
    #[cfg(feature = "audio")]
    {
        let _ = std::panic::catch_unwind(|| {
            use vauchi_core::exchange::AudioBackend;
            with_backend(|b| b.stop());
        });
    }
}
