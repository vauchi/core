//! WebBook Orchestrator
//!
//! Main entry point for the WebBook API.

use std::sync::Arc;

use crate::contact::Contact;
use crate::contact_card::{ContactCard, ContactField};
use crate::crypto::SymmetricKey;
use crate::identity::Identity;
use crate::network::{MockTransport, Transport};
use crate::storage::Storage;

use super::config::WebBookConfig;
use super::contact_manager::ContactManager;
use super::error::{WebBookError, WebBookResult};
use super::events::{EventDispatcher, EventHandler, WebBookEvent};
use super::sync_controller::SyncController;

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
    #[allow(dead_code)]  // For future sync integration
    sync_controller: Option<SyncController<'static, T>>,
    /// Leaked storage reference for sync controller
    /// This is necessary because SyncController needs a 'static reference
    #[allow(dead_code)]  // For future sync integration
    storage_ref: Option<&'static Storage>,
}

impl WebBook<MockTransport> {
    /// Creates a new WebBook instance with mock transport (for testing).
    pub fn new(config: WebBookConfig) -> WebBookResult<Self> {
        Self::with_transport_factory(config, MockTransport::new)
    }
}

impl<T: Transport> WebBook<T> {
    /// Creates a new WebBook instance with a custom transport factory.
    pub fn with_transport_factory<F>(config: WebBookConfig, _transport_factory: F) -> WebBookResult<Self>
    where
        F: FnOnce() -> T,
    {
        // Use provided storage key or generate a new one
        let storage_key = config.storage_key.clone().unwrap_or_else(SymmetricKey::generate);

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
            sync_controller: None,
            storage_ref: None,
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
            sync_controller: None,
            storage_ref: None,
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
        self.identity.as_ref()
            .map(|id| id.public_id())
            .ok_or(WebBookError::IdentityNotInitialized)
    }

    /// Returns true if an identity has been created or set.
    pub fn has_identity(&self) -> bool {
        self.identity.is_some()
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
        let factory = self.transport_factory.unwrap_or_else(|| Box::new(T::default));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::FieldType;

    fn create_test_webbook() -> WebBook<MockTransport> {
        WebBook::in_memory().unwrap()
    }

    #[test]
    fn test_webbook_create_identity() {
        let mut wb = create_test_webbook();

        assert!(!wb.has_identity());

        wb.create_identity("Alice").unwrap();

        assert!(wb.has_identity());
        assert_eq!(wb.identity().unwrap().display_name(), "Alice");
    }

    #[test]
    fn test_webbook_create_identity_twice_fails() {
        let mut wb = create_test_webbook();

        wb.create_identity("Alice").unwrap();

        let result = wb.create_identity("Bob");
        assert!(matches!(result, Err(WebBookError::AlreadyInitialized)));
    }

    #[test]
    fn test_webbook_own_card() {
        let mut wb = create_test_webbook();
        wb.create_identity("Alice").unwrap();

        let card = wb.own_card().unwrap().unwrap();
        assert_eq!(card.display_name(), "Alice");
    }

    #[test]
    fn test_webbook_update_own_card() {
        let mut wb = create_test_webbook();
        wb.create_identity("Alice").unwrap();

        let mut card = wb.own_card().unwrap().unwrap();
        card.add_field(ContactField::new(FieldType::Email, "email", "alice@example.com"));

        let changed = wb.update_own_card(&card).unwrap();
        assert!(changed.contains(&"email".to_string()));

        let loaded = wb.own_card().unwrap().unwrap();
        assert!(loaded.fields().iter().any(|f| f.label() == "email"));
    }

    #[test]
    fn test_webbook_add_own_field() {
        let mut wb = create_test_webbook();
        wb.create_identity("Alice").unwrap();

        let field = ContactField::new(FieldType::Phone, "phone", "+1234567890");
        wb.add_own_field(field).unwrap();

        let card = wb.own_card().unwrap().unwrap();
        assert!(card.fields().iter().any(|f| f.label() == "phone"));
    }

    #[test]
    fn test_webbook_remove_own_field() {
        let mut wb = create_test_webbook();
        wb.create_identity("Alice").unwrap();

        // Add field
        let field = ContactField::new(FieldType::Phone, "phone", "+1234567890");
        wb.add_own_field(field).unwrap();

        // Remove field
        let removed = wb.remove_own_field("phone").unwrap();
        assert!(removed);

        let card = wb.own_card().unwrap().unwrap();
        assert!(!card.fields().iter().any(|f| f.label() == "phone"));
    }

    #[test]
    fn test_webbook_contact_operations() {
        let wb = create_test_webbook();

        // Initially no contacts
        assert_eq!(wb.contact_count().unwrap(), 0);
        assert!(wb.list_contacts().unwrap().is_empty());

        // Add contact
        let contact = Contact::from_exchange(
            [1u8; 32],
            ContactCard::new("Bob"),
            SymmetricKey::generate(),
        );
        let contact_id = contact.id().to_string();
        wb.add_contact(contact).unwrap();

        // Verify contact exists
        assert_eq!(wb.contact_count().unwrap(), 1);
        assert!(wb.get_contact(&contact_id).unwrap().is_some());

        // Search contacts
        let results = wb.search_contacts("bob").unwrap();
        assert_eq!(results.len(), 1);

        // Remove contact
        let removed = wb.remove_contact(&contact_id).unwrap();
        assert!(removed);
        assert_eq!(wb.contact_count().unwrap(), 0);
    }

    #[test]
    fn test_webbook_verify_fingerprint() {
        let wb = create_test_webbook();

        let contact = Contact::from_exchange(
            [1u8; 32],
            ContactCard::new("Bob"),
            SymmetricKey::generate(),
        );
        let contact_id = contact.id().to_string();
        wb.add_contact(contact).unwrap();

        // Initially not verified
        let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
        assert!(!loaded.is_fingerprint_verified());

        // Verify
        wb.verify_contact_fingerprint(&contact_id).unwrap();

        let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
        assert!(loaded.is_fingerprint_verified());
    }

    #[test]
    fn test_webbook_public_id() {
        let mut wb = create_test_webbook();

        // No identity yet
        let result = wb.public_id();
        assert!(matches!(result, Err(WebBookError::IdentityNotInitialized)));

        // Create identity
        wb.create_identity("Alice").unwrap();

        let public_id = wb.public_id().unwrap();
        assert!(!public_id.is_empty());
    }

    #[test]
    fn test_webbook_builder() {
        let wb: WebBook<MockTransport> = WebBookBuilder::new()
            .storage_path("/tmp/test_webbook")
            .relay_url("wss://relay.example.com")
            .build()
            .unwrap();

        assert_eq!(wb.config().relay.server_url, "wss://relay.example.com");
    }

    #[test]
    fn test_webbook_builder_with_identity() {
        let identity = Identity::create("Alice");
        let public_id = identity.public_id();

        let wb: WebBook<MockTransport> = WebBookBuilder::new()
            .storage_path("/tmp/test_webbook2")
            .identity(identity)
            .build()
            .unwrap();

        assert!(wb.has_identity());
        assert_eq!(wb.public_id().unwrap(), public_id);
    }
}
