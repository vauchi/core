// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Orchestrator
//!
//! Main entry point for the Vauchi API.

use std::sync::{Arc, Mutex};

use crate::contact::Contact;
use crate::contact_card::{ContactCard, ContactField};
use crate::crypto::ratchet::DoubleRatchetState;
use crate::crypto::{ShreddingMasterKey, SymmetricKey};
use crate::identity::Identity;
use crate::network::{MockTransport, Transport};
use crate::storage::{SecureStorage, Storage};
use crate::sync::state::ReplayDetector;

use super::app_password::{AppPasswordConfig, AuthResult};
use super::config::VauchiConfig;
use super::consent::{ConsentManager, ConsentRecord, ConsentType};
use super::contact_manager::ContactManager;
use super::duress::{DuressAlert, DuressAlertType, DuressSettings};
use super::emergency::{BroadcastResult, EmergencyBroadcastConfig, MAX_TRUSTED_CONTACTS};
use super::error::{VauchiError, VauchiResult};
use super::events::{EventDispatcher, EventHandler, VauchiEvent};

/// Authentication mode for the Vauchi instance.
///
/// Determines which data is shown to the user. The password system is
/// opt-in: without a password, the mode is `Unauthenticated` and behaves
/// identically to the legacy (pre-password) behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// The normal (real) password was used — show real contacts.
    Normal,
    /// The duress PIN was used — show decoy contacts only.
    Duress,
    /// No password is set — backward-compatible, show real contacts.
    Unauthenticated,
}

/// Main Vauchi orchestrator.
///
/// This is the primary entry point for using Vauchi. It coordinates:
/// - Identity management
/// - Contact management
/// - Synchronization
/// - Event dispatching
///
/// # Example
///
/// ```ignore
/// use vauchi_core::api::{Vauchi, VauchiConfig};
///
/// // Create Vauchi with default config
/// let mut wb = Vauchi::new(VauchiConfig::default())?;
///
/// // Create identity
/// wb.create_identity("Alice")?;
///
/// // Add event handler
/// wb.add_event_handler(|event| {
///     println!("Event: {:?}", event);
/// });
///
/// // Update contact card
/// let mut card = wb.own_card()?.unwrap();
/// card.add_field(ContactField::new(FieldType::Email, "email", "alice@example.com"));
/// wb.update_own_card(&card)?;
///
/// // Connect and sync
/// wb.connect()?;
/// wb.sync()?;
/// ```
/// Key name used to store SMK in SecureStorage.
const SMK_KEY_NAME: &str = "smk";

pub struct Vauchi<T: Transport = MockTransport> {
    config: VauchiConfig,
    identity: Option<Identity>,
    storage: Storage,
    events: Arc<EventDispatcher>,
    secure_storage: Option<Arc<dyn SecureStorage>>,
    replay_detector: Mutex<ReplayDetector>,
    auth_mode: AuthMode,
    /// In-memory queue of duress alerts waiting to be sent.
    ///
    /// Populated when `authenticate()` detects a duress PIN. Alerts are
    /// drained by the sync system and sent as card updates to trusted
    /// contacts, indistinguishable from normal sync traffic.
    duress_alerts: Vec<DuressAlert>,
    _phantom: std::marker::PhantomData<T>,
}

impl Vauchi<MockTransport> {
    /// Creates a new Vauchi instance with mock transport (for testing).
    pub fn new(config: VauchiConfig) -> VauchiResult<Self> {
        Self::with_transport_factory(config, MockTransport::new)
    }

    /// Creates a new Vauchi instance using SMK from SecureStorage for encryption.
    ///
    /// Boot flow (DP-1): Load SMK from SecureStorage → derive SEK → open Storage.
    /// Falls back to `config.storage_key` if SMK is not found in SecureStorage.
    pub fn with_secure_storage(
        config: VauchiConfig,
        secure_storage: Arc<dyn SecureStorage>,
    ) -> VauchiResult<Self> {
        Self::with_transport_and_secure_storage(config, MockTransport::new, Some(secure_storage))
    }
}

impl<T: Transport> Vauchi<T> {
    /// Creates a new Vauchi instance with a custom transport factory.
    pub fn with_transport_factory<F>(
        config: VauchiConfig,
        transport_factory: F,
    ) -> VauchiResult<Self>
    where
        F: FnOnce() -> T,
    {
        Self::with_transport_and_secure_storage(config, transport_factory, None)
    }

    /// Creates a new Vauchi instance with transport factory and optional SecureStorage.
    ///
    /// If SecureStorage is provided and contains an SMK, the SEK is derived from it.
    /// Otherwise, falls back to `config.storage_key` or generates a random key.
    pub fn with_transport_and_secure_storage<F>(
        config: VauchiConfig,
        _transport_factory: F,
        secure_storage: Option<Arc<dyn SecureStorage>>,
    ) -> VauchiResult<Self>
    where
        F: FnOnce() -> T,
    {
        // Determine the storage encryption key
        let storage_key = Self::resolve_storage_key(&config, secure_storage.as_deref())?;

        // Open or create storage
        let storage = if config.storage_path.exists() {
            Storage::open(&config.storage_path, storage_key)?
        } else {
            // Create parent directories if needed
            if let Some(parent) = config.storage_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| VauchiError::Configuration(e.to_string()))?;
            }
            Storage::open(&config.storage_path, storage_key)?
        };

        let events = Arc::new(EventDispatcher::new());

        Ok(Vauchi {
            config,
            identity: None,
            storage,
            events,
            secure_storage,
            replay_detector: Mutex::new(ReplayDetector::default_tolerance()),
            auth_mode: AuthMode::Unauthenticated,
            duress_alerts: Vec::new(),
            _phantom: std::marker::PhantomData,
        })
    }

    /// Resolves the storage encryption key from available sources.
    ///
    /// Priority:
    /// 1. SMK from SecureStorage → derive SEK
    /// 2. Explicit storage_key from config
    /// 3. Generate random key (ephemeral, not persistent)
    fn resolve_storage_key(
        config: &VauchiConfig,
        secure_storage: Option<&dyn SecureStorage>,
    ) -> VauchiResult<SymmetricKey> {
        // Try loading SMK from SecureStorage
        if let Some(ss) = secure_storage {
            if let Some(smk_bytes) = ss.load_key(SMK_KEY_NAME).map_err(|e| {
                VauchiError::Configuration(format!("Failed to load SMK from SecureStorage: {}", e))
            })? {
                let smk_array: [u8; 32] = smk_bytes.try_into().map_err(|_| {
                    VauchiError::Configuration("SMK in SecureStorage has invalid length".into())
                })?;
                let smk = ShreddingMasterKey::from_bytes(smk_array);
                return Ok(smk.derive_sek());
            }
        }

        // Fall back to config storage key or generate random
        Ok(config
            .storage_key
            .clone()
            .unwrap_or_else(SymmetricKey::generate))
    }

    /// Creates a new Vauchi instance with in-memory storage (for testing).
    pub fn in_memory() -> VauchiResult<Self>
    where
        T: Default,
    {
        let storage_key = SymmetricKey::generate();
        let storage = Storage::in_memory(storage_key)?;
        let events = Arc::new(EventDispatcher::new());

        Ok(Vauchi {
            config: VauchiConfig::default(),
            identity: None,
            storage,
            events,
            secure_storage: None,
            replay_detector: Mutex::new(ReplayDetector::default_tolerance()),
            auth_mode: AuthMode::Unauthenticated,
            duress_alerts: Vec::new(),
            _phantom: std::marker::PhantomData,
        })
    }

    // === Identity Operations ===

    /// Sets the SecureStorage backend for SMK persistence.
    ///
    /// Call this before `create_identity()` to enable SMK-based encryption,
    /// or before `migrate_to_smk()` for upgrading existing installations.
    pub fn set_secure_storage(&mut self, secure_storage: Arc<dyn SecureStorage>) {
        self.secure_storage = Some(secure_storage);
    }

    /// Returns a reference to the SecureStorage, if set.
    pub fn secure_storage(&self) -> Option<&dyn SecureStorage> {
        self.secure_storage.as_deref()
    }

    /// Creates a new identity with the given display name.
    ///
    /// If SecureStorage is set, derives SMK from the identity's master seed,
    /// stores it in SecureStorage, and re-encrypts storage with the SMK-derived SEK.
    pub fn create_identity(&mut self, display_name: &str) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }

        let identity = Identity::create(display_name);

        // Create initial contact card from identity
        let card = ContactCard::new(display_name);
        self.storage.save_own_card(&card)?;

        // If SecureStorage is available, derive and store SMK, then rekey storage
        if let Some(ref ss) = self.secure_storage {
            let smk = identity.derive_smk();

            // Store SMK in SecureStorage BEFORE rekey (safety: see DP-1 rationale)
            ss.save_key(SMK_KEY_NAME, smk.as_bytes())
                .map_err(|e| VauchiError::Configuration(format!("Failed to store SMK: {}", e)))?;

            // Derive SEK and rekey storage
            let sek = smk.derive_sek();
            self.storage.rekey(sek).map_err(|e| {
                VauchiError::Configuration(format!("Failed to rekey storage: {}", e))
            })?;
        }

        self.identity = Some(identity);
        Ok(())
    }

    /// Migrates an existing installation from old storage_key to SMK-derived SEK.
    ///
    /// Requires:
    /// - SecureStorage is set (`set_secure_storage()` called)
    /// - Identity is loaded (via `set_identity()` or `load_identity()`)
    /// - Storage is open with the old key
    ///
    /// Flow (see Phase 2a.3):
    /// 1. Derive SMK from identity's master_seed
    /// 2. Store SMK in SecureStorage (before rekey for safety)
    /// 3. Derive SEK from SMK
    /// 4. Rekey all encrypted columns to SEK
    pub fn migrate_to_smk(&mut self) -> VauchiResult<()> {
        let ss = self
            .secure_storage
            .as_ref()
            .ok_or_else(|| VauchiError::Configuration("SecureStorage not set".into()))?;

        // Check if already migrated
        if ss
            .has_key(SMK_KEY_NAME)
            .map_err(|e| VauchiError::Configuration(format!("Failed to check SMK: {}", e)))?
        {
            return Ok(()); // Already migrated
        }

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let smk = identity.derive_smk();

        // Store SMK in SecureStorage BEFORE rekey
        ss.save_key(SMK_KEY_NAME, smk.as_bytes())
            .map_err(|e| VauchiError::Configuration(format!("Failed to store SMK: {}", e)))?;

        // Derive SEK and rekey storage
        let sek = smk.derive_sek();
        self.storage
            .rekey(sek)
            .map_err(|e| VauchiError::Configuration(format!("Failed to rekey storage: {}", e)))?;

        Ok(())
    }

    /// Sets an existing identity.
    pub fn set_identity(&mut self, identity: Identity) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }
        self.identity = Some(identity);
        Ok(())
    }

    /// Returns the current identity, if set.
    pub fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }

    /// Returns the public ID of the current identity.
    pub fn public_id(&self) -> VauchiResult<String> {
        self.identity
            .as_ref()
            .map(|id| id.public_id())
            .ok_or(VauchiError::IdentityNotInitialized)
    }

    /// Returns true if an identity has been created or set.
    pub fn has_identity(&self) -> bool {
        self.identity.is_some()
    }

    /// Updates the user's display name.
    ///
    /// Updates both the identity and contact card display name.
    /// Returns an error if:
    /// - No identity is set
    /// - The name is empty or whitespace-only
    /// - The name exceeds 100 characters
    pub fn update_display_name(&mut self, new_name: &str) -> VauchiResult<()> {
        let name = new_name.trim();

        if name.is_empty() {
            return Err(VauchiError::InvalidState(
                "Display name cannot be empty".into(),
            ));
        }
        if name.len() > 100 {
            return Err(VauchiError::InvalidState(
                "Display name cannot exceed 100 characters".into(),
            ));
        }

        // Get mutable reference to identity
        let identity = self
            .identity
            .as_mut()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Update identity display name
        identity.set_display_name(name);

        // Update contact card display name
        let mut card = self
            .storage
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(name));
        card.set_display_name(name)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.save_own_card(&card)?;

        Ok(())
    }

    // === Contact Card Operations ===

    /// Gets the user's own contact card.
    pub fn own_card(&self) -> VauchiResult<Option<ContactCard>> {
        Ok(self.storage.load_own_card()?)
    }

    /// Updates the user's own contact card.
    pub fn update_own_card(&self, card: &ContactCard) -> VauchiResult<Vec<String>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.update_own_card(card)
    }

    /// Adds a field to the user's own card.
    pub fn add_own_field(&self, field: ContactField) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_field_to_own_card(field)
    }

    /// Removes a field from the user's own card.
    pub fn remove_own_field(&self, label: &str) -> VauchiResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.remove_field_from_own_card(label)
    }

    // === Contact Operations ===

    /// Gets a contact by ID.
    pub fn get_contact(&self, id: &str) -> VauchiResult<Option<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.get_contact(id)
    }

    /// Lists all contacts, respecting the current auth mode.
    ///
    /// - **Normal** or **Unauthenticated**: Returns real contacts (filtered
    ///   by hidden status, as before).
    /// - **Duress**: Returns decoy contacts only, presented as real contacts.
    pub fn list_contacts(&self) -> VauchiResult<Vec<Contact>> {
        match self.auth_mode {
            AuthMode::Duress => {
                // Load decoy contacts and convert to Contact structs
                let decoys = self.storage.load_decoy_contacts()?;
                Ok(decoys
                    .into_iter()
                    .map(|(id, _display_name, card)| {
                        Contact::from_exchange(
                            // Use a deterministic "public key" derived from the ID
                            // (decoys don't have real keys — this is display-only)
                            decoy_id_to_fake_pk(&id),
                            card,
                            crate::crypto::SymmetricKey::generate(),
                        )
                    })
                    .collect())
            }
            AuthMode::Normal | AuthMode::Unauthenticated => {
                let manager = ContactManager::new(&self.storage, self.events.clone());
                manager.list_contacts()
            }
        }
    }

    /// Lists contacts with pagination.
    pub fn list_contacts_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> VauchiResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.list_contacts_paginated(offset, limit)
    }

    /// Searches contacts by display name.
    pub fn search_contacts(&self, query: &str) -> VauchiResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.search_contacts(query)
    }

    /// Returns the number of contacts.
    pub fn contact_count(&self) -> VauchiResult<usize> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.contact_count()
    }

    /// Adds a new contact from an exchange.
    pub fn add_contact(&self, contact: Contact) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_contact(contact)
    }

    /// Removes a contact by ID.
    pub fn remove_contact(&self, id: &str) -> VauchiResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.remove_contact(id)
    }

    /// Updates an existing contact.
    pub fn update_contact(&self, contact: &Contact) -> VauchiResult<()> {
        self.storage.save_contact(contact)?;
        Ok(())
    }

    /// Verifies a contact's fingerprint.
    pub fn verify_contact_fingerprint(&self, id: &str) -> VauchiResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.verify_fingerprint(id)
    }

    // === Double Ratchet Operations ===

    /// Gets the Double Ratchet state for a contact.
    pub fn get_ratchet_state(&self, contact_id: &str) -> VauchiResult<Option<DoubleRatchetState>> {
        Ok(self.storage.load_ratchet_state(contact_id)?.map(|(r, _)| r))
    }

    /// Saves a Double Ratchet state for a contact.
    ///
    /// If a ratchet state already exists, preserves the is_initiator flag.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
    ) -> VauchiResult<()> {
        // Load existing to preserve is_initiator flag
        let is_initiator = self
            .storage
            .load_ratchet_state(contact_id)?
            .map(|(_, i)| i)
            .unwrap_or(true);
        self.storage
            .save_ratchet_state(contact_id, state, is_initiator)?;
        Ok(())
    }

    /// Creates and saves a new ratchet state for a contact as initiator.
    pub fn create_ratchet_as_initiator(
        &self,
        contact_id: &str,
        shared_secret: &SymmetricKey,
        their_dh_public: [u8; 32],
    ) -> VauchiResult<()> {
        let ratchet = DoubleRatchetState::initialize_initiator(shared_secret, their_dh_public);
        self.storage
            .save_ratchet_state(contact_id, &ratchet, true)?;
        Ok(())
    }

    /// Creates and saves a new ratchet state for a contact as responder.
    pub fn create_ratchet_as_responder(
        &self,
        contact_id: &str,
        shared_secret: &SymmetricKey,
        our_dh: crate::exchange::X3DHKeyPair,
    ) -> VauchiResult<()> {
        let ratchet = DoubleRatchetState::initialize_responder(shared_secret, our_dh);
        self.storage
            .save_ratchet_state(contact_id, &ratchet, false)?;
        Ok(())
    }

    // === Card Propagation Operations ===

    /// Propagates own card update to all contacts.
    ///
    /// For each contact with an established ratchet:
    /// 1. Computes delta between old and new card
    /// 2. Signs delta with our identity
    /// 3. If contact has CEK: wraps in `CekWrappedPayload` (version 0x02), rotates CEK
    /// 4. If contact has no CEK: uses legacy format (raw JSON bytes)
    /// 5. Encrypts with contact's ratchet
    /// 6. Queues for delivery via relay
    ///
    /// Returns the number of contacts queued for update.
    pub fn propagate_card_update(
        &self,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> VauchiResult<usize> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let contacts = self.storage.list_contacts()?;
        let mut queued = 0;

        for mut contact in contacts {
            // Skip blocked contacts
            if contact.is_blocked() {
                continue;
            }

            // Skip contacts without ratchet (not yet synced)
            let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact.id())? {
                Some(r) => r,
                None => continue,
            };

            // Compute delta
            let delta = CardDelta::compute(old_card, new_card);
            if delta.is_empty() {
                continue;
            }

            // Filter delta based on visibility rules for this contact
            let mut delta = delta.filter_for_contact(contact.id(), contact.visibility_rules());
            if delta.is_empty() {
                continue;
            }

            // Sign delta with our identity, bound to recipient
            delta.sign(identity, contact.public_key());

            // Serialize delta
            let delta_bytes = serde_json::to_vec(&delta)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Wrap with CEK if contact has one (version 0x02), otherwise legacy
            let payload_bytes = if contact.cek().is_some() {
                // Rotate CEK
                let new_cek = ContentEncryptionKey::generate();

                // Encrypt delta with new CEK
                let cek_ciphertext = new_cek
                    .encrypt(&delta_bytes)
                    .map_err(|e| VauchiError::Crypto(format!("CEK encrypt: {:?}", e)))?;

                // Build wrapped payload
                let wrapped = CekWrappedPayload {
                    cek: new_cek.to_bytes(),
                    cek_ciphertext,
                    signature: delta.signature,
                    nonce: delta.nonce,
                };

                // Update contact with rotated CEK and re-save
                // (re-encrypts card at rest with new CEK)
                contact.set_cek(new_cek);
                self.storage.save_contact(&contact)?;

                // Version-tagged encoding
                VersionedPayload::encode_cek(&wrapped)
            } else {
                // Legacy format: raw delta JSON bytes
                delta_bytes
            };

            // Encrypt with ratchet
            let ratchet_msg = ratchet
                .encrypt(&payload_bytes)
                .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Save updated ratchet state
            self.storage
                .save_ratchet_state(contact.id(), &ratchet, is_initiator)?;

            // Queue for delivery
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let update = PendingUpdate {
                id: format!("{}-{}", contact.id(), now),
                contact_id: contact.id().to_string(),
                update_type: "card_delta".to_string(),
                payload: encrypted,
                created_at: now,
                retry_count: 0,
                status: UpdateStatus::Pending,
            };
            self.storage.queue_update(&update)?;
            queued += 1;
        }

        Ok(queued)
    }

    /// Processes an encrypted card update from a contact.
    ///
    /// 1. Checks revoked_senders tombstone — rejects updates from revoked senders
    /// 2. Decrypts the update using the contact's ratchet
    /// 3. Detects payload version:
    ///    - Version 0x02 (CEK-wrapped): extracts CEK, decrypts delta, saves CEK
    ///    - Version 0x01 or raw JSON (legacy): parses delta directly
    /// 4. Verifies the signature using the contact's public key
    /// 5. Applies the delta to the contact's card
    ///
    /// Returns a list of changed field labels.
    pub fn process_card_update(
        &self,
        contact_id: &str,
        encrypted: &[u8],
    ) -> VauchiResult<Vec<String>> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::crypto::ratchet::RatchetMessage;
        use crate::sync::delta::{CardDelta, VersionedPayload, PAYLOAD_VERSION_CEK};

        // Check revoked_senders tombstone
        if self.storage.is_sender_revoked(contact_id)? {
            return Err(VauchiError::InvalidState(
                "update from revoked sender".to_string(),
            ));
        }

        // Reject updates from blocked contacts
        if let Some(contact) = self.storage.load_contact(contact_id)? {
            if contact.is_blocked() {
                return Err(VauchiError::ContactBlocked(contact_id.to_string()));
            }
        }

        // Load contact
        let mut contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", contact_id)))?;

        // Load and decrypt with ratchet
        let (mut ratchet, is_initiator) = self
            .storage
            .load_ratchet_state(contact_id)?
            .ok_or_else(|| VauchiError::NotFound("ratchet state".into()))?;

        let ratchet_msg: RatchetMessage = serde_json::from_slice(encrypted)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;
        let plaintext = ratchet
            .decrypt(&ratchet_msg)
            .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;

        // Detect payload version and extract delta bytes + optional CEK
        let (delta_bytes, new_cek) = if !plaintext.is_empty() && plaintext[0] == PAYLOAD_VERSION_CEK
        {
            // Version 0x02: CEK-wrapped payload
            match VersionedPayload::decode(&plaintext) {
                Ok(VersionedPayload::CekWrapped(wrapped)) => {
                    let cek = ContentEncryptionKey::from_bytes(wrapped.cek);
                    let decrypted = cek
                        .decrypt(&wrapped.cek_ciphertext)
                        .map_err(|e| VauchiError::Crypto(format!("CEK decrypt: {:?}", e)))?;
                    (decrypted, Some(cek))
                }
                Ok(VersionedPayload::Legacy(data)) => (data, None),
                Err(e) => {
                    return Err(VauchiError::Serialization(format!("payload decode: {}", e)));
                }
            }
        } else {
            // Legacy: raw JSON bytes (no version tag, or version 0x01)
            match VersionedPayload::decode(&plaintext) {
                Ok(VersionedPayload::Legacy(data)) => (data, None),
                _ => {
                    // Fall back to treating entire plaintext as legacy delta JSON
                    (plaintext, None)
                }
            }
        };

        // Parse delta
        let delta: CardDelta = serde_json::from_slice(&delta_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Verify signature with contact's (sender) and our (recipient) public keys
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        if !delta.verify(contact.public_key(), identity.signing_public_key()) {
            return Err(VauchiError::SignatureInvalid);
        }

        // Check for replay attack
        {
            let mut detector = self
                .replay_detector
                .lock()
                .map_err(|_| VauchiError::InvalidState("replay detector poisoned".into()))?;
            if !detector.check_replay(contact_id, &delta.nonce, delta.timestamp) {
                return Err(VauchiError::ReplayDetected);
            }
        }

        // Reject stale/downgraded delta versions (#42)
        let last_version = self.storage.last_delta_version(contact_id).unwrap_or(0);
        if delta.version > 0 && delta.version < last_version {
            return Err(VauchiError::InvalidState(format!(
                "stale delta version {} (last applied: {})",
                delta.version, last_version
            )));
        }

        // Get changed fields before applying
        let changed = delta.changed_fields();

        // Apply delta to contact's card
        let mut new_card = contact.card().clone();
        delta
            .apply(&mut new_card)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        // Update contact card and CEK atomically
        contact.update_card(new_card);
        if let Some(cek) = new_cek {
            contact.set_cek(cek);
        }

        // All DB writes in a single transaction: ratchet state, replay nonce, contact card.
        // If any write fails, all are rolled back to prevent inconsistent state.
        self.storage.begin_transaction()?;
        let result = (|| -> VauchiResult<()> {
            self.storage
                .save_ratchet_state(contact_id, &ratchet, is_initiator)?;
            self.storage
                .save_replay_nonce(contact_id, &delta.nonce, delta.timestamp)?;
            self.storage.save_contact(&contact)?;
            // Track delta version for downgrade detection (#42)
            if delta.version > 0 {
                self.storage.record_delta_version(contact_id, delta.version)?;
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.storage.commit()?;
                Ok(changed)
            }
            Err(e) => {
                self.storage.rollback();
                Err(e)
            }
        }
    }

    // === CEK Migration ===

    /// Migrates legacy contacts to CEK-protected format.
    ///
    /// For each contact that has an established ratchet but no CEK:
    /// 1. Generates a new CEK
    /// 2. Saves the CEK locally
    /// 3. Queues a migration update (empty delta carrying the CEK) for relay delivery
    ///
    /// Returns the number of contacts migrated.
    pub fn migrate_contacts_to_cek(&self) -> VauchiResult<usize> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let own_card = self
            .storage
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let contacts = self.storage.list_contacts()?;
        let mut migrated = 0;

        for mut contact in contacts {
            // Skip contacts that already have a CEK
            if contact.cek().is_some() {
                continue;
            }

            // Skip contacts without ratchet (can't send updates)
            let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact.id())? {
                Some(r) => r,
                None => continue,
            };

            // Generate a new CEK for this contact
            let cek = ContentEncryptionKey::generate();

            // Create a no-op delta (empty changes — just carries the CEK)
            let mut delta = CardDelta::compute(&own_card, &own_card);
            // Force a nonce so the delta is processable even with no changes
            delta.sign(identity, contact.public_key());

            // Serialize and CEK-encrypt the delta
            let delta_bytes = serde_json::to_vec(&delta)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;
            let cek_ciphertext = cek
                .encrypt(&delta_bytes)
                .map_err(|e| VauchiError::Crypto(format!("CEK encrypt: {:?}", e)))?;

            let wrapped = CekWrappedPayload {
                cek: cek.to_bytes(),
                cek_ciphertext,
                signature: delta.signature,
                nonce: delta.nonce,
            };
            let payload_bytes = VersionedPayload::encode_cek(&wrapped);

            // Ratchet-encrypt
            let ratchet_msg = ratchet
                .encrypt(&payload_bytes)
                .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Save updated ratchet state
            self.storage
                .save_ratchet_state(contact.id(), &ratchet, is_initiator)?;

            // Set CEK on contact and re-save (re-encrypts card at rest with CEK)
            contact.set_cek(cek);
            self.storage.save_contact(&contact)?;

            // Queue for delivery
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let update = PendingUpdate {
                id: format!("{}-cek-migrate-{}", contact.id(), now),
                contact_id: contact.id().to_string(),
                update_type: "cek_migration".to_string(),
                payload: encrypted,
                created_at: now,
                retry_count: 0,
                status: UpdateStatus::Pending,
            };
            self.storage.queue_update(&update)?;
            migrated += 1;
        }

        Ok(migrated)
    }

    // === Event Operations ===

    /// Adds an event handler.
    pub fn add_event_handler(&mut self, handler: Arc<dyn EventHandler>) {
        if let Some(events) = Arc::get_mut(&mut self.events) {
            events.add_handler(handler);
        }
    }

    /// Clears all event handlers.
    pub fn clear_event_handlers(&mut self) {
        if let Some(events) = Arc::get_mut(&mut self.events) {
            events.clear_handlers();
        }
    }

    /// Dispatches an event to all handlers.
    pub fn dispatch_event(&self, event: VauchiEvent) {
        self.events.dispatch(event);
    }

    // === App Password / Duress PIN ===

    /// Returns the current authentication mode.
    pub fn auth_mode(&self) -> AuthMode {
        self.auth_mode
    }

    /// Authenticates with a password.
    ///
    /// Loads the password configuration from storage, verifies the password,
    /// and sets the auth mode accordingly:
    /// - `Normal` if the real password matches
    /// - `Duress` if the duress PIN matches
    /// - Returns an error if neither matches
    pub fn authenticate(&mut self, password: &str) -> VauchiResult<AuthMode> {
        let config = self
            .storage
            .load_password_config()?
            .ok_or_else(|| VauchiError::InvalidState("no password configured".into()))?;

        match config.verify(password) {
            AuthResult::Normal => {
                self.auth_mode = AuthMode::Normal;
                Ok(AuthMode::Normal)
            }
            AuthResult::Duress => {
                self.auth_mode = AuthMode::Duress;
                self.queue_duress_alert()?;
                Ok(AuthMode::Duress)
            }
            AuthResult::Invalid => Err(VauchiError::InvalidState("invalid password".into())),
        }
    }

    /// Sets up an app password (PIN).
    ///
    /// Requires an identity to be created first (the password columns
    /// live on the `identity` table). If the identity row doesn't exist
    /// in the database yet, it is created with a placeholder.
    pub fn setup_app_password(&mut self, password: &str) -> VauchiResult<()> {
        if self.identity.is_none() {
            return Err(VauchiError::IdentityNotInitialized);
        }

        // Ensure the identity row exists in DB (may not yet if create_identity
        // only stored the own_card). Insert a placeholder row if missing.
        if !self.storage.has_identity()? {
            self.storage.save_identity(b"", "")?;
        }

        let config = AppPasswordConfig::create(password)?;
        self.storage
            .save_app_password(config.password_hash(), config.password_salt())?;

        Ok(())
    }

    /// Sets up a duress PIN.
    ///
    /// Requires an app password to be configured first.
    pub fn setup_duress_password(&mut self, duress_password: &str) -> VauchiResult<()> {
        let mut config = self.storage.load_password_config()?.ok_or_else(|| {
            VauchiError::InvalidState("app password must be set before duress PIN".into())
        })?;

        config.setup_duress(duress_password)?;

        let duress_hash = config
            .duress_hash()
            .ok_or_else(|| VauchiError::InvalidState("duress hash not set".into()))?;
        let duress_salt = config
            .duress_salt()
            .ok_or_else(|| VauchiError::InvalidState("duress salt not set".into()))?;

        self.storage
            .save_duress_password(duress_hash, duress_salt)?;

        Ok(())
    }

    /// Returns whether an app password has been configured.
    pub fn is_password_enabled(&self) -> VauchiResult<bool> {
        Ok(self.storage.load_password_config()?.is_some())
    }

    /// Returns whether duress mode is enabled.
    pub fn is_duress_enabled(&self) -> VauchiResult<bool> {
        match self.storage.load_password_config()? {
            Some(config) => Ok(config.duress_enabled()),
            None => Ok(false),
        }
    }

    /// Disables duress mode and clears duress hash/salt.
    pub fn disable_duress(&mut self) -> VauchiResult<()> {
        self.storage.disable_duress()?;
        Ok(())
    }

    // === Duress Settings ===

    /// Saves duress alert settings (trusted contacts, message, location).
    pub fn save_duress_settings(&self, settings: &DuressSettings) -> VauchiResult<()> {
        self.storage.save_duress_settings(settings)?;
        Ok(())
    }

    /// Loads duress alert settings.
    ///
    /// Returns `None` if no settings have been configured.
    pub fn load_duress_settings(&self) -> VauchiResult<Option<DuressSettings>> {
        Ok(self.storage.load_duress_settings()?)
    }

    /// Deletes duress alert settings.
    pub fn delete_duress_settings(&self) -> VauchiResult<()> {
        self.storage.delete_duress_settings()?;
        Ok(())
    }

    /// Returns a reference to the pending duress alerts queue.
    ///
    /// Alerts are queued when `authenticate()` detects a duress PIN.
    /// The sync system should drain this queue and send alerts as
    /// card updates to trusted contacts.
    pub fn pending_duress_alerts(&self) -> &[DuressAlert] {
        &self.duress_alerts
    }

    /// Queues a duress alert for sending to trusted contacts.
    ///
    /// Called internally by `authenticate()` when the duress PIN is entered.
    /// If no duress settings are configured, this is a no-op.
    ///
    /// The alert is stored in an in-memory queue. When the sync system
    /// connects, it drains this queue and sends alerts as card updates
    /// (indistinguishable from normal sync traffic).
    fn queue_duress_alert(&mut self) -> VauchiResult<()> {
        let settings = self.storage.load_duress_settings()?;
        if let Some(_settings) = settings {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let device_id = self.device_id_string();

            let alert = DuressAlert {
                timestamp: now,
                device_id,
                alert_type: DuressAlertType::Unlock,
            };

            self.duress_alerts.push(alert);
        }
        Ok(())
    }

    /// Returns a string identifier for this device.
    ///
    /// Uses the identity's public ID if available, otherwise falls
    /// back to a placeholder. Used in duress alerts to identify the
    /// originating device.
    fn device_id_string(&self) -> String {
        self.identity
            .as_ref()
            .map(|id| hex::encode(id.signing_public_key()))
            .unwrap_or_else(|| "unknown-device".to_string())
    }

    // === Emergency Broadcast ===

    /// Configures the emergency broadcast system.
    ///
    /// Sets which contacts receive emergency alerts, the alert message,
    /// and whether to include device location.
    ///
    /// # Constraints
    /// - Maximum 10 trusted contacts
    /// - Contact IDs list must not be empty
    pub fn configure_emergency_broadcast(
        &mut self,
        contact_ids: Vec<String>,
        message: String,
        include_location: bool,
    ) -> VauchiResult<()> {
        if contact_ids.len() > MAX_TRUSTED_CONTACTS {
            return Err(VauchiError::InvalidState(format!(
                "maximum {} trusted contacts allowed, got {}",
                MAX_TRUSTED_CONTACTS,
                contact_ids.len()
            )));
        }

        let config = EmergencyBroadcastConfig {
            trusted_contact_ids: contact_ids,
            message,
            include_location,
        };

        self.storage.save_emergency_config(&config)?;
        Ok(())
    }

    /// Loads the emergency broadcast configuration.
    ///
    /// Returns `None` if no configuration has been set.
    pub fn load_emergency_config(&self) -> VauchiResult<Option<EmergencyBroadcastConfig>> {
        Ok(self.storage.load_emergency_config()?)
    }

    /// Sends an emergency broadcast to all trusted contacts.
    ///
    /// For each trusted contact that has an established ratchet:
    /// 1. Creates an `EmergencyAlert` payload
    /// 2. Serializes and encrypts it as a card update (indistinguishable)
    /// 3. Queues for delivery via relay
    ///
    /// Returns a `BroadcastResult` with sent/total counts.
    pub fn send_emergency_broadcast(&mut self) -> VauchiResult<BroadcastResult> {
        use crate::network::EmergencyAlert;
        use crate::storage::{PendingUpdate, UpdateStatus};

        let config = self.storage.load_emergency_config()?.ok_or_else(|| {
            VauchiError::InvalidState("emergency broadcast not configured".into())
        })?;

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let sender_id = identity.public_id();
        let total = config.trusted_contact_ids.len();
        let mut sent = 0;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for contact_id in &config.trusted_contact_ids {
            // Skip contacts that don't exist locally
            let contact = match self.storage.load_contact(contact_id)? {
                Some(c) => c,
                None => continue,
            };

            // Skip blocked contacts
            if contact.is_blocked() {
                continue;
            }

            // Skip contacts without ratchet (can't encrypt)
            let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact_id)? {
                Some(r) => r,
                None => continue,
            };

            // Create the emergency alert payload
            let alert = EmergencyAlert {
                sender_id: sender_id.clone(),
                message: config.message.clone(),
                timestamp: now,
                location: None, // Location is provided by mobile layer at send time
            };

            // Serialize the alert as JSON (same format as card delta)
            let alert_bytes = serde_json::to_vec(&alert)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Encrypt with ratchet (indistinguishable from card update)
            let ratchet_msg = ratchet
                .encrypt(&alert_bytes)
                .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Save updated ratchet state
            self.storage
                .save_ratchet_state(contact_id, &ratchet, is_initiator)?;

            // Queue for delivery (update_type = "emergency_alert" internally,
            // but on the wire it's just an EncryptedUpdate like any other)
            let update = PendingUpdate {
                id: format!("{}-emergency-{}", contact_id, now),
                contact_id: contact_id.to_string(),
                update_type: "card_delta".to_string(), // Indistinguishable
                payload: encrypted,
                created_at: now,
                retry_count: 0,
                status: UpdateStatus::Pending,
            };
            self.storage.queue_update(&update)?;
            sent += 1;
        }

        // Dispatch event
        self.events.dispatch(VauchiEvent::EmergencyBroadcastSent {
            sent_count: sent,
            total,
        });

        Ok(BroadcastResult { sent, total })
    }

    /// Deletes the emergency broadcast configuration.
    pub fn delete_emergency_config(&mut self) -> VauchiResult<()> {
        self.storage.delete_emergency_config()?;
        Ok(())
    }

    // === Decoy Contacts ===

    /// Adds a decoy contact for duress mode.
    pub fn add_decoy_contact(
        &self,
        id: &str,
        display_name: &str,
        card: &ContactCard,
    ) -> VauchiResult<()> {
        self.storage.save_decoy_contact(id, display_name, card)?;
        Ok(())
    }

    /// Removes a decoy contact.
    pub fn remove_decoy_contact(&self, id: &str) -> VauchiResult<()> {
        self.storage.delete_decoy_contact(id)?;
        Ok(())
    }

    /// Lists all decoy contacts as (id, display_name, card) tuples.
    pub fn list_decoy_contacts(&self) -> VauchiResult<Vec<(String, String, ContactCard)>> {
        Ok(self.storage.load_decoy_contacts()?)
    }

    /// Clears all decoy contacts.
    pub fn clear_decoy_contacts(&self) -> VauchiResult<()> {
        self.storage.clear_all_decoy_contacts()?;
        Ok(())
    }

    // === Configuration ===

    /// Returns the current configuration.
    pub fn config(&self) -> &VauchiConfig {
        &self.config
    }

    /// Returns a reference to the storage.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Returns a reference to the event dispatcher.
    pub fn events(&self) -> &Arc<EventDispatcher> {
        &self.events
    }

    // === Visibility Labels ===

    /// Lists all visibility labels.
    pub fn list_labels(&self) -> VauchiResult<Vec<crate::contact::VisibilityLabel>> {
        Ok(self.storage.load_all_labels()?)
    }

    /// Creates a new visibility label.
    pub fn create_label(&self, name: &str) -> VauchiResult<crate::contact::VisibilityLabel> {
        Ok(self.storage.create_label(name)?)
    }

    /// Renames a visibility label.
    pub fn rename_label(&self, label_id: &str, new_name: &str) -> VauchiResult<()> {
        Ok(self.storage.rename_label(label_id, new_name)?)
    }

    /// Deletes a visibility label.
    ///
    /// Contacts in the label remain in the contact list; they just lose
    /// their label membership.
    pub fn delete_label(&self, label_id: &str) -> VauchiResult<()> {
        Ok(self.storage.delete_label(label_id)?)
    }

    /// Gets a visibility label by ID.
    pub fn get_label(&self, label_id: &str) -> VauchiResult<crate::contact::VisibilityLabel> {
        Ok(self.storage.load_label(label_id)?)
    }

    /// Adds a contact to a visibility label.
    pub fn add_contact_to_label(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self.storage.add_contact_to_label(label_id, contact_id)?)
    }

    /// Removes a contact from a visibility label.
    pub fn remove_contact_from_label(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self
            .storage
            .remove_contact_from_label(label_id, contact_id)?)
    }

    /// Gets all labels that contain a specific contact.
    pub fn get_labels_for_contact(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Vec<crate::contact::VisibilityLabel>> {
        Ok(self.storage.get_labels_for_contact(contact_id)?)
    }

    /// Sets field visibility for a label.
    ///
    /// When `is_visible` is true, contacts in this label will see the field.
    /// When false, the field is hidden from contacts in this label.
    pub fn set_label_field_visibility(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        Ok(self
            .storage
            .set_label_field_visibility(label_id, field_id, is_visible)?)
    }

    /// Sets a per-contact visibility override for a field.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_visibility_override(
        &self,
        contact_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        Ok(self
            .storage
            .save_contact_override(contact_id, field_id, is_visible)?)
    }

    /// Removes a per-contact visibility override.
    pub fn remove_contact_visibility_override(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> VauchiResult<()> {
        Ok(self.storage.delete_contact_override(contact_id, field_id)?)
    }

    /// Gets all per-contact visibility overrides for a contact.
    pub fn get_contact_visibility_overrides(
        &self,
        contact_id: &str,
    ) -> VauchiResult<std::collections::HashMap<String, bool>> {
        Ok(self.storage.load_contact_overrides(contact_id)?)
    }

    /// Determines the effective visibility of a field for a contact.
    ///
    /// Returns visibility determined by (in priority order):
    /// 1. Per-contact override (if set)
    /// 2. Label membership (visible if contact is in any label that shows this field)
    /// 3. Contact's VisibilityRules (the default field visibility)
    pub fn get_effective_field_visibility(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> VauchiResult<bool> {
        // Load the contact's visibility rules as fallback
        let contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", contact_id)))?;

        // Check per-contact override first
        let overrides = self.storage.load_contact_overrides(contact_id)?;
        if let Some(&is_visible) = overrides.get(field_id) {
            return Ok(is_visible);
        }

        // Check if any label containing this contact shows this field
        let labels = self.storage.get_labels_for_contact(contact_id)?;
        for label in labels {
            if label.is_field_visible(field_id) {
                return Ok(true);
            }
        }

        // Fall back to contact's default visibility rules
        // Note: The visibility rules determine what this contact can see of *our* card
        // We use their contact_id to check if they're in the allowed list
        Ok(contact.visibility_rules().can_see(field_id, contact_id))
    }

    // === Field Validation Operations ===

    /// Validates a contact's field.
    ///
    /// Creates a cryptographically signed validation record that attests
    /// the current user believes the field value belongs to the contact.
    ///
    /// # Arguments
    /// * `contact_id` - The contact whose field is being validated
    /// * `field_id` - The field name (e.g., "twitter", "email")
    /// * `field_value` - The current value of the field
    ///
    /// # Returns
    /// The created validation record
    pub fn validate_field(
        &self,
        contact_id: &str,
        field_id: &str,
        field_value: &str,
    ) -> VauchiResult<crate::social::ProfileValidation> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Check we're not validating our own field
        let my_id = hex::encode(identity.signing_public_key());
        if contact_id == my_id {
            return Err(VauchiError::InvalidState(
                "Cannot validate your own field".into(),
            ));
        }

        // Check we haven't already validated this field
        let validator_id = hex::encode(identity.signing_public_key());
        if self
            .storage
            .has_validated(contact_id, field_id, &validator_id)?
        {
            return Err(VauchiError::InvalidState(
                "You have already validated this field".into(),
            ));
        }

        // Create signed validation
        let validation = crate::social::ProfileValidation::create_signed(
            identity,
            field_id,
            field_value,
            contact_id,
        );

        // Store it
        self.storage.save_validation(&validation)?;

        Ok(validation)
    }

    /// Gets the validation status for a contact's field.
    ///
    /// Returns aggregated validation information including count, trust level,
    /// and whether the current user has validated this field.
    pub fn get_field_validation_status(
        &self,
        contact_id: &str,
        field_id: &str,
        field_value: &str,
    ) -> VauchiResult<crate::social::ValidationStatus> {
        let validations = self
            .storage
            .load_validations_for_field(contact_id, field_id)?;

        // Get current user's ID if available
        let my_id = self
            .identity
            .as_ref()
            .map(|id| hex::encode(id.signing_public_key()));

        // Get blocked contacts (empty for now, could be extended)
        let blocked = std::collections::HashSet::new();

        let status = crate::social::ValidationStatus::from_validations(
            &validations,
            field_value,
            my_id.as_deref(),
            &blocked,
        );

        Ok(status)
    }

    /// Revokes the current user's validation of a field.
    ///
    /// Returns true if a validation was revoked, false if none existed.
    pub fn revoke_field_validation(&self, contact_id: &str, field_id: &str) -> VauchiResult<bool> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let validator_id = hex::encode(identity.signing_public_key());
        let deleted = self
            .storage
            .delete_validation(contact_id, field_id, &validator_id)?;

        Ok(deleted)
    }

    /// Lists all validations made by the current user.
    ///
    /// Returns a list of all fields the user has validated, sorted by
    /// validation timestamp (most recent first).
    pub fn list_my_validations(&self) -> VauchiResult<Vec<crate::social::ProfileValidation>> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validations = self.storage.load_validations_by_validator(&validator_id)?;

        Ok(validations)
    }

    /// Checks if the current user has validated a specific field.
    pub fn has_validated_field(&self, contact_id: &str, field_id: &str) -> VauchiResult<bool> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validated = self
            .storage
            .has_validated(contact_id, field_id, &validator_id)?;

        Ok(validated)
    }

    // === Aha Moments Operations ===

    /// Tries to trigger an aha moment of the given type.
    ///
    /// Returns the moment if it should be shown (not yet seen).
    /// Automatically persists the "seen" state.
    pub fn try_trigger_aha_moment(
        &self,
        moment_type: crate::aha_moments::AhaMomentType,
    ) -> VauchiResult<Option<crate::aha_moments::AhaMoment>> {
        let mut tracker = self.storage.load_or_create_aha_tracker()?;
        let moment = tracker.try_trigger(moment_type);
        if moment.is_some() {
            self.storage.save_aha_tracker(&tracker)?;
        }
        Ok(moment)
    }

    /// Tries to trigger an aha moment with context.
    ///
    /// Context is used for personalized messages (e.g., contact name).
    pub fn try_trigger_aha_moment_with_context(
        &self,
        moment_type: crate::aha_moments::AhaMomentType,
        context: String,
    ) -> VauchiResult<Option<crate::aha_moments::AhaMoment>> {
        let mut tracker = self.storage.load_or_create_aha_tracker()?;
        let moment = tracker.try_trigger_with_context(moment_type, context);
        if moment.is_some() {
            self.storage.save_aha_tracker(&tracker)?;
        }
        Ok(moment)
    }

    /// Checks if an aha moment has been seen.
    pub fn has_seen_aha_moment(
        &self,
        moment_type: crate::aha_moments::AhaMomentType,
    ) -> VauchiResult<bool> {
        let tracker = self.storage.load_or_create_aha_tracker()?;
        Ok(tracker.has_seen(moment_type))
    }

    /// Gets the number of aha moments seen.
    pub fn aha_moments_seen_count(&self) -> VauchiResult<usize> {
        let tracker = self.storage.load_or_create_aha_tracker()?;
        Ok(tracker.seen_count())
    }

    /// Resets all aha moments (for testing or demo replay).
    pub fn reset_aha_moments(&self) -> VauchiResult<()> {
        let mut tracker = self.storage.load_or_create_aha_tracker()?;
        tracker.reset();
        self.storage.save_aha_tracker(&tracker)?;
        Ok(())
    }

    // === Demo Contact Operations ===

    /// Gets the current demo contact state.
    pub fn demo_contact_state(&self) -> VauchiResult<crate::demo_contact::DemoContactState> {
        Ok(self.storage.load_or_create_demo_contact_state()?)
    }

    /// Checks if the demo contact is active.
    pub fn is_demo_contact_active(&self) -> VauchiResult<bool> {
        Ok(self.storage.is_demo_contact_active()?)
    }

    /// Gets the current demo contact card (if active).
    pub fn demo_contact_card(&self) -> VauchiResult<Option<crate::demo_contact::DemoContactCard>> {
        let state = self.storage.load_or_create_demo_contact_state()?;
        if !state.is_active {
            return Ok(None);
        }
        match state.current_tip() {
            Some(tip) => Ok(Some(crate::demo_contact::generate_demo_contact_card(&tip))),
            None => Ok(None),
        }
    }

    /// Advances the demo contact to the next tip.
    ///
    /// Returns the new tip if successful.
    pub fn advance_demo_contact(&self) -> VauchiResult<Option<crate::demo_contact::DemoTip>> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        if !state.is_active {
            return Ok(None);
        }
        let tip = state.advance_to_next_tip();
        self.storage.save_demo_contact_state(&state)?;
        Ok(tip)
    }

    /// Dismisses the demo contact (user-initiated).
    pub fn dismiss_demo_contact(&self) -> VauchiResult<()> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        state.dismiss();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    /// Auto-removes the demo contact (after first real exchange).
    pub fn auto_remove_demo_contact(&self) -> VauchiResult<()> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        state.auto_remove();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    /// Restores the demo contact from settings.
    pub fn restore_demo_contact(&self) -> VauchiResult<()> {
        let mut state = self.storage.load_or_create_demo_contact_state()?;
        state.restore();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    /// Initializes the demo contact for a new user.
    ///
    /// Should be called after identity creation if user has no contacts.
    pub fn initialize_demo_contact(&self) -> VauchiResult<()> {
        // Only initialize if user has no real contacts
        if self.contact_count()? > 0 {
            return Ok(());
        }

        let state = crate::demo_contact::DemoContactState::new_active();
        self.storage.save_demo_contact_state(&state)?;
        Ok(())
    }

    // === Tor Configuration ===

    /// Returns the current Tor configuration.
    pub fn tor_config(&self) -> &crate::tor_config::TorConfig {
        &self.config.tor
    }

    /// Returns the current Tor status.
    ///
    /// Without the `tor` feature enabled, this always returns `Disabled`.
    pub fn tor_status(&self) -> crate::tor_config::TorStatus {
        crate::tor_config::TorStatus::Disabled
    }

    /// Enables Tor with the current configuration.
    ///
    /// Persists the enabled state to storage.
    /// Note: Actual Tor bootstrapping requires the `tor` feature.
    pub fn enable_tor(&mut self) -> VauchiResult<()> {
        self.config.tor.enabled = true;
        self.storage.save_tor_config(&self.config.tor)?;
        self.events.dispatch(VauchiEvent::TorStatusChanged {
            status: crate::tor_config::TorStatus::Disabled,
        });
        Ok(())
    }

    /// Disables Tor.
    ///
    /// Persists the disabled state to storage.
    pub fn disable_tor(&mut self) -> VauchiResult<()> {
        self.config.tor.enabled = false;
        self.storage.save_tor_config(&self.config.tor)?;
        self.events.dispatch(VauchiEvent::TorStatusChanged {
            status: crate::tor_config::TorStatus::Disabled,
        });
        Ok(())
    }

    /// Configures Tor bridge addresses.
    ///
    /// Bridges are used when direct Tor connections are blocked.
    pub fn configure_tor_bridges(&mut self, bridges: Vec<String>) -> VauchiResult<()> {
        self.config.tor.bridges = bridges;
        self.storage.save_tor_config(&self.config.tor)?;
        Ok(())
    }

    /// Requests a new Tor circuit rotation.
    ///
    /// Without the `tor` feature, this is a no-op that returns Ok.
    pub fn request_new_tor_circuit(&self) -> VauchiResult<()> {
        // Actual circuit rotation requires the `tor` feature with arti
        Ok(())
    }

    /// Loads the persisted Tor configuration from storage and applies it.
    pub fn load_tor_config(&mut self) -> VauchiResult<()> {
        if let Some(config) = self.storage.load_tor_config()? {
            self.config.tor = config;
        }
        Ok(())
    }

    // === Multi-Relay Configuration ===

    /// Returns the current multi-relay configuration, if any.
    pub fn relay_list(&self) -> Option<&crate::network::MultiRelayConfig> {
        self.config.relay_list.as_ref()
    }

    /// Sets the multi-relay configuration.
    pub fn set_relay_list(&mut self, config: crate::network::MultiRelayConfig) -> VauchiResult<()> {
        self.config.relay_list = Some(config);
        Ok(())
    }

    /// Clears the multi-relay configuration (reverts to single relay).
    pub fn clear_relay_list(&mut self) {
        self.config.relay_list = None;
    }

    // === Hide/Unhide Contacts ===

    /// Hides a contact from the main contact list.
    ///
    /// Hidden contacts provide plausible deniability - they only appear
    /// via secret access (gesture, PIN, or special settings navigation).
    /// Updates from hidden contacts are still received but notifications
    /// are suppressed.
    pub fn hide_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.hide();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactHidden {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Unhides a contact, making it visible in the main contact list again.
    pub fn unhide_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.unhide();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactUnhidden {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Lists all hidden contacts.
    pub fn list_hidden_contacts(&self) -> VauchiResult<Vec<Contact>> {
        let contacts = self.storage.list_contacts()?;
        Ok(contacts.into_iter().filter(|c| c.is_hidden()).collect())
    }

    // === Block/Unblock Contacts ===

    /// Blocks a contact.
    ///
    /// Blocked contacts will not receive card updates and their incoming
    /// updates will be rejected.
    pub fn block_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.block();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactBlocked {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Unblocks a contact.
    pub fn unblock_contact(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))?;
        contact.unblock();
        self.storage.save_contact(&contact)?;
        self.events.dispatch(VauchiEvent::ContactUnblocked {
            contact_id: id.to_string(),
        });
        Ok(())
    }

    /// Lists all blocked contacts.
    pub fn list_blocked_contacts(&self) -> VauchiResult<Vec<Contact>> {
        let contacts = self.storage.list_contacts()?;
        Ok(contacts.into_iter().filter(|c| c.is_blocked()).collect())
    }

    // === Consent Management ===

    /// Grants consent for a specific type.
    pub fn grant_consent(&self, consent_type: ConsentType) -> VauchiResult<()> {
        let manager = ConsentManager::new(&self.storage);
        manager.grant(consent_type).map_err(VauchiError::from)
    }

    /// Revokes consent for a specific type.
    pub fn revoke_consent(&self, consent_type: ConsentType) -> VauchiResult<()> {
        let manager = ConsentManager::new(&self.storage);
        manager.revoke(consent_type).map_err(VauchiError::from)
    }

    /// Checks whether consent is currently granted for a type.
    pub fn check_consent(&self, consent_type: &ConsentType) -> VauchiResult<bool> {
        let manager = ConsentManager::new(&self.storage);
        manager.check(consent_type).map_err(VauchiError::from)
    }

    /// Exports all consent records.
    pub fn export_consent_log(&self) -> VauchiResult<Vec<ConsentRecord>> {
        let manager = ConsentManager::new(&self.storage);
        manager.export_consent_log().map_err(VauchiError::from)
    }

    // === Visibility Re-Propagation ===

    /// Sets a field as visible to everyone for a contact, and re-propagates the card.
    pub fn set_field_public_and_repropagate(
        &self,
        contact_id: &str,
        field: &str,
    ) -> VauchiResult<()> {
        let cm = ContactManager::new(&self.storage, self.events.clone());
        cm.set_field_public(contact_id, field)?;
        self.events.dispatch(VauchiEvent::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field: field.to_string(),
        });
        self.repropagate_to_contact(contact_id)
    }

    /// Sets a field as private for a contact, and re-propagates the card.
    pub fn set_field_private_and_repropagate(
        &self,
        contact_id: &str,
        field: &str,
    ) -> VauchiResult<()> {
        let cm = ContactManager::new(&self.storage, self.events.clone());
        cm.set_field_private(contact_id, field)?;
        self.events.dispatch(VauchiEvent::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field: field.to_string(),
        });
        self.repropagate_to_contact(contact_id)
    }

    /// Sets a field as restricted to specific contacts, and re-propagates the card.
    pub fn set_field_restricted_and_repropagate(
        &self,
        contact_id: &str,
        field: &str,
        allowed: Vec<String>,
    ) -> VauchiResult<()> {
        let cm = ContactManager::new(&self.storage, self.events.clone());
        cm.set_field_restricted(contact_id, field, allowed)?;
        self.events.dispatch(VauchiEvent::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field: field.to_string(),
        });
        self.repropagate_to_contact(contact_id)
    }

    /// Adds a contact to a label and re-propagates the card to that contact.
    ///
    /// The contact receives an updated card reflecting their new label membership.
    pub fn add_contact_to_label_and_repropagate(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> VauchiResult<()> {
        self.storage.add_contact_to_label(label_id, contact_id)?;
        self.repropagate_to_contact(contact_id)
    }

    /// Removes a contact from a label and re-propagates the card to that contact.
    ///
    /// The contact receives an updated card with fields they can no longer see removed.
    pub fn remove_contact_from_label_and_repropagate(
        &self,
        label_id: &str,
        contact_id: &str,
    ) -> VauchiResult<()> {
        self.storage
            .remove_contact_from_label(label_id, contact_id)?;
        self.repropagate_to_contact(contact_id)
    }

    /// Sets field visibility for a label and re-propagates to all contacts in that label.
    ///
    /// All contacts in the label receive updated cards reflecting the visibility change.
    pub fn set_label_field_visibility_and_repropagate(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        self.storage
            .set_label_field_visibility(label_id, field_id, is_visible)?;

        // Re-propagate to all contacts in this label
        let label = self.storage.load_label(label_id)?;
        for contact_id in label.contacts() {
            self.repropagate_to_contact(contact_id)?;
        }
        Ok(())
    }

    /// Sets a per-contact visibility override and re-propagates to that contact.
    ///
    /// The contact receives an updated card reflecting the override.
    pub fn set_contact_visibility_override_and_repropagate(
        &self,
        contact_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        self.storage
            .save_contact_override(contact_id, field_id, is_visible)?;
        self.repropagate_to_contact(contact_id)
    }

    /// Re-propagates the current card state to a single contact.
    ///
    /// Sends a "full card" delta so the contact receives the card as filtered
    /// by their current visibility rules. Skips if the contact has no ratchet.
    fn repropagate_to_contact(&self, contact_id: &str) -> VauchiResult<()> {
        use crate::crypto::cek::ContentEncryptionKey;
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let own_card = self
            .storage
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let mut contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;

        // Skip contacts without ratchet (not yet synced)
        let (mut ratchet, is_initiator) = match self.storage.load_ratchet_state(contact_id)? {
            Some(r) => r,
            None => return Ok(()),
        };

        // Compute a "full card" delta from an empty card
        let empty_card = ContactCard::new(own_card.display_name());
        let delta = CardDelta::compute(&empty_card, &own_card);
        if delta.is_empty() {
            return Ok(());
        }

        // Filter delta using effective visibility (labels + overrides + defaults)
        let contact_id_owned = contact_id.to_string();
        let mut delta = delta.filter_with(|field_id| {
            self.get_effective_field_visibility(&contact_id_owned, field_id)
                .unwrap_or(false)
        });
        if delta.is_empty() {
            return Ok(());
        }

        // Sign delta with our identity, bound to recipient
        delta.sign(identity, contact.public_key());

        // Serialize delta
        let delta_bytes =
            serde_json::to_vec(&delta).map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Wrap with CEK if contact has one, otherwise legacy
        let payload_bytes = if contact.cek().is_some() {
            let new_cek = ContentEncryptionKey::generate();
            let cek_ciphertext = new_cek
                .encrypt(&delta_bytes)
                .map_err(|e| VauchiError::Crypto(format!("CEK encrypt: {:?}", e)))?;

            let wrapped = CekWrappedPayload {
                cek: new_cek.to_bytes(),
                cek_ciphertext,
                signature: delta.signature,
                nonce: delta.nonce,
            };

            contact.set_cek(new_cek);
            self.storage.save_contact(&contact)?;
            VersionedPayload::encode_cek(&wrapped)
        } else {
            delta_bytes
        };

        // Encrypt with ratchet
        let ratchet_msg = ratchet
            .encrypt(&payload_bytes)
            .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;
        let encrypted = serde_json::to_vec(&ratchet_msg)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Save updated ratchet state
        self.storage
            .save_ratchet_state(contact_id, &ratchet, is_initiator)?;

        // Queue for delivery
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let update = PendingUpdate {
            id: format!("{}-vis-{}", contact_id, now),
            contact_id: contact_id.to_string(),
            update_type: "card_delta".to_string(),
            payload: encrypted,
            created_at: now,
            retry_count: 0,
            status: UpdateStatus::Pending,
        };
        self.storage.queue_update(&update)?;

        Ok(())
    }
}

/// Converts a decoy contact ID string into a fake 32-byte "public key".
///
/// This is a deterministic mapping used only for display purposes — decoy
/// contacts don't have real cryptographic keys. The resulting bytes are
/// derived by hashing the ID with ring's SHA-256, ensuring consistent
/// IDs across sessions.
fn decoy_id_to_fake_pk(id: &str) -> [u8; 32] {
    use ring::digest;
    let hash = digest::digest(&digest::SHA256, id.as_bytes());
    let mut pk = [0u8; 32];
    pk.copy_from_slice(hash.as_ref());
    pk
}

/// Builder for creating Vauchi instances.
pub struct VauchiBuilder<T: Transport> {
    config: VauchiConfig,
    identity: Option<Identity>,
    transport_factory: Option<Box<dyn FnOnce() -> T>>,
}

impl<T: Transport> VauchiBuilder<T> {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        VauchiBuilder {
            config: VauchiConfig::default(),
            identity: None,
            transport_factory: None,
        }
    }

    /// Sets the configuration.
    pub fn config(mut self, config: VauchiConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the storage path.
    pub fn storage_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.config.storage_path = path.into();
        self
    }

    /// Sets the relay URL.
    pub fn relay_url(mut self, url: impl Into<String>) -> Self {
        self.config.relay.server_url = url.into();
        self
    }

    /// Sets an existing identity.
    pub fn identity(mut self, identity: Identity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// Sets the transport factory.
    pub fn transport<F>(mut self, factory: F) -> Self
    where
        F: FnOnce() -> T + 'static,
    {
        self.transport_factory = Some(Box::new(factory));
        self
    }

    /// Builds the Vauchi instance.
    pub fn build(self) -> VauchiResult<Vauchi<T>>
    where
        T: Default,
    {
        let factory = self
            .transport_factory
            .unwrap_or_else(|| Box::new(T::default));
        let mut wb = Vauchi::with_transport_factory(self.config, factory)?;

        if let Some(identity) = self.identity {
            wb.set_identity(identity)?;
        }

        Ok(wb)
    }
}

impl<T: Transport + Default> Default for VauchiBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}
