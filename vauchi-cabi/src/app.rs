// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine C ABI functions.

use std::os::raw::c_char;
#[cfg(feature = "secure-storage")]
use std::sync::Arc;
use std::sync::Mutex;

use vauchi_core::api::Vauchi;
use vauchi_core::ui::*;

use super::{from_c_str, to_c_string, VauchiApp};

/// Create a new AppEngine with in-memory storage and default relay.
///
/// Returns null on initialization failure.
///
/// # Safety
/// No special requirements.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_create() -> *mut VauchiApp {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vauchi_app_create_with_relay(std::ptr::null())
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Create a new AppEngine with a custom relay URL.
///
/// If `relay_url` is null, uses the default (`wss://relay.vauchi.app`).
/// The caller retains ownership of the `relay_url` string.
///
/// Returns null on initialization failure.
///
/// # Safety
/// `relay_url` must be a valid null-terminated C string, or null.
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_create_with_key(
    data_dir: *const c_char,
    relay_url: *const c_char,
    key_bytes: *const u8,
    key_len: usize,
) -> *mut VauchiApp {
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
        let key = SymmetricKey::from_bytes_unchecked(arr);
        arr.zeroize();

        let storage_path = data_path.join("vauchi.db");
        let mut config =
            vauchi_core::api::VauchiConfig::with_storage_path(&storage_path).with_storage_key(key);
        if let Some(url) = from_c_str(relay_url) {
            config = config.with_relay_url(url);
        }

        let vauchi = match Vauchi::new(config) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(VauchiApp {
            engine: Mutex::new(AppEngine::new(vauchi)),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy an AppEngine instance.
///
/// # Safety
/// `handle` must be a pointer returned by `vauchi_app_create`, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_destroy(handle: *mut VauchiApp) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }));
}

/// Get the current screen as a JSON string.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_current_screen(handle: *mut VauchiApp) -> *mut c_char {
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

/// Handle a user action (JSON) and return the result as JSON.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `action_json` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_handle_action(
    handle: *mut VauchiApp,
    action_json: *const c_char,
) -> *mut c_char {
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

/// Navigate to a screen by name. Returns the new screen as JSON.
///
/// Supported screen names: "home", "contacts", "exchange", "settings",
/// "help", "backup", "lock", "onboarding", "emergency_shred",
/// "device_linking", "duress_pin", "delivery_status".
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `screen_name` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_navigate_to(
    handle: *mut VauchiApp,
    screen_name: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let name = match from_c_str(screen_name) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null screen name"}"#),
        };
        let screen = match name.as_str() {
            "onboarding" => AppScreen::Onboarding,
            "home" | "my_info" => AppScreen::MyInfo,
            "contacts" => AppScreen::Contacts,
            "exchange" => AppScreen::Exchange,
            "settings" => AppScreen::Settings,
            "help" => AppScreen::Help,
            "backup" => AppScreen::Backup,
            "lock" => AppScreen::Lock,
            "device_linking" => AppScreen::DeviceLinking,
            "duress_pin" => AppScreen::DuressPin,
            "emergency_shred" => AppScreen::EmergencyShred,
            "delivery_status" => AppScreen::DeliveryStatus,
            _ => return to_c_string(&format!(r#"{{"error":"unknown screen: {}"}}"#, name)),
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

/// Get available screens as a JSON array of strings.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_available_screens(handle: *mut VauchiApp) -> *mut c_char {
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
                    .map(|s| match s {
                        AppScreen::Onboarding => "onboarding",
                        AppScreen::MyInfo => "my_info",
                        AppScreen::Contacts => "contacts",
                        AppScreen::Exchange => "exchange",
                        AppScreen::Settings => "settings",
                        AppScreen::Help => "help",
                        AppScreen::Backup => "backup",
                        AppScreen::Lock => "lock",
                        AppScreen::DeviceLinking => "device_linking",
                        AppScreen::DuressPin => "duress_pin",
                        AppScreen::EmergencyShred => "emergency_shred",
                        AppScreen::DeliveryStatus => "delivery_status",
                        _ => "unknown",
                    })
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

/// Returns the default landing screen as a C string ("my_info" or "contacts").
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_default_screen(handle: *mut VauchiApp) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let app = &*handle;
        match app.engine.lock() {
            Ok(engine) => {
                let screen_id = match engine.default_screen() {
                    AppScreen::Onboarding => "onboarding",
                    AppScreen::Contacts => "contacts",
                    _ => "my_info",
                };
                to_c_string(screen_id)
            }
            Err(_) => to_c_string("my_info"),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Handle a hardware event during an exchange (ADR-031).
///
/// `event_json` must be a JSON-encoded `ExchangeHardwareEvent`.
/// Returns the action result as JSON, or null if the event was ignored
/// (e.g., not on the exchange screen).
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `event_json` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_handle_hardware_event(
    handle: *mut VauchiApp,
    event_json: *const c_char,
) -> *mut c_char {
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
            Ok(mut engine) => {
                match serde_json::from_str::<vauchi_core::exchange::ExchangeHardwareEvent>(&json) {
                    Ok(event) => match engine.handle_hardware_event(event) {
                        Some(result) => serde_json::to_string(&result).map_or_else(
                            |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                            |j| to_c_string(&j),
                        ),
                        None => std::ptr::null_mut(),
                    },
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
#[no_mangle]
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
            if keyring.load_key("_probe").is_ok() {
                if let Ok(vauchi) = Vauchi::with_secure_storage(config.clone(), keyring) {
                    return Box::into_raw(Box::new(VauchiApp {
                        engine: Mutex::new(AppEngine::new(vauchi)),
                    }));
                }
            }
        }

        // Fallback: no keyring
        let vauchi = match Vauchi::new(config) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(VauchiApp {
            engine: Mutex::new(AppEngine::new(vauchi)),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}
