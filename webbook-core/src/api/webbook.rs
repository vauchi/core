//! WebBook Orchestrator
//!
//! Main entry point for the WebBook API.

use std::sync::Arc;

use crate::contact::Contact;
use crate::contact_card::{ContactCard, ContactField};
use crate::crypto::ratchet::DoubleRatchetState;
use crate::crypto::SymmetricKey;
use crate::identity::Identity;
use crate::network::{MockTransport, Transport};
use crate::storage::Storage;

use super::config::WebBookConfig;
use super::contact_manager::ContactManager;
use super::error::{WebBookError, WebBookResult};
use super::events::{EventDispatcher, EventHandler, WebBookEvent};

/// Main WebBook orchestrator.
///
/// This is the primary entry point for using WebBook. It coordinates:
/// - Identity management
/// - Contact management
/// - Synchronization
/// - Event dispatching
///
/// # Example
///
/// ```ignore
/// use webbook_core::api::{WebBook, WebBookConfig};
///
/// // Create WebBook with default config
/// let mut wb = WebBook::new(WebBookConfig::default())?;
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
pub struct WebBook<T: Transport = MockTransport> {
    config: WebBookConfig,
    identity: Option<Identity>,
    storage: Storage,
    events: Arc<EventDispatcher>,
    _phantom: std::marker::PhantomData<T>,
}

impl WebBook<MockTransport> {
    /// Creates a new WebBook instance with mock transport (for testing).
    pub fn new(config: WebBookConfig) -> WebBookResult<Self> {
        Self::with_transport_factory(config, MockTransport::new)
    }
}

impl<T: Transport> WebBook<T> {
    /// Creates a new WebBook instance with a custom transport factory.
    pub fn with_transport_factory<F>(
        config: WebBookConfig,
        _transport_factory: F,
    ) -> WebBookResult<Self>
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
                    .map_err(|e| WebBookError::Configuration(e.to_string()))?;
            }
            Storage::open(&config.storage_path, storage_key)?
        };

        let events = Arc::new(EventDispatcher::new());

        Ok(WebBook {
            config,
            identity: None,
            storage,
            events,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Creates a new WebBook instance with in-memory storage (for testing).
    pub fn in_memory() -> WebBookResult<Self>
    where
        T: Default,
    {
        let storage_key = SymmetricKey::generate();
        let storage = Storage::in_memory(storage_key)?;
        let events = Arc::new(EventDispatcher::new());

        Ok(WebBook {
            config: WebBookConfig::default(),
            identity: None,
            storage,
            events,
            _phantom: std::marker::PhantomData,
        })
    }

    // === Identity Operations ===

    /// Creates a new identity with the given display name.
    pub fn create_identity(&mut self, display_name: &str) -> WebBookResult<()> {
        if self.identity.is_some() {
            return Err(WebBookError::AlreadyInitialized);
        }

        let identity = Identity::create(display_name);

        // Create initial contact card from identity
        let card = ContactCard::new(display_name);
        self.storage.save_own_card(&card)?;

        self.identity = Some(identity);
        Ok(())
    }

    /// Sets an existing identity.
    pub fn set_identity(&mut self, identity: Identity) -> WebBookResult<()> {
        if self.identity.is_some() {
            return Err(WebBookError::AlreadyInitialized);
        }
        self.identity = Some(identity);
        Ok(())
    }

    /// Returns the current identity, if set.
    pub fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }

    /// Returns the public ID of the current identity.
    pub fn public_id(&self) -> WebBookResult<String> {
        self.identity
            .as_ref()
            .map(|id| id.public_id())
            .ok_or(WebBookError::IdentityNotInitialized)
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
    pub fn update_display_name(&mut self, new_name: &str) -> WebBookResult<()> {
        let name = new_name.trim();

        if name.is_empty() {
            return Err(WebBookError::InvalidState(
                "Display name cannot be empty".into(),
            ));
        }
        if name.len() > 100 {
            return Err(WebBookError::InvalidState(
                "Display name cannot exceed 100 characters".into(),
            ));
        }

        // Get mutable reference to identity
        let identity = self
            .identity
            .as_mut()
            .ok_or(WebBookError::IdentityNotInitialized)?;

        // Update identity display name
        identity.set_display_name(name);

        // Update contact card display name
        let mut card = self
            .storage
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(name));
        card.set_display_name(name)
            .map_err(|e| WebBookError::InvalidState(e.to_string()))?;
        self.storage.save_own_card(&card)?;

        Ok(())
    }

    // === Contact Card Operations ===

    /// Gets the user's own contact card.
    pub fn own_card(&self) -> WebBookResult<Option<ContactCard>> {
        Ok(self.storage.load_own_card()?)
    }

    /// Updates the user's own contact card.
    pub fn update_own_card(&self, card: &ContactCard) -> WebBookResult<Vec<String>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.update_own_card(card)
    }

    /// Adds a field to the user's own card.
    pub fn add_own_field(&self, field: ContactField) -> WebBookResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_field_to_own_card(field)
    }

    /// Removes a field from the user's own card.
    pub fn remove_own_field(&self, label: &str) -> WebBookResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.remove_field_from_own_card(label)
    }

    // === Contact Operations ===

    /// Gets a contact by ID.
    pub fn get_contact(&self, id: &str) -> WebBookResult<Option<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.get_contact(id)
    }

    /// Lists all contacts.
    pub fn list_contacts(&self) -> WebBookResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.list_contacts()
    }

    /// Searches contacts by display name.
    pub fn search_contacts(&self, query: &str) -> WebBookResult<Vec<Contact>> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.search_contacts(query)
    }

    /// Returns the number of contacts.
    pub fn contact_count(&self) -> WebBookResult<usize> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.contact_count()
    }

    /// Adds a new contact from an exchange.
    pub fn add_contact(&self, contact: Contact) -> WebBookResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_contact(contact)
    }

    /// Removes a contact by ID.
    pub fn remove_contact(&self, id: &str) -> WebBookResult<bool> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.remove_contact(id)
    }

    /// Updates an existing contact.
    pub fn update_contact(&self, contact: &Contact) -> WebBookResult<()> {
        self.storage.save_contact(contact)?;
        Ok(())
    }

    /// Verifies a contact's fingerprint.
    pub fn verify_contact_fingerprint(&self, id: &str) -> WebBookResult<()> {
        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.verify_fingerprint(id)
    }

    // === Double Ratchet Operations ===

    /// Gets the Double Ratchet state for a contact.
    pub fn get_ratchet_state(&self, contact_id: &str) -> WebBookResult<Option<DoubleRatchetState>> {
        Ok(self.storage.load_ratchet_state(contact_id)?.map(|(r, _)| r))
    }

    /// Saves a Double Ratchet state for a contact.
    ///
    /// If a ratchet state already exists, preserves the is_initiator flag.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
    ) -> WebBookResult<()> {
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
    ) -> WebBookResult<()> {
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
    ) -> WebBookResult<()> {
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
    ) -> WebBookResult<usize> {
        use crate::storage::{PendingUpdate, UpdateStatus};
        use crate::sync::delta::CardDelta;

        let identity = self
            .identity
            .as_ref()
            .ok_or(WebBookError::IdentityNotInitialized)?;

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
                .map_err(|e| WebBookError::Serialization(e.to_string()))?;

            // Encrypt with ratchet
            let ratchet_msg = ratchet
                .encrypt(&delta_bytes)
                .map_err(|e| WebBookError::Crypto(format!("{:?}", e)))?;
            let encrypted = serde_json::to_vec(&ratchet_msg)
                .map_err(|e| WebBookError::Serialization(e.to_string()))?;

            // Save updated ratchet state
            self.storage
                .save_ratchet_state(contact.id(), &ratchet, is_initiator)?;

            // Queue for delivery
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

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
    ) -> WebBookResult<Vec<String>> {
        use crate::crypto::ratchet::RatchetMessage;
        use crate::sync::delta::CardDelta;

        // Load contact
        let mut contact = self
            .storage
            .load_contact(contact_id)?
            .ok_or_else(|| WebBookError::NotFound(format!("contact: {}", contact_id)))?;

        // Load and decrypt with ratchet
        let (mut ratchet, is_initiator) = self
            .storage
            .load_ratchet_state(contact_id)?
            .ok_or_else(|| WebBookError::NotFound("ratchet state".into()))?;

        let ratchet_msg: RatchetMessage = serde_json::from_slice(encrypted)
            .map_err(|e| WebBookError::Serialization(e.to_string()))?;
        let delta_bytes = ratchet
            .decrypt(&ratchet_msg)
            .map_err(|e| WebBookError::Crypto(format!("{:?}", e)))?;

        // Save updated ratchet state
        self.storage
            .save_ratchet_state(contact_id, &ratchet, is_initiator)?;

        // Parse delta
        let delta: CardDelta = serde_json::from_slice(&delta_bytes)
            .map_err(|e| WebBookError::Serialization(e.to_string()))?;

        // Verify signature with contact's public key
        if !delta.verify(contact.public_key()) {
            return Err(WebBookError::SignatureInvalid);
        }

        // Get changed fields before applying
        let changed = delta.changed_fields();

        // Apply delta to contact's card
        let mut new_card = contact.card().clone();
        delta
            .apply(&mut new_card)
            .map_err(|e| WebBookError::InvalidState(e.to_string()))?;

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
    pub fn dispatch_event(&self, event: WebBookEvent) {
        self.events.dispatch(event);
    }

    // === Configuration ===

    /// Returns the current configuration.
    pub fn config(&self) -> &WebBookConfig {
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
}

/// Builder for creating WebBook instances.
pub struct WebBookBuilder<T: Transport> {
    config: WebBookConfig,
    identity: Option<Identity>,
    transport_factory: Option<Box<dyn FnOnce() -> T>>,
}

impl<T: Transport> WebBookBuilder<T> {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        WebBookBuilder {
            config: WebBookConfig::default(),
            identity: None,
            transport_factory: None,
        }
    }

    /// Sets the configuration.
    pub fn config(mut self, config: WebBookConfig) -> Self {
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

    /// Builds the WebBook instance.
    pub fn build(self) -> WebBookResult<WebBook<T>>
    where
        T: Default,
    {
        let factory = self
            .transport_factory
            .unwrap_or_else(|| Box::new(T::default));
        let mut wb = WebBook::with_transport_factory(self.config, factory)?;

        if let Some(identity) = self.identity {
            wb.set_identity(identity)?;
        }

        Ok(wb)
    }
}

impl<T: Transport + Default> Default for WebBookBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}
