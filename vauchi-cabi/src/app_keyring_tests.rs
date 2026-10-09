// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A desktop whose keyring holds the storage keys but does not answer, or
//! whose data no key opens, starts on Core's storage-lock screens instead of
//! a null handle (vauchi/private#581). The keyring is platform secure
//! storage, so an in-memory double is the right one; the crypto is real.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use vauchi_core::StorageError;
use vauchi_core::api::VauchiConfig;
use vauchi_core::storage::SecureStorage;

use crate::VauchiApp;
use crate::app::open_app;
use crate::app_presentation::{vauchi_app_dispatch, vauchi_app_initial_commands};

/// A keyring that can stop answering, as the Secret Service does when D-Bus
/// is not up yet.
struct SwitchableKeyring {
    keys: Mutex<HashMap<String, Vec<u8>>>,
    reachable: AtomicBool,
}

impl SwitchableKeyring {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            keys: Mutex::new(HashMap::new()),
            reachable: AtomicBool::new(true),
        })
    }

    fn check(&self) -> Result<(), StorageError> {
        if self.reachable.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(StorageError::Encryption(
                "Keyring error: service not reachable".into(),
            ))
        }
    }
}

impl SecureStorage for SwitchableKeyring {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.keys.lock().unwrap().insert(name.into(), key.to_vec());
        Ok(())
    }
    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.check()?;
        Ok(self.keys.lock().unwrap().get(name).cloned())
    }
    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        self.check()?;
        self.keys.lock().unwrap().remove(name);
        Ok(())
    }
}

fn open(dir: &tempfile::TempDir, keyring: Arc<dyn SecureStorage>) -> *mut VauchiApp {
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"));
    let app = open_app(dir.path(), config, Some(keyring));
    assert!(
        !app.is_null(),
        "a storage-key problem must not yield a null handle"
    );
    app
}

fn take_string(ptr: *mut std::ffi::c_char) -> String {
    assert!(!ptr.is_null());
    // SAFETY: the C ABI returned an owned, NUL-terminated string.
    let text = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    // SAFETY: freed exactly once, here.
    unsafe { crate::vauchi_string_free(ptr) };
    text
}

fn initial(app: *mut VauchiApp) -> serde_json::Value {
    // SAFETY: `app` is a live handle owned by the test.
    let batch = take_string(unsafe { vauchi_app_initial_commands(app) });
    serde_json::from_str(&batch).unwrap()
}

fn press_primary(app: *mut VauchiApp, batch: &serde_json::Value) -> serde_json::Value {
    let bar = batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("SetContextBar"))
        .expect("a context bar");
    let event = serde_json::json!({
        "ActionActivated": {
            "surface_id": bar["surface_id"],
            "interaction_id": bar["bar"]["primary"]["interaction_id"],
        }
    });
    let event = CString::new(event.to_string()).unwrap();
    // SAFETY: `app` is live and `event` is NUL-terminated.
    let reply = take_string(unsafe { vauchi_app_dispatch(app, event.as_ptr()) });
    serde_json::from_str(&reply).unwrap()
}

fn surface(batch: &serde_json::Value) -> String {
    batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("SetContextBar"))
        .and_then(|bar| bar["surface_id"].as_str())
        .unwrap_or_default()
        .to_owned()
}

fn destroy(app: *mut VauchiApp) {
    // SAFETY: `app` came from `open_app` and is destroyed exactly once.
    unsafe { crate::vauchi_app_destroy(app) };
}

// @internal
#[test]
fn keyring_held_data_with_the_keyring_not_answering_offers_try_again() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = SwitchableKeyring::new();
    destroy(open(&dir, keyring.clone()));
    keyring.reachable.store(false, Ordering::SeqCst);

    let app = open(&dir, keyring.clone());

    assert_eq!(surface(&initial(app)), "storage_lock.unavailable");
    destroy(app);
}

// @internal
#[test]
fn try_again_opens_once_the_keyring_answers() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = SwitchableKeyring::new();
    destroy(open(&dir, keyring.clone()));
    keyring.reachable.store(false, Ordering::SeqCst);
    let app = open(&dir, keyring.clone());
    let locked = initial(app);

    keyring.reachable.store(true, Ordering::SeqCst);
    let opened = press_primary(app, &locked);

    assert!(
        !surface(&opened).starts_with("storage_lock"),
        "expected the opened app, got {opened}"
    );
    destroy(app);
}

// @internal
#[test]
fn data_no_key_opens_shows_the_unreadable_screen() {
    let dir = tempfile::tempdir().unwrap();
    destroy(open(&dir, SwitchableKeyring::new()));

    let app = open(&dir, SwitchableKeyring::new());

    assert_eq!(surface(&initial(app)), "storage_lock.unreadable");
    destroy(app);
}

// @internal
#[test]
fn data_under_the_file_key_still_opens_with_a_keyring() {
    let dir = tempfile::tempdir().unwrap();
    let config = || VauchiConfig::with_storage_path(dir.path().join("vauchi.db"));
    drop(crate::app::open_with_file_fallback(dir.path(), config()).expect("file-key install"));

    let app = open(&dir, SwitchableKeyring::new());

    assert!(!surface(&initial(app)).starts_with("storage_lock"));
    destroy(app);
}

/// Opens the action menu and activates its only item, Start over.
fn choose_start_over(app: *mut VauchiApp, batch: &serde_json::Value) -> serde_json::Value {
    let bar = batch["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("SetContextBar"))
        .expect("a context bar")
        .clone();
    let dispatch = |interaction: &serde_json::Value| {
        let event = serde_json::json!({
            "ActionActivated": { "surface_id": bar["surface_id"], "interaction_id": interaction }
        });
        let event = CString::new(event.to_string()).unwrap();
        // SAFETY: `app` is live and `event` is NUL-terminated.
        let reply = take_string(unsafe { vauchi_app_dispatch(app, event.as_ptr()) });
        serde_json::from_str::<serde_json::Value>(&reply).unwrap()
    };
    let menu = dispatch(&bar["bar"]["secondary"]["interaction_id"]);
    let item = menu["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| c.get("PresentOverlay"))
        .expect("the action menu")["overlay"]["items"][0]["interaction_id"]
        .clone();
    dispatch(&item)
}

// @internal
#[test]
fn starting_over_on_the_desktop_opens_a_fresh_install() {
    let dir = tempfile::tempdir().unwrap();
    let first = open(&dir, SwitchableKeyring::new());
    // SAFETY: `first` is live; a null display name means the default.
    assert_eq!(
        unsafe { crate::vauchi_app_create_identity(first, std::ptr::null()) },
        0
    );
    destroy(first);
    let app = open(&dir, SwitchableKeyring::new());
    let confirm = choose_start_over(app, &initial(app));
    assert_eq!(surface(&confirm), "storage_lock.confirm_start_over");

    let fresh = press_primary(app, &confirm);

    assert!(!surface(&fresh).starts_with("storage_lock"), "got {fresh}");
    // SAFETY: `app` is live.
    assert_eq!(unsafe { crate::vauchi_app_has_identity(app) }, 0);
    destroy(app);
}
