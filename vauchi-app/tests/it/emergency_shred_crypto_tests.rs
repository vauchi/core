// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The emergency wipe is the crypto-shred (vauchi/private#599): it deletes
//! every key that opens the data, and the data, so a copy taken before the
//! wipe stays unreadable. Found on the rig: the wipe deleted rows, kept the
//! SMK and the database, and a restored pre-wipe copy opened again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::StorageError;
use vauchi_core::VauchiError;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::storage::SecureStorage;

/// The platform keychain: secure storage outside the data directory.
#[derive(Default)]
struct Keychain(Mutex<HashMap<String, Vec<u8>>>);

impl SecureStorage for Keychain {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), StorageError> {
        self.0.lock().unwrap().insert(name.into(), key.to_vec());
        Ok(())
    }
    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }
    fn delete_key(&self, name: &str) -> Result<(), StorageError> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

struct Install {
    _root: tempfile::TempDir,
    data_dir: PathBuf,
    keychain: Arc<Keychain>,
}

impl Install {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        Self {
            _root: root,
            data_dir,
            keychain: Arc::new(Keychain::default()),
        }
    }

    fn open(&self, db_dir: &Path) -> Result<Vauchi, VauchiError> {
        let config = VauchiConfig::with_storage_path(db_dir.join("vauchi.db"));
        Vauchi::with_secure_storage(config, self.keychain.clone())
    }

    fn has_key(&self, name: &str) -> bool {
        self.keychain.load_key(name).unwrap().is_some()
    }
}

fn engine_on_shred(install: &Install) -> AppEngine {
    let mut vauchi = install.open(&install.data_dir).unwrap();
    vauchi.create_identity("Alice").unwrap();
    assert!(
        install.has_key("smk"),
        "precondition: the SMK is in the keychain"
    );
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "danger".into(),
        item_id: "emergency_wipe".into(),
    });
    engine
}

fn confirm_wipe(engine: &mut AppEngine) -> ActionResult {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "confirmation".into(),
        value: "WIPE".into(),
    });
    engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_shred".into(),
    })
}

/// A copy of the data directory as someone could have taken it before the
/// wipe.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

// @internal
#[test]
fn the_emergency_wipe_deletes_every_key_and_the_data() {
    let install = Install::new();
    let mut engine = engine_on_shred(&install);

    let result = confirm_wipe(&mut engine);

    assert!(
        matches!(result, ActionResult::WipeComplete),
        "got {result:?}"
    );
    assert!(!install.has_key("smk"), "the SMK survived the wipe");
    assert!(
        !install.has_key("storage_bootstrap"),
        "the bootstrap key survived"
    );
    assert!(!install.data_dir.exists(), "the data directory survived");
}

// @internal
#[test]
fn a_copy_taken_before_the_wipe_does_not_open_afterwards() {
    let install = Install::new();
    let mut engine = engine_on_shred(&install);
    let copy = install._root.path().join("copy");
    copy_dir(&install.data_dir, &copy);

    let _ = confirm_wipe(&mut engine);
    drop(engine);

    let reopened = install.open(&copy);
    assert!(
        matches!(reopened, Err(VauchiError::Storage(StorageError::WrongKey))),
        "a pre-wipe copy opened after the wipe: {:?}",
        reopened.as_ref().map(|_| "opened")
    );
}
