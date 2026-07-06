// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine C ABI functions.

use std::os::raw::c_char;
#[cfg(feature = "secure-storage")]
use std::sync::Arc;
use std::sync::Mutex;

use vauchi_app::ui::*;
use vauchi_core::api::Vauchi;
#[cfg(feature = "secure-storage")]
use vauchi_core::storage::SecureStorage;

use super::app_import_warnings::warnings_to_json;
use super::{VauchiApp, from_c_str, to_c_string};

/// Create a new AppEngine with in-memory storage and default relay.
///
/// Returns null on initialization failure.
///
/// # Safety
/// No special requirements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_create() -> *mut VauchiApp {
    // SAFETY: Delegates to vauchi_app_create_with_relay; catch_unwind prevents panics from unwinding into C.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            vauchi_app_create_with_relay(std::ptr::null())
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Create a new AppEngine with a custom relay URL.
///
/// If `relay_url` is null, uses the default (`https://relay.vauchi.app`).
/// The caller retains ownership of the `relay_url` string.
///
/// Returns null on initialization failure.
///
/// # Safety
/// `relay_url` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_create_with_relay(relay_url: *const c_char) -> *mut VauchiApp {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let vauchi = match Vauchi::in_memory() {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };
        let mut engine = AppEngine::new(vauchi);
        if let Some(url) = from_c_str(relay_url) {
            engine.vauchi_mut().config_mut().relay.server_url = url;
        }
        Box::into_raw(Box::new(VauchiApp {
            engine: Mutex::new(engine),
            event_handler_id: Mutex::new(None),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Create a new AppEngine with persistent storage and custom relay URL.
///
/// Unlike `vauchi_app_create` (in-memory), this stores data on disk at
/// `data_dir/vauchi.db`. Pass null for `relay_url` to use the default.
///
/// Returns null on initialization failure.
///
/// # Safety
/// `data_dir` must be a valid null-terminated C string pointing to a
/// writable directory. `relay_url` must be a valid null-terminated C
/// string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_create_with_config(
    data_dir: *const c_char,
    relay_url: *const c_char,
) -> *mut VauchiApp {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = match from_c_str(data_dir) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let data_path = std::path::PathBuf::from(&dir);
        if std::fs::create_dir_all(&data_path).is_err() {
            return std::ptr::null_mut();
        }

        let storage_path = data_path.join("vauchi.db");
        let mut config = vauchi_core::api::VauchiConfig::with_storage_path(&storage_path);
        if let Some(url) = from_c_str(relay_url) {
            config = config.with_relay_url(url);
        }

        let vauchi = match Vauchi::new(config) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(VauchiApp {
            engine: Mutex::new(AppEngine::new(vauchi)),
            event_handler_id: Mutex::new(None),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Create a new AppEngine with persistent storage and caller-provided key.
///
/// The caller manages key storage (e.g., Windows PasswordVault, platform keychain).
/// `key_bytes` must point to exactly `key_len` bytes. `key_len` must be 32.
///
/// Returns null on initialization failure or invalid parameters.
///
/// # Safety
/// `data_dir` must be a valid null-terminated C string pointing to a writable directory.
/// `relay_url` must be a valid null-terminated C string, or null.
/// `key_bytes` must point to at least `key_len` valid bytes, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_create_with_key(
    data_dir: *const c_char,
    relay_url: *const c_char,
    key_bytes: *const u8,
    key_len: usize,
) -> *mut VauchiApp {
    // SAFETY: key_bytes/key_len validated inside closure; from_raw_parts requires valid ptr checked above. Box::into_raw transfers ownership to C caller.
    unsafe {
        use vauchi_core::crypto::SymmetricKey;
        use zeroize::Zeroize;

        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            if key_bytes.is_null() || key_len != 32 {
                return std::ptr::null_mut();
            }

            let dir = match from_c_str(data_dir) {
                Some(d) => d,
                None => return std::ptr::null_mut(),
            };

            let data_path = std::path::PathBuf::from(&dir);
            if std::fs::create_dir_all(&data_path).is_err() {
                return std::ptr::null_mut();
            }

            let mut arr = [0u8; 32];
            arr.copy_from_slice(std::slice::from_raw_parts(key_bytes, 32));
            let key = match SymmetricKey::try_from_bytes(arr) {
                Ok(k) => k,
                Err(_) => return std::ptr::null_mut(),
            };
            arr.zeroize();

            let storage_path = data_path.join("vauchi.db");
            let mut config = vauchi_core::api::VauchiConfig::with_storage_path(&storage_path)
                .with_storage_key(key);
            if let Some(url) = from_c_str(relay_url) {
                config = config.with_relay_url(url);
            }

            let vauchi = match Vauchi::new(config) {
                Ok(v) => v,
                Err(_) => return std::ptr::null_mut(),
            };

            Box::into_raw(Box::new(VauchiApp {
                engine: Mutex::new(AppEngine::new(vauchi)),
                event_handler_id: Mutex::new(None),
            }))
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Destroy an AppEngine instance.
///
/// # Safety
/// `handle` must be a pointer returned by `vauchi_app_create`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_destroy(handle: *mut VauchiApp) {
    // SAFETY: handle was created by Box::into_raw in a _create function. Caller must not use the handle after this call.
    unsafe {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !handle.is_null() {
                drop(Box::from_raw(handle));
            }
        }));
    }
}

/// Get the current screen as a JSON string.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_current_screen(handle: *mut VauchiApp) -> *mut c_char {
    // SAFETY: handle is checked non-null; ptr was created by Box::into_raw and has not been freed.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            match app.engine.lock() {
                Ok(engine) => {
                    let screen = engine.current_screen();
                    match serde_json::to_string(&screen) {
                        Ok(json) => to_c_string(&json),
                        Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                    }
                }
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Poll for any new OS notifications produced by the app engine.
///
/// Returns a JSON-encoded array of `PendingNotification` objects, or
/// null if there are no new notifications.
///
/// # Safety
/// `app` must be a valid pointer created by `vauchi_app_create*`.
/// The caller must free the returned string via `vauchi_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_poll_notifications(app: *mut VauchiApp) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if app.is_null() {
            return std::ptr::null_mut();
        }

        let app = unsafe { &*app };
        let mut engine = match app.engine.lock() {
            Ok(lock) => lock,
            Err(_) => return std::ptr::null_mut(),
        };

        let notifications = engine.poll_notifications();

        if notifications.is_empty() {
            return std::ptr::null_mut();
        }

        match serde_json::to_string(&notifications) {
            Ok(json) => to_c_string(&json),
            Err(_) => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Handle a user action (JSON) and return the result as JSON.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `action_json` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_handle_action(
    handle: *mut VauchiApp,
    action_json: *const c_char,
) -> *mut c_char {
    // SAFETY: handle is checked non-null; action_json read via from_c_str which checks null and requires NUL-terminated string.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let json = match from_c_str(action_json) {
                Some(s) => s,
                None => return to_c_string(r#"{"error":"null action JSON"}"#),
            };
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => match serde_json::from_str::<UserAction>(&json) {
                    Ok(action) => {
                        let result = engine.handle_action(action);
                        serde_json::to_string(&result).map_or_else(
                            |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                            |j| to_c_string(&j),
                        )
                    }
                    Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                },
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Navigate to a screen by name. Returns the new screen as JSON.
///
/// Supported screen names: "home", "contacts", "exchange", "settings",
/// "help", "backup", "lock", "onboarding", "emergency_shred",
/// "device_linking", "device_management", "duress_pin", "delivery_status",
/// "sync", "recovery", "groups", "privacy", "support",
/// "contact_duplicates", "contact_limit", "more".
///
/// **Deprecated (Tier-0 d, ADR-043 Amendment 4):** a forward-navigate surface.
/// Desktop frontends should forward tab taps via `UserAction::NavigateToTab`
/// (carrying the `TabInfo.action_id` core minted) and render the returned
/// `NavigateTo`. Do not add new callers; retires once frontends migrate.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `screen_name` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_navigate_to(
    handle: *mut VauchiApp,
    screen_name: *const c_char,
) -> *mut c_char {
    // SAFETY: handle is checked non-null; screen_name read via from_c_str which checks null and requires NUL-terminated string.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let name = match from_c_str(screen_name) {
                Some(s) => s,
                None => return to_c_string(r#"{"error":"null screen name"}"#),
            };
            let screen = match AppScreen::from_screen_id(&name) {
                Some(s) => s,
                None => return to_c_string(&format!(r#"{{"error":"unknown screen: {}"}}"#, name)),
            };
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => {
                    let model = engine.navigate_to(screen);
                    serde_json::to_string(&model).map_or_else(
                        |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                        |j| to_c_string(&j),
                    )
                }
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Open a device-link join invitation on a fresh device.
///
/// `invitation_url` is the raw invitation string (e.g.
/// `vauchi://device-link?qr=...&code=...`). On success, navigates to the
/// device-link join screen and returns it as JSON. On failure returns
/// `{"error":"..."}` (invalid URL or this device already has an identity).
/// Returns null if `handle` or `invitation_url` is null.
///
/// The caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `invitation_url` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_open_device_link_invitation(
    handle: *mut VauchiApp,
    invitation_url: *const c_char,
) -> *mut c_char {
    // SAFETY: handle is checked non-null; invitation_url read via from_c_str which checks null and requires NUL-terminated string.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let url = match from_c_str(invitation_url) {
                Some(s) => s,
                None => return to_c_string(r#"{"error":"null invitation URL"}"#),
            };
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => match engine.open_device_link_invitation(&url) {
                    Ok(screen) => serde_json::to_string(&screen).map_or_else(
                        |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                        |j| to_c_string(&j),
                    ),
                    Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                },
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Navigate back one step. Returns the resulting screen as JSON.
///
/// Pops the engine's `AppScreen` nav history, or rewinds one in-engine
/// sub-flow step (the exchange flow). Frontends gate this on the
/// `can_go_back` field of the current screen and render a back affordance
/// in their own chrome — so C-ABI frontends (linux-qt, windows) no longer
/// depend on a footer "Back" action.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_navigate_back(handle: *mut VauchiApp) -> *mut c_char {
    // SAFETY: handle is checked non-null; ptr was created by Box::into_raw and has not been freed.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => {
                    let model = engine.navigate_back();
                    serde_json::to_string(&model).map_or_else(
                        |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                        |j| to_c_string(&j),
                    )
                }
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Get available screens as a JSON array of strings.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_available_screens(handle: *mut VauchiApp) -> *mut c_char {
    // SAFETY: handle is checked non-null; ptr was created by Box::into_raw and has not been freed.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            match app.engine.lock() {
                Ok(engine) => {
                    let screens: Vec<&str> = engine
                        .available_screens()
                        .iter()
                        .map(|s| s.screen_id())
                        .collect();
                    serde_json::to_string(&screens).map_or_else(
                        |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                        |j| to_c_string(&j),
                    )
                }
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Returns the default landing screen as a C string ("my_info" or "contacts").
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_default_screen(handle: *mut VauchiApp) -> *mut c_char {
    // SAFETY: handle is checked non-null; ptr was created by Box::into_raw and has not been freed.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            match app.engine.lock() {
                Ok(engine) => to_c_string(engine.default_screen().screen_id()),
                Err(_) => to_c_string("my_info"),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Get the canonical screen-id of the parent tab the active screen
/// belongs to under the requested layout.
///
/// `layout` selects the tab universe:
/// - `0` = Mobile (5-tab bottom nav, matches `vauchi_app_tab_info`)
/// - `1` = Desktop (14-tab sidebar, matches `vauchi_app_sidebar_items`)
///
/// Returns:
/// - A C string with the parent tab's screen_id (caller must free
///   with `vauchi_string_free`)
/// - Null when the active screen is a transient overlay (Lock,
///   FormDialog) — frontend should leave selection unchanged.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_current_tab_id(
    handle: *mut VauchiApp,
    layout: i32,
) -> *mut c_char {
    // SAFETY: handle is checked non-null; engine lock prevents concurrent access.
    unsafe {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let layout = match layout {
                0 => TabLayout::Mobile,
                1 => TabLayout::Desktop,
                _ => return std::ptr::null_mut(),
            };
            let app = &*handle;
            let Ok(engine) = app.engine.lock() else {
                return std::ptr::null_mut();
            };
            match engine.current_tab_id(layout) {
                Some(id) => to_c_string(id),
                None => std::ptr::null_mut(),
            }
        }))
        .unwrap_or(std::ptr::null_mut())
    }
}

/// Check whether the app has an identity.
///
/// Returns 1 if an identity exists, 0 if not, -1 on error (null handle, lock failure).
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_has_identity(handle: *mut VauchiApp) -> i32 {
    // SAFETY: handle is checked non-null; engine lock prevents concurrent access.
    unsafe {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return -1;
            }
            let app = &*handle;
            app.engine
                .lock()
                .map(|engine| i32::from(engine.vauchi().has_identity()))
                .unwrap_or(-1)
        }))
        .unwrap_or(-1)
    }
}

/// Create a test identity (DEBUG/testing only).
///
/// Creates an identity with the given display name. No-op if an identity
/// already exists. Returns 0 on success, -1 on error.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `display_name` must be a valid null-terminated C string, or null (defaults to "Test User").
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_create_identity(
    handle: *mut VauchiApp,
    display_name: *const c_char,
) -> i32 {
    // SAFETY: handle is checked non-null; display_name is checked null or valid C string.
    unsafe {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return -1;
            }
            let app = &mut *handle;
            let name = if display_name.is_null() {
                "Test User".to_string()
            } else {
                from_c_str(display_name).unwrap_or_else(|| "Test User".to_string())
            };
            app.engine
                .lock()
                .map(|mut engine| {
                    if engine.vauchi().has_identity() {
                        return 0; // Already has identity — true no-op
                    }
                    engine
                        .vauchi_mut()
                        .create_identity(&name)
                        .map(|()| 0)
                        .unwrap_or(-1)
                })
                .unwrap_or(-1)
        }))
        .unwrap_or(-1)
    }
}

/// Handle a hardware event during an exchange (ADR-031).
///
/// `event_json` must be a JSON-encoded `Event`.
/// Returns the action result as JSON, or null if the event was ignored
/// (e.g., not on the exchange screen).
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `event_json` must be a valid null-terminated C string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_handle_hardware_event(
    handle: *mut VauchiApp,
    event_json: *const c_char,
) -> *mut c_char {
    // SAFETY: handle is checked non-null; event_json read via from_c_str which checks null and requires NUL-terminated string.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let json = match from_c_str(event_json) {
                Some(s) => s,
                None => return std::ptr::null_mut(),
            };
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => match serde_json::from_str::<vauchi_core::Event>(&json) {
                    Ok(event) => match engine.handle_hardware_event(event) {
                        Some(result) => serde_json::to_string(&result).map_or_else(
                            |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                            |j| to_c_string(&j),
                        ),
                        None => std::ptr::null_mut(),
                    },
                    Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                },
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Notify the engine that the app moved to the background.
///
/// If a password is set and the app is not already locked or in
/// onboarding, navigates to the lock screen and returns the lock
/// screen JSON. Otherwise returns null.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_handle_app_backgrounded(handle: *mut VauchiApp) -> *mut c_char {
    // SAFETY: handle is checked non-null; ptr was created by Box::into_raw and has not been freed.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => match engine.handle_app_backgrounded() {
                    Some(screen) => serde_json::to_string(&screen).map_or_else(
                        |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                        |j| to_c_string(&j),
                    ),
                    None => std::ptr::null_mut(),
                },
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Type alias for the C event callback function pointer.
///
/// Called by core when background operations invalidate screen data.
/// `screen_ids_json` is a JSON array of screen ID strings, e.g. `["contacts","sync"]`.
/// `user_data` is the opaque pointer passed to `vauchi_app_set_event_callback`.
///
/// The string is owned by core and must NOT be freed by the caller.
pub type VauchiEventCallback =
    Option<unsafe extern "C" fn(screen_ids_json: *const c_char, user_data: *mut std::ffi::c_void)>;

/// Register a callback for async state-change notifications.
///
/// Core calls `callback` when background operations (sync, delivery,
/// device link) change data that affects rendered screens. Pass null
/// to unregister. `user_data` is forwarded to each callback invocation.
///
/// # Threading — IMPORTANT
///
/// The callback may fire **on the same thread** that called
/// `vauchi_app_handle_action` (synchronous event dispatch). The callback
/// **must not** call back into any `vauchi_app_*` function directly —
/// doing so would deadlock on the internal Mutex. Always defer
/// processing to a separate thread or event loop iteration.
///
/// # Safety
/// `handle` must be a valid `VauchiApp` pointer. `callback` (if non-null)
/// must be safe to call from any thread. `user_data` must remain valid
/// until the callback is unregistered.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_set_event_callback(
    handle: *mut VauchiApp,
    callback: VauchiEventCallback,
    user_data: *mut std::ffi::c_void,
) {
    if handle.is_null() {
        return;
    }
    // Wrap in Send+Sync wrapper before entering catch_unwind closure
    let handler = callback.map(|cb| EventCallbackHandler { cb, user_data });
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: handle is checked for null above
        let app = unsafe { &*handle };
        let Ok(engine) = app.engine.lock() else {
            return;
        };
        let Ok(mut handler_id_slot) = app.event_handler_id.lock() else {
            return;
        };

        if let Some(old_id) = handler_id_slot.take() {
            engine.vauchi().remove_event_handler(old_id);
        }

        if let Some(handler) = handler {
            let new_id = engine
                .vauchi()
                .add_event_handler(std::sync::Arc::new(move |event| {
                    handler.dispatch(&event);
                }));
            *handler_id_slot = Some(new_id);
        }
    }));
}

/// Bundles a C callback function pointer and user_data for cross-thread use.
///
/// # Safety
/// The caller of `vauchi_app_set_event_callback` guarantees that both the
/// callback and `user_data` remain valid and thread-safe for the lifetime
/// of the registration.
struct EventCallbackHandler {
    cb: unsafe extern "C" fn(*const c_char, *mut std::ffi::c_void),
    user_data: *mut std::ffi::c_void,
}

// SAFETY: caller of vauchi_app_set_event_callback guarantees thread-safety
unsafe impl Send for EventCallbackHandler {}
// SAFETY: caller of vauchi_app_set_event_callback guarantees thread-safety
unsafe impl Sync for EventCallbackHandler {}

impl EventCallbackHandler {
    fn dispatch(&self, event: &vauchi_core::api::VauchiEvent) {
        let screen_ids = crate::platform_event::affected_screens_json(event);
        if let Ok(json) = std::ffi::CString::new(screen_ids) {
            // SAFETY: caller guarantees callback + user_data are valid
            unsafe {
                (self.cb)(json.as_ptr(), self.user_data);
            }
        }
    }
}

/// Create a new AppEngine with persistent storage and platform keyring.
///
/// Uses `PlatformKeyring` (D-Bus Secret Service on Linux, Keychain on macOS)
/// for secure key storage. Falls back to file-based key storage if the
/// keyring is unavailable.
///
/// Returns null on initialization failure.
///
/// # Safety
/// `data_dir` must be a valid null-terminated C string pointing to a
/// writable directory. `relay_url` must be a valid null-terminated C
/// string, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_create_with_keyring(
    data_dir: *const c_char,
    relay_url: *const c_char,
) -> *mut VauchiApp {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = match from_c_str(data_dir) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let data_path = std::path::PathBuf::from(&dir);
        if std::fs::create_dir_all(&data_path).is_err() {
            return std::ptr::null_mut();
        }

        let storage_path = data_path.join("vauchi.db");
        let mut config = vauchi_core::api::VauchiConfig::with_storage_path(&storage_path);
        if let Some(url) = from_c_str(relay_url) {
            config = config.with_relay_url(url);
        }

        // Try platform keyring first, fall back to config-only init
        #[cfg(feature = "secure-storage")]
        {
            let keyring = Arc::new(vauchi_core::storage::PlatformKeyring::new("vauchi"));
            // Probe the keyring to see if it's functional
            if keyring.load_key("_probe").is_ok()
                && let Ok(vauchi) = Vauchi::with_secure_storage(config.clone(), keyring)
            {
                return Box::into_raw(Box::new(VauchiApp {
                    engine: Mutex::new(AppEngine::new(vauchi)),
                    event_handler_id: Mutex::new(None),
                }));
            }
        }

        // Fallback: no keyring
        let vauchi = match Vauchi::new(config) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(VauchiApp {
            engine: Mutex::new(AppEngine::new(vauchi)),
            event_handler_id: Mutex::new(None),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Import contacts from vCard (.vcf) data.
///
/// Returns a JSON object on success:
/// ```json
/// {
///   "imported": 3,
///   "skipped": 1,
///   "warnings": [
///     {"key": "import.warning.duplicate_uid", "args": {"uid": "abc"}, "legacy_text": "Skipped duplicate (UID: abc)"}
///   ]
/// }
/// ```
///
/// Each warning object carries the stable i18n `key`, a string map of
/// placeholder `args`, and a pre-rendered English `legacy_text` frontends
/// may use as a fallback. The shape matches the UniFFI
/// `MobileImportWarning` record so CABI + UniFFI consumers stay aligned
/// (G6 of the pure-renderer remediation).
///
/// Returns `{"error":"..."}` on failure. Returns null if `handle` or
/// `data` is null.
///
/// The caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `data` must point to at least `data_len` valid bytes, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_import_contacts_from_vcf(
    handle: *mut VauchiApp,
    data: *const u8,
    data_len: usize,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() || data.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: caller guarantees data points to data_len valid bytes
        let bytes = unsafe { std::slice::from_raw_parts(data, data_len) };
        let app = unsafe { &*handle };
        match app.engine.lock() {
            Ok(engine) => match engine.vauchi().import_contacts_from_vcf(bytes) {
                Ok(result) => {
                    let json = serde_json::json!({
                        "imported": result.imported,
                        "skipped": result.skipped,
                        "warnings": warnings_to_json(&result.warnings),
                    });
                    to_c_string(&json.to_string())
                }
                Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
            },
            Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Drain pending OS notifications as a JSON array.
///
/// Returns a JSON array of notification objects, e.g.:
/// `[{"event_key":"...","category":"EmergencyAlert","title":"...","body":"...","contact_id":"..."}]`
///
/// Returns `"[]"` if no notifications are pending.
/// Returns null on error (null handle, lock poisoned).
///
/// Frontends should call this after receiving the event callback.
/// Each call clears the buffer — notifications are never returned twice.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_drain_notifications(handle: *mut VauchiApp) -> *mut c_char {
    // SAFETY: handle is checked non-null; engine lock prevents concurrent access.
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => {
                    let notifications = engine.drain_pending_notifications();
                    match serde_json::to_string(&notifications) {
                        Ok(json) => to_c_string(&json),
                        Err(_) => to_c_string("[]"),
                    }
                }
                Err(_) => std::ptr::null_mut(),
            }
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}
