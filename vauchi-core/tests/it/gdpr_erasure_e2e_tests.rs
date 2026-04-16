// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR Right-to-Erasure End-to-End Tests
//!
//! Verifies the complete Article 17 deletion path — grace period enforcement,
//! full erasure with revocations, cancellation semantics, crypto-shredding
//! key-hierarchy soundness, and ShredManager purge/revocation integration.
//!
//! Feature: privacy_compliance.feature @erasure @gdpr

use crate::common;

use vauchi_core::api::{
    DeletionManager, PreSignedPurgeRequest, PurgeSender, RevocationSender, ShredError,
    ShredManager, ShredToken,
};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{ShreddingMasterKey, SymmetricKey};
use vauchi_core::identity::Identity;
use vauchi_core::network::IdentityRevoked;
use vauchi_core::storage::{DeletionState, MemoryKeyStorage, SecureStorage, Storage};

const SMK_KEY_NAME: &str = "smk";

// ============================================================
// Helpers
// ============================================================

fn in_memory_storage() -> Storage {
    Storage::in_memory(SymmetricKey::generate()).unwrap()
}

fn add_test_contact(storage: &Storage, contact_id: &str) {
    let card = ContactCard::new("Test Contact");
    let mut public_key = [0u8; 32];
    let id_bytes = contact_id.as_bytes();
    let len = id_bytes.len().min(32);
    public_key[..len].copy_from_slice(&id_bytes[..len]);

    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange(public_key, card, shared_key);
    storage.save_contact(&contact).unwrap();
}

fn setup_shred_env() -> (tempfile::TempDir, Storage, MemoryKeyStorage, Identity) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, SymmetricKey::generate()).unwrap();
    let secure_storage = MemoryKeyStorage::new();
    let identity = Identity::create("TestUser");

    let smk = ShreddingMasterKey::derive_from_seed(&[0x42; 32]);
    secure_storage
        .save_key(SMK_KEY_NAME, smk.as_bytes())
        .unwrap();

    (dir, storage, secure_storage, identity)
}

// ============================================================
// C1 — Grace Period and Full Erasure
// ============================================================

// @scenario: privacy_compliance :: Execute deletion requires grace period to have elapsed
/// C1-1: Deletion cannot be executed during the grace period.
///
/// After `schedule_deletion()` is called, `execute_deletion()` must return an
/// error and all local data (contacts, identity state) must survive intact.
// @internal
#[test]
fn test_erasure_blocked_during_grace_period() {
    let storage = in_memory_storage();
    let manager = DeletionManager::new(&storage);

    // Seed storage with a contact to confirm it survives the blocked attempt.
    add_test_contact(&storage, "contact_blocked");
    let contacts_before = storage.list_contacts().unwrap();
    assert_eq!(
        contacts_before.len(),
        1,
        "Expected 1 contact before blocked erasure"
    );

    manager.schedule_deletion().unwrap();

    // Execution must be rejected — grace period has not elapsed.
    let identity = Identity::create("Alice");
    let result = manager.execute_deletion(&identity);
    assert!(
        result.is_err(),
        "execute_deletion must fail when grace period has not elapsed"
    );

    // Data must be intact.
    let contacts_after = storage.list_contacts().unwrap();
    assert_eq!(
        contacts_after.len(),
        1,
        "Contacts must survive a blocked erasure attempt"
    );

    // State must still be Scheduled, not Executed.
    let state = manager.deletion_state().unwrap();
    assert!(
        matches!(state, DeletionState::Scheduled { .. }),
        "Deletion state must remain Scheduled after a blocked attempt, got {:?}",
        state
    );
}

/// C1-2: Full erasure after grace period produces revocations for all contacts.
///
/// Uses `schedule_deletion_with_execute_at` with past timestamps to bypass the
/// wall-clock wait. Verifies that one `IdentityRevoked` message is generated per
/// contact and that the deletion state transitions to `Executed`.
// @scenario: privacy_compliance :: Full erasure after grace period
// @internal
#[test]
fn test_full_erasure_after_grace_period() {
    let storage = in_memory_storage();
    let manager = DeletionManager::new(&storage);

    add_test_contact(&storage, "contact_alpha");
    add_test_contact(&storage, "contact_beta");
    add_test_contact(&storage, "contact_gamma");

    // Schedule with execute_at in the past so grace period has elapsed.
    manager
        .schedule_deletion_with_execute_at(1000, 999)
        .unwrap();

    let identity = Identity::create("Alice");
    let result = manager.execute_deletion(&identity).unwrap();

    // One revocation must be generated per contact.
    assert_eq!(
        result.revocations.len(),
        3,
        "Expected exactly one revocation per contact, got {}",
        result.revocations.len()
    );

    // All revocations must be for distinct recipients.
    let mut recipients: Vec<&str> = result
        .revocations
        .iter()
        .map(|r| r.recipient_id.as_str())
        .collect();
    recipients.sort_unstable();
    recipients.dedup();
    assert_eq!(
        recipients.len(),
        3,
        "All revocations must target distinct contacts"
    );

    // Deletion state must be Executed.
    let state = manager.deletion_state().unwrap();
    assert!(
        matches!(state, DeletionState::Executed { .. }),
        "State must be Executed after successful erasure, got {:?}",
        state
    );

    // Contacts must be gone (deletion clears the rows).
    let remaining = storage.list_contacts().unwrap();
    assert_eq!(
        remaining.len(),
        0,
        "All contacts must be removed after execution"
    );
}

// @scenario: privacy_compliance :: Cancel deletion during grace period
/// C1-3: Cancelling during the grace period restores full access.
///
/// After cancellation: state is None, contacts are untouched, and the user
/// can schedule again without error.
// @internal
#[test]
fn test_cancellation_restores_full_access() {
    let storage = in_memory_storage();
    let manager = DeletionManager::new(&storage);

    add_test_contact(&storage, "contact_x");
    add_test_contact(&storage, "contact_y");

    manager.schedule_deletion().unwrap();

    // Verify deletion is scheduled.
    let state_after_schedule = manager.deletion_state().unwrap();
    assert!(
        matches!(state_after_schedule, DeletionState::Scheduled { .. }),
        "Expected Scheduled state before cancel"
    );

    // Cancel within the grace period.
    manager.cancel_deletion().unwrap();

    // State must return to None.
    let state_after_cancel = manager.deletion_state().unwrap();
    assert!(
        matches!(state_after_cancel, DeletionState::None),
        "State must be None after cancel, got {:?}",
        state_after_cancel
    );

    // All contacts must still be present.
    let contacts = storage.list_contacts().unwrap();
    assert_eq!(
        contacts.len(),
        2,
        "Contacts must survive cancellation, got {}",
        contacts.len()
    );

    // User must be able to reschedule after cancel.
    let reschedule_result = manager.schedule_deletion();
    assert!(
        reschedule_result.is_ok(),
        "Rescheduling after cancel must succeed"
    );
    let state_rescheduled = manager.deletion_state().unwrap();
    assert!(
        matches!(state_rescheduled, DeletionState::Scheduled { .. }),
        "State must be Scheduled after rescheduling"
    );
}

// ============================================================
// C2 — Crypto-Shredding Key Hierarchy Soundness
// ============================================================

/// C2-1: Different SMK seeds produce different SEKs.
///
/// Verifies the foundational property that destroys the SMK makes data
/// irrecoverable: two distinct seeds → two distinct SMKs → two distinct SEKs.
/// Both the SMK and SEK bytes must differ.
// @scenario: privacy_compliance :: Crypto-shredding key hierarchy is sound
// @internal
#[test]
fn test_crypto_shredding_key_hierarchy_is_sound() {
    let seed_a: [u8; 32] = [0xAA; 32];
    let seed_b: [u8; 32] = [0xBB; 32];

    let smk_a = ShreddingMasterKey::derive_from_seed(&seed_a);
    let smk_b = ShreddingMasterKey::derive_from_seed(&seed_b);

    // SMKs must differ.
    assert_ne!(
        smk_a.as_bytes(),
        smk_b.as_bytes(),
        "Different seeds must produce different SMKs"
    );

    let sek_a = smk_a.derive_sek();
    let sek_b = smk_b.derive_sek();

    // SEKs must differ — destroying SMK_A makes SEK_A unrecoverable.
    assert_ne!(
        sek_a.as_bytes(),
        sek_b.as_bytes(),
        "Different SMKs must produce different SEKs — \
         destroying SMK must render SEK-encrypted data irrecoverable"
    );

    // SEK must also differ from the raw seed (no accidental key identity).
    assert_ne!(sek_a.as_bytes(), &seed_a, "SEK must not equal the raw seed");
}

// ============================================================
// C3 — ShredManager Integration
// ============================================================

struct MockPurgeSender {
    purge_sent: bool,
}

impl MockPurgeSender {
    fn new() -> Self {
        Self { purge_sent: false }
    }
}

impl PurgeSender for MockPurgeSender {
    fn send_purge(&mut self, _purge: &PreSignedPurgeRequest) -> Result<bool, ShredError> {
        self.purge_sent = true;
        Ok(true)
    }
}

struct MockRevocationSender {
    recipient_ids: Vec<String>,
}

impl MockRevocationSender {
    fn new() -> Self {
        Self {
            recipient_ids: Vec::new(),
        }
    }

    fn sent_count(&self) -> usize {
        self.recipient_ids.len()
    }
}

impl RevocationSender for MockRevocationSender {
    fn send_revocation(&mut self, revocation: &IdentityRevoked) -> Result<bool, ShredError> {
        self.recipient_ids.push(revocation.recipient_id.clone());
        Ok(true)
    }
}

// @scenario: privacy_compliance :: Hard shred sends purge and revocations
/// C3-1: `hard_shred` sends purge to relay and revocations for all contacts.
///
/// Sets up ShredManager with mock senders, adds contacts, bypasses the grace
/// period, and verifies the report reflects exactly the expected notifications.
// @internal
#[test]
fn test_hard_shred_sends_purge_and_revocations() {
    let (dir, storage, secure_storage, identity) = setup_shred_env();

    add_test_contact(&storage, "contact_one");
    add_test_contact(&storage, "contact_two");

    // Bypass grace period.
    let dm = DeletionManager::new(&storage);
    dm.schedule_deletion_with_execute_at(1000, 999).unwrap();

    let manager = ShredManager::new(&storage, &secure_storage, &identity, dir.path());
    let token = ShredToken::from_created_at(1000);

    let mut purge_sender = MockPurgeSender::new();
    let mut revocation_sender = MockRevocationSender::new();

    let report = manager
        .hard_shred(token, Some(&mut purge_sender), Some(&mut revocation_sender))
        .unwrap();

    // Purge must have been sent.
    assert!(
        purge_sender.purge_sent,
        "Purge request must be sent to relay during hard shred"
    );
    assert!(
        report.relay_purge_sent,
        "ShredReport must reflect relay_purge_sent = true"
    );

    // Revocations must be sent for all contacts.
    assert_eq!(
        revocation_sender.sent_count(),
        2,
        "Expected 2 revocations (one per contact), got {}",
        revocation_sender.sent_count()
    );
    assert_eq!(
        report.contacts_notified, 2,
        "ShredReport.contacts_notified must equal the number of contacts"
    );

    // SMK must be destroyed.
    assert!(
        report.smk_destroyed,
        "ShredReport must indicate SMK was destroyed"
    );
    assert!(
        secure_storage.load_key(SMK_KEY_NAME).unwrap().is_none(),
        "SMK must be absent from SecureStorage after hard shred"
    );
}
