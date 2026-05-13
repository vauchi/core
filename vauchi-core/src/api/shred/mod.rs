// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shred Module — Types, Traits, and Re-exports
//!
//! Provides the public types for the shred protocol and re-exports the
//! `ShredManager` orchestrator and `widget_panic_shred` entry point.

mod manager;
mod storage;

use crate::api::pre_signed::PreSignedPurgeRequest;

pub use manager::ShredManager;
pub(crate) use storage::secure_overwrite_file_public;
pub use storage::widget_panic_shred;

/// Key name for the Shredding Master Key in SecureStorage.
const SMK_KEY_NAME: &str = "smk";

/// Trait for sending relay purge requests during shred operations.
///
/// Abstracted to allow testing without a real relay connection and to
/// decouple the shred orchestration from the network layer.
pub trait PurgeSender {
    /// Sends a pre-signed purge request to the relay.
    ///
    /// Returns `Ok(true)` if the relay acknowledged the purge,
    /// `Ok(false)` if the request was sent but not confirmed,
    /// or `Err` if sending failed entirely.
    fn send_purge(&mut self, purge: &PreSignedPurgeRequest, now: u64) -> Result<bool, ShredError>;
}

/// Trait for sending identity revocation messages to contacts during shred.
///
/// Abstracted to allow testing without a real relay connection and to
/// decouple the shred orchestration from the network layer.
pub trait RevocationSender {
    /// Sends an identity revocation message to a contact via the relay.
    ///
    /// Returns `Ok(true)` if the relay acknowledged the message,
    /// `Ok(false)` if the message was sent but not confirmed,
    /// or `Err` if sending failed entirely.
    fn send_revocation(
        &mut self,
        revocation: &crate::network::IdentityRevoked,
        now: u64,
    ) -> Result<bool, ShredError>;
}

/// Token returned by soft_shred to authorize hard_shred.
#[derive(Debug, Clone)]
pub struct ShredToken {
    created_at: u64,
}

impl ShredToken {
    pub(super) fn new(now: u64) -> Self {
        Self { created_at: now }
    }

    /// Returns when this token was created (unix seconds).
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Reconstructs a token from a stored created_at timestamp.
    pub fn from_created_at(created_at: u64) -> Self {
        Self { created_at }
    }
}

/// Report of shred operations performed.
#[derive(Debug, Default)]
pub struct ShredReport {
    /// Number of contacts notified of deletion.
    pub contacts_notified: usize,
    /// Whether the relay purge was sent successfully.
    pub relay_purge_sent: bool,
    /// Number of linked devices notified.
    pub devices_notified: usize,
    /// Whether SMK was destroyed from SecureStorage.
    pub smk_destroyed: bool,
    /// Whether the identity backup file was securely deleted.
    pub identity_file_destroyed: bool,
    /// Number of key files deleted from FileKeyStorage.
    pub key_files_destroyed: usize,
    /// Whether the SQLite database was securely deleted.
    pub sqlite_destroyed: bool,
    /// Whether the pre-signed messages file was deleted.
    pub pre_signed_deleted: bool,
    /// Whether the data directory was removed.
    pub data_dir_deleted: bool,
}

/// Post-shred verification result.
#[derive(Debug)]
pub struct ShredVerification {
    /// Whether SMK is absent from SecureStorage (expected: true after shred).
    pub smk_absent: bool,
    /// Whether the database file is absent (expected: true after shred).
    pub database_absent: bool,
    /// Whether the data directory is absent (expected: true after shred).
    pub data_dir_absent: bool,
    /// Whether the pre-signed messages file is absent (expected: true after shred).
    pub pre_signed_absent: bool,
    /// Overall: all checks passed.
    pub all_clear: bool,
}

/// Errors from shred operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShredError {
    #[error("Deletion error: {0}")]
    Deletion(#[from] crate::api::deletion::DeletionError),

    #[error("Storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),

    #[error("Pre-signed messages unavailable: {0}")]
    PreSignedUnavailable(String),

    #[error("SMK destruction failed: {0}")]
    SmkDestructionFailed(String),

    #[error("File operation failed: {0}")]
    FileError(String),
}

/// Widget confirmation mode for panic shred activation.
///
/// Defines how the user confirms a panic shred from the home screen widget,
/// providing a safety mechanism against accidental triggers.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum WidgetConfirmationMode {
    /// Default: tap once, then confirm in a dialog.
    TapConfirm,
    /// Long press to trigger (no separate confirmation).
    LongPress,
    /// Double tap to trigger (no separate confirmation).
    DoubleTap,
}

// INLINE_TEST_REQUIRED: tests access private internals
#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::deletion::DeletionManager;
    use crate::api::pre_signed::PreSignedShredMessages;
    use crate::crypto::SymmetricKey;
    use crate::storage::Storage;
    use crate::storage::secure::{MemoryKeyStorage, SecureStorage};

    fn setup_test_env() -> (
        tempfile::TempDir,
        Storage,
        MemoryKeyStorage,
        crate::identity::Identity,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("vauchi.db");
        let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
        let secure_storage = MemoryKeyStorage::new();
        let identity = crate::identity::Identity::create("TestUser", 0);

        // Store SMK in secure storage (as would happen at identity creation)
        let smk = crate::crypto::ShreddingMasterKey::derive_from_seed(&[0x42; 32]);
        secure_storage
            .save_key(SMK_KEY_NAME, smk.as_bytes())
            .unwrap();

        (dir, storage, secure_storage, identity)
    }

    #[test]
    fn test_soft_shred_schedules_deletion() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let token = manager.soft_shred().unwrap();
        assert!(token.created_at() > 0);

        // DeletionManager should show Scheduled state
        let dm = DeletionManager::new(&storage);
        let state = dm.deletion_state().unwrap();
        assert!(
            matches!(state, crate::storage::DeletionState::Scheduled { .. }),
            "Expected Scheduled state, got {:?}",
            state
        );
    }

    #[test]
    fn test_soft_shred_refreshes_pre_signed() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let _token = manager.soft_shred().unwrap();

        // Pre-signed file should exist
        let path = PreSignedShredMessages::file_path(dir.path());
        assert!(path.exists(), "Pre-signed messages file should be created");
    }

    #[test]
    fn test_cancel_shred_restores_state() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let token = manager.soft_shred().unwrap();
        manager.cancel_shred(token).unwrap();

        let dm = DeletionManager::new(&storage);
        let state = dm.deletion_state().unwrap();
        assert!(
            matches!(state, crate::storage::DeletionState::None),
            "Expected None state after cancel"
        );
    }

    #[test]
    fn test_hard_shred_requires_grace_period() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let token = manager.soft_shred().unwrap();

        // Hard shred should fail — grace period hasn't elapsed
        let result = manager.hard_shred(token, None, None);
        result.expect_err("expected error");
    }

    #[test]
    fn test_hard_shred_after_grace_period() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Schedule deletion with timestamps in the past
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);

        let report = manager.hard_shred(token, None, None).unwrap();

        // SMK should be destroyed
        assert!(report.smk_destroyed);
        assert!(secure_storage.load_key(SMK_KEY_NAME).unwrap().is_none());
    }

    #[test]
    fn test_hard_shred_destroys_smk() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Verify SMK exists before shred
        secure_storage
            .load_key(SMK_KEY_NAME)
            .unwrap()
            .expect("expected Some");

        // Set up past-due deletion
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);
        let report = manager.hard_shred(token, None, None).unwrap();

        assert!(report.smk_destroyed);

        // SMK must be gone from SecureStorage
        let smk = secure_storage.load_key(SMK_KEY_NAME).unwrap();
        assert!(smk.is_none(), "SMK should be absent after hard shred");
    }

    #[test]
    fn test_panic_shred_destroys_smk() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Verify SMK exists before
        secure_storage
            .load_key(SMK_KEY_NAME)
            .unwrap()
            .expect("expected Some");

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred(None, None).unwrap();

        assert!(report.smk_destroyed);
        assert!(secure_storage.load_key(SMK_KEY_NAME).unwrap().is_none());
    }

    #[test]
    fn test_panic_shred_loads_pre_signed_before_key_destruction() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create pre-signed messages file
        let msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);
        msgs.save(dir.path()).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred(None, None).unwrap();

        // Should succeed — pre-signed messages were loaded before key destruction
        assert!(report.smk_destroyed);
        assert!(report.pre_signed_deleted);
    }

    #[test]
    fn test_panic_shred_fallback_without_pre_signed() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Do NOT create pre-signed file — panic shred should fall back to fresh signing
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let report = manager.panic_shred(None, None).unwrap();

        // Should succeed even without pre-signed file
        assert!(report.smk_destroyed);
    }

    #[test]
    fn test_verify_shred_after_panic() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        manager.panic_shred(None, None).unwrap();

        let verification = manager.verify_shred();
        assert!(verification.smk_absent, "SMK should be absent");
        assert!(
            verification.pre_signed_absent,
            "Pre-signed file should be absent"
        );
    }

    #[test]
    fn test_verify_shred_before_shred() {
        let (dir, storage, secure_storage, identity) = setup_test_env();
        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let verification = manager.verify_shred();
        // SMK is present, so not all clear
        assert!(!verification.smk_absent);
        assert!(!verification.all_clear);
    }

    #[test]
    fn test_hard_shred_deletes_database() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create some files that would exist
        let identity_path = dir.path().join("identity.json");
        std::fs::write(&identity_path, b"test identity data").unwrap();

        // Set up past-due deletion
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);
        let report = manager.hard_shred(token, None, None).unwrap();

        assert!(report.identity_file_destroyed);
        assert!(!identity_path.exists(), "Identity file should be deleted");
    }

    #[test]
    fn test_hard_shred_deletes_key_files() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create some key files
        let keys_dir = dir.path().join("keys");
        std::fs::create_dir_all(&keys_dir).unwrap();
        std::fs::write(keys_dir.join("key1"), b"secret key 1").unwrap();
        std::fs::write(keys_dir.join("key2"), b"secret key 2").unwrap();

        // Set up past-due deletion
        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);
        let report = manager.hard_shred(token, None, None).unwrap();

        assert_eq!(report.key_files_destroyed, 2);
    }

    // === PurgeSender tests ===

    struct MockPurgeSender {
        purge_sent: bool,
        should_fail: bool,
    }

    impl MockPurgeSender {
        fn new() -> Self {
            Self {
                purge_sent: false,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                purge_sent: false,
                should_fail: true,
            }
        }
    }

    impl PurgeSender for MockPurgeSender {
        fn send_purge(
            &mut self,
            _purge: &PreSignedPurgeRequest,
            _now: u64,
        ) -> Result<bool, ShredError> {
            if self.should_fail {
                return Err(ShredError::FileError("mock purge failure".into()));
            }
            self.purge_sent = true;
            Ok(true)
        }
    }

    #[test]
    fn test_hard_shred_sends_purge_when_sender_provided() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);

        let mut sender = MockPurgeSender::new();
        let report = manager.hard_shred(token, Some(&mut sender), None).unwrap();

        assert!(sender.purge_sent, "Purge should have been sent");
        assert!(report.relay_purge_sent, "Report should reflect purge sent");
    }

    #[test]
    fn test_hard_shred_succeeds_when_purge_fails() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);

        let mut sender = MockPurgeSender::failing();
        let report = manager.hard_shred(token, Some(&mut sender), None).unwrap();

        assert!(
            !report.relay_purge_sent,
            "Purge failure should not abort shred"
        );
        assert!(report.smk_destroyed, "SMK should still be destroyed");
    }

    #[test]
    fn test_panic_shred_sends_purge() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Create pre-signed messages file
        let msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);
        msgs.save(dir.path()).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let mut sender = MockPurgeSender::new();
        let report = manager.panic_shred(Some(&mut sender), None).unwrap();

        assert!(sender.purge_sent, "Purge should have been sent");
        assert!(report.relay_purge_sent, "Report should reflect purge sent");
        assert!(report.smk_destroyed, "SMK should be destroyed");
    }

    #[test]
    fn test_shred_without_sender_still_works() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);

        // Pass None — backward compat
        let report = manager.hard_shred(token, None, None).unwrap();

        assert!(!report.relay_purge_sent);
        assert!(report.smk_destroyed);
    }

    // === RevocationSender tests ===

    struct MockRevocationSender {
        revocations_sent: std::cell::RefCell<Vec<String>>,
        should_fail: bool,
    }

    impl MockRevocationSender {
        fn new() -> Self {
            Self {
                revocations_sent: std::cell::RefCell::new(Vec::new()),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                revocations_sent: std::cell::RefCell::new(Vec::new()),
                should_fail: true,
            }
        }

        fn sent_count(&self) -> usize {
            self.revocations_sent.borrow().len()
        }
    }

    impl RevocationSender for MockRevocationSender {
        fn send_revocation(
            &mut self,
            revocation: &crate::network::IdentityRevoked,
            _now: u64,
        ) -> Result<bool, ShredError> {
            if self.should_fail {
                return Err(ShredError::FileError("mock revocation failure".into()));
            }
            self.revocations_sent
                .borrow_mut()
                .push(revocation.recipient_id.clone());
            Ok(true)
        }
    }

    fn add_test_contact(storage: &crate::storage::Storage, _contact_id: &str) {
        use crate::contact::Contact;
        use crate::contact_card::ContactCard;
        use crate::crypto::SymmetricKey;

        let card = ContactCard::new("Test Contact");
        // Use a deterministic public key derived from the contact_id
        let mut public_key = [0u8; 32];
        let id_bytes = _contact_id.as_bytes();
        let len = id_bytes.len().min(32);
        public_key[..len].copy_from_slice(&id_bytes[..len]);

        let shared_key = SymmetricKey::generate();
        let contact = Contact::from_exchange(public_key, card, shared_key);
        storage.save_contact(&contact).unwrap();
    }

    #[test]
    fn test_hard_shred_sends_revocations() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        // Add test contacts
        add_test_contact(&storage, "contact_aaa");
        add_test_contact(&storage, "contact_bbb");

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);

        let mut sender = MockRevocationSender::new();
        let report = manager.hard_shred(token, None, Some(&mut sender)).unwrap();

        assert_eq!(report.contacts_notified, 2);
        assert_eq!(sender.sent_count(), 2);
    }

    #[test]
    fn test_hard_shred_revocation_failure_does_not_abort() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);

        let mut sender = MockRevocationSender::failing();
        let report = manager.hard_shred(token, None, Some(&mut sender)).unwrap();

        // Revocation failed but shred still succeeded
        assert_eq!(report.contacts_notified, 0);
        assert!(report.smk_destroyed, "SMK should still be destroyed");
    }

    #[test]
    fn test_panic_shred_sends_revocations() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");
        add_test_contact(&storage, "contact_bbb");
        add_test_contact(&storage, "contact_ccc");

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let mut sender = MockRevocationSender::new();
        let report = manager.panic_shred(None, Some(&mut sender)).unwrap();

        assert_eq!(report.contacts_notified, 3);
        assert_eq!(sender.sent_count(), 3);
        assert!(report.smk_destroyed);
    }

    #[test]
    fn test_panic_shred_revocation_failure_does_not_abort() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());

        let mut sender = MockRevocationSender::failing();
        let report = manager.panic_shred(None, Some(&mut sender)).unwrap();

        assert_eq!(report.contacts_notified, 0);
        assert!(report.smk_destroyed, "SMK should still be destroyed");
    }

    #[test]
    fn test_hard_shred_no_revocation_sender_backward_compat() {
        let (dir, storage, secure_storage, identity) = setup_test_env();

        add_test_contact(&storage, "contact_aaa");

        let dm = DeletionManager::new(&storage);
        dm.schedule_deletion_with_execute_at(1000, 1001).unwrap();

        let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
        let token = ShredToken::new(1_700_000_000);

        // Pass None for revocation_sender — backward compat
        let report = manager.hard_shred(token, None, None).unwrap();

        assert_eq!(report.contacts_notified, 0);
        assert!(report.smk_destroyed);
    }
}
