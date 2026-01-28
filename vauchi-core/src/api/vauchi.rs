// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Orchestrator
//!
//! Main entry point for the Vauchi API.

use std::sync::Arc;

use crate::contact::Contact;
use crate::contact_card::{ContactCard, ContactField};
use crate::crypto::ratchet::DoubleRatchetState;
use crate::crypto::SymmetricKey;
use crate::identity::Identity;
use crate::network::{MockTransport, Transport};
use crate::storage::Storage;

use super::config::VauchiConfig;
use super::contact_manager::ContactManager;
use super::error::{VauchiError, VauchiResult};
use super::events::{EventDispatcher, EventHandler, VauchiEvent};

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
pub struct Vauchi<T: Transport = MockTransport> {
    config: VauchiConfig,
    identity: Option<Identity>,
    storage: Storage,
    events: Arc<EventDispatcher>,
    _phantom: std::marker::PhantomData<T>,
}

impl Vauchi<MockTransport> {
    /// Creates a new Vauchi instance with mock transport (for testing).
    pub fn new(config: VauchiConfig) -> VauchiResult<Self> {
        Self::with_transport_factory(config, MockTransport::new)
    }
}

impl<T: Transport> Vauchi<T> {
    /// Creates a new Vauchi instance with a custom transport factory.
    pub fn with_transport_factory<F>(
        config: VauchiConfig,
        _transport_factory: F,
    ) -> VauchiResult<Self>
    where
        F: FnOnce() -> T,
    {
        // Use provided storage key or generate a new one
        let storage_key = config
            .storage_key
            .clone()
            .unwrap_or_else(SymmetricKey::generate);

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
            _phantom: std::marker::PhantomData,
        })
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
            _phantom: std::marker::PhantomData,
        })
    }

    // === Identity Operations ===

    /// Creates a new identity with the given display name.
    pub fn create_identity(&mut self, display_name: &str) -> VauchiResult<()> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }

        let identity = Identity::create(display_name);

        // Create initial contact card from identity
        let card = ContactCard::new(display_name);
        self.storage.save_own_card(&card)?;

        self.identity = Some(identity);
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

    /// Lists all contacts.
    pub fn list_contacts(&self) -> VauchiResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.list_contacts()
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
    /// 3. Encrypts with contact's ratchet
    /// 4. Queues for delivery via relay
    ///
    /// Returns the number of contacts queued for update.
    pub fn propagate_card_update(
        &self,
        old_card: &ContactCard,
        new_card: &ContactCard,
    ) -> VauchiResult<usize> {
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::CardDelta;

        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let contacts = self.storage.list_contacts()?;
        let mut queued = 0;

        for contact in contacts {
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

            // Sign delta with our identity
            delta.sign(identity);

            // Serialize delta
            let delta_bytes = serde_json::to_vec(&delta)
                .map_err(|e| VauchiError::Serialization(e.to_string()))?;

            // Encrypt with ratchet
            let ratchet_msg = ratchet
                .encrypt(&delta_bytes)
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
    /// 1. Decrypts the update using the contact's ratchet
    /// 2. Verifies the signature using the contact's public key
    /// 3. Applies the delta to the contact's card
    ///
    /// Returns a list of changed field labels.
    pub fn process_card_update(
        &self,
        contact_id: &str,
        encrypted: &[u8],
    ) -> VauchiResult<Vec<String>> {
        use crate::crypto::ratchet::RatchetMessage;
        use crate::sync::delta::CardDelta;

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
        let delta_bytes = ratchet
            .decrypt(&ratchet_msg)
            .map_err(|e| VauchiError::Crypto(format!("{:?}", e)))?;

        // Save updated ratchet state
        self.storage
            .save_ratchet_state(contact_id, &ratchet, is_initiator)?;

        // Parse delta
        let delta: CardDelta = serde_json::from_slice(&delta_bytes)
            .map_err(|e| VauchiError::Serialization(e.to_string()))?;

        // Verify signature with contact's public key
        if !delta.verify(contact.public_key()) {
            return Err(VauchiError::SignatureInvalid);
        }

        // Get changed fields before applying
        let changed = delta.changed_fields();

        // Apply delta to contact's card
        let mut new_card = contact.card().clone();
        delta
            .apply(&mut new_card)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;

        // Update contact
        contact.update_card(new_card);
        self.storage.save_contact(&contact)?;

        Ok(changed)
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
