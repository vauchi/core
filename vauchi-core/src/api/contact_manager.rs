//! Contact Manager
//!
//! High-level interface for contact operations.

use std::sync::Arc;

use crate::contact::Contact;
use crate::contact_card::{ContactCard, ContactField};
use crate::storage::Storage;

use super::error::{VauchiError, VauchiResult};
use super::events::{EventDispatcher, VauchiEvent};

/// Manages contacts and the user's own contact card.
///
/// Provides high-level operations for:
/// - Managing the user's own contact card
/// - Listing and searching contacts
/// - Updating contact visibility rules
/// - Removing contacts
pub struct ContactManager<'a> {
    storage: &'a Storage,
    events: Arc<EventDispatcher>,
}

impl<'a> ContactManager<'a> {
    /// Creates a new ContactManager.
    pub fn new(storage: &'a Storage, events: Arc<EventDispatcher>) -> Self {
        ContactManager { storage, events }
    }

    // === Own Card Operations ===

    /// Gets the user's own contact card.
    pub fn get_own_card(&self) -> VauchiResult<Option<ContactCard>> {
        Ok(self.storage.load_own_card()?)
    }

    /// Updates the user's own contact card.
    ///
    /// Returns the list of changed field names.
    pub fn update_own_card(&self, card: &ContactCard) -> VauchiResult<Vec<String>> {
        let old_card = self.storage.load_own_card()?;
        let changed_fields = match &old_card {
            Some(old) => Self::compute_changed_fields(old, card),
            None => card
                .fields()
                .iter()
                .map(|f| f.label().to_string())
                .collect(),
        };

        self.storage.save_own_card(card)?;

        if !changed_fields.is_empty() {
            self.events.dispatch(VauchiEvent::OwnCardUpdated {
                changed_fields: changed_fields.clone(),
            });
        }

        Ok(changed_fields)
    }

    /// Adds a field to the user's own card.
    pub fn add_field_to_own_card(&self, field: ContactField) -> VauchiResult<()> {
        let mut card = self
            .storage
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;

        card.add_field(field.clone())
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.save_own_card(&card)?;

        self.events.dispatch(VauchiEvent::OwnCardUpdated {
            changed_fields: vec![field.label().to_string()],
        });

        Ok(())
    }

    /// Removes a field from the user's own card by label.
    pub fn remove_field_from_own_card(&self, label: &str) -> VauchiResult<bool> {
        let mut card = self
            .storage
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Find field by label
        let field_id = card
            .fields()
            .iter()
            .find(|f| f.label() == label)
            .map(|f| f.id().to_string());

        let Some(field_id) = field_id else {
            return Ok(false);
        };

        card.remove_field(&field_id)
            .map_err(|_| VauchiError::InvalidState("Field not found".into()))?;

        self.storage.save_own_card(&card)?;
        self.events.dispatch(VauchiEvent::OwnCardUpdated {
            changed_fields: vec![label.to_string()],
        });

        Ok(true)
    }

    // === Contact Operations ===

    /// Gets a contact by ID.
    pub fn get_contact(&self, id: &str) -> VauchiResult<Option<Contact>> {
        Ok(self.storage.load_contact(id)?)
    }

    /// Gets a contact by ID, returning error if not found.
    pub fn get_contact_required(&self, id: &str) -> VauchiResult<Contact> {
        self.storage
            .load_contact(id)?
            .ok_or_else(|| VauchiError::ContactNotFound(id.to_string()))
    }

    /// Lists all contacts.
    pub fn list_contacts(&self) -> VauchiResult<Vec<Contact>> {
        Ok(self.storage.list_contacts()?)
    }

    /// Searches contacts by display name (case-insensitive).
    pub fn search_contacts(&self, query: &str) -> VauchiResult<Vec<Contact>> {
        let query_lower = query.to_lowercase();
        let contacts = self.storage.list_contacts()?;

        Ok(contacts
            .into_iter()
            .filter(|c| c.display_name().to_lowercase().contains(&query_lower))
            .collect())
    }

    /// Returns the number of contacts.
    pub fn contact_count(&self) -> VauchiResult<usize> {
        Ok(self.storage.list_contacts()?.len())
    }

    /// Adds a new contact from an exchange.
    ///
    /// This is typically called after a successful key exchange.
    pub fn add_contact(&self, contact: Contact) -> VauchiResult<()> {
        let contact_id = contact.id().to_string();

        // Check if already exists
        if self.storage.load_contact(&contact_id)?.is_some() {
            return Err(VauchiError::InvalidState(format!(
                "Contact {} already exists",
                contact_id
            )));
        }

        self.storage.save_contact(&contact)?;

        self.events
            .dispatch(VauchiEvent::ContactAdded { contact_id });

        Ok(())
    }

    /// Updates an existing contact.
    pub fn update_contact(&self, contact: &Contact) -> VauchiResult<Vec<String>> {
        let contact_id = contact.id().to_string();

        let old_contact = self
            .storage
            .load_contact(&contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.clone()))?;

        let changed_fields = Self::compute_changed_fields(old_contact.card(), contact.card());

        self.storage.save_contact(contact)?;

        if !changed_fields.is_empty() {
            self.events.dispatch(VauchiEvent::ContactUpdated {
                contact_id,
                changed_fields: changed_fields.clone(),
            });
        }

        Ok(changed_fields)
    }

    /// Removes a contact by ID.
    pub fn remove_contact(&self, id: &str) -> VauchiResult<bool> {
        let existed = self.storage.delete_contact(id)?;

        if existed {
            self.events.dispatch(VauchiEvent::ContactRemoved {
                contact_id: id.to_string(),
            });
        }

        Ok(existed)
    }

    /// Marks a contact's fingerprint as verified.
    pub fn verify_fingerprint(&self, id: &str) -> VauchiResult<()> {
        let mut contact = self.get_contact_required(id)?;
        contact.mark_fingerprint_verified();
        self.storage.save_contact(&contact)?;
        Ok(())
    }

    // === Visibility Operations ===

    /// Sets a field as visible to everyone for a contact.
    pub fn set_field_public(&self, contact_id: &str, field: &str) -> VauchiResult<()> {
        let mut contact = self.get_contact_required(contact_id)?;
        contact.visibility_rules_mut().set_everyone(field);
        self.storage.save_contact(&contact)?;
        Ok(())
    }

    /// Sets a field as visible to nobody for a contact.
    pub fn set_field_private(&self, contact_id: &str, field: &str) -> VauchiResult<()> {
        let mut contact = self.get_contact_required(contact_id)?;
        contact.visibility_rules_mut().set_nobody(field);
        self.storage.save_contact(&contact)?;
        Ok(())
    }

    /// Sets a field as visible to specific contacts.
    pub fn set_field_restricted(
        &self,
        contact_id: &str,
        field: &str,
        allowed_contacts: Vec<String>,
    ) -> VauchiResult<()> {
        use std::collections::HashSet;
        let mut contact = self.get_contact_required(contact_id)?;
        let allowed_set: HashSet<String> = allowed_contacts.into_iter().collect();
        contact
            .visibility_rules_mut()
            .set_contacts(field, allowed_set);
        self.storage.save_contact(&contact)?;
        Ok(())
    }

    // === Helper Methods ===

    /// Computes which fields changed between two cards.
    fn compute_changed_fields(old: &ContactCard, new: &ContactCard) -> Vec<String> {
        let mut changed = Vec::new();

        // Helper to find field by label
        fn find_field_by_label<'a>(card: &'a ContactCard, label: &str) -> Option<&'a ContactField> {
            card.fields().iter().find(|f| f.label() == label)
        }

        // Check for modified or removed fields
        for old_field in old.fields() {
            match find_field_by_label(new, old_field.label()) {
                Some(new_field) if new_field.value() != old_field.value() => {
                    changed.push(old_field.label().to_string());
                }
                None => {
                    changed.push(old_field.label().to_string());
                }
                _ => {}
            }
        }

        // Check for new fields
        for new_field in new.fields() {
            if find_field_by_label(old, new_field.label()).is_none() {
                changed.push(new_field.label().to_string());
            }
        }

        // Check display name
        if old.display_name() != new.display_name() {
            changed.push("display_name".to_string());
        }

        changed
    }
}

// INLINE_TEST_REQUIRED: Tests private compute_changed_fields function for change detection logic
#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::FieldType;
    use crate::crypto::SymmetricKey;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn create_test_storage() -> Storage {
        let key = SymmetricKey::generate();
        Storage::in_memory(key).unwrap()
    }

    fn create_test_contact(name: &str, pk: [u8; 32]) -> Contact {
        let card = ContactCard::new(name);
        let shared_key = SymmetricKey::generate();
        Contact::from_exchange(pk, card, shared_key)
    }

    #[test]
    fn test_own_card_operations() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        // Initially no card
        assert!(manager.get_own_card().unwrap().is_none());

        // Create card
        let card = ContactCard::new("Test User");
        manager.update_own_card(&card).unwrap();

        let loaded = manager.get_own_card().unwrap().unwrap();
        assert_eq!(loaded.display_name(), "Test User");
    }

    #[test]
    fn test_own_card_add_field() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        // Create initial card
        let card = ContactCard::new("Test User");
        manager.update_own_card(&card).unwrap();

        // Add field
        let field = ContactField::new(FieldType::Email, "email", "test@example.com");
        manager.add_field_to_own_card(field).unwrap();

        let loaded = manager.get_own_card().unwrap().unwrap();
        assert_eq!(loaded.fields().len(), 1);
        assert!(loaded.fields().iter().any(|f| f.label() == "email"));
    }

    #[test]
    fn test_own_card_remove_field() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        // Create card with field
        let mut card = ContactCard::new("Test User");
        card.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "test@example.com",
        ))
        .unwrap();
        manager.update_own_card(&card).unwrap();

        // Remove field
        let removed = manager.remove_field_from_own_card("email").unwrap();
        assert!(removed);

        let loaded = manager.get_own_card().unwrap().unwrap();
        assert_eq!(loaded.fields().len(), 0);
    }

    #[test]
    fn test_contact_operations() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        let contact = create_test_contact("Alice", [1u8; 32]);
        let contact_id = contact.id().to_string();

        // Add contact
        manager.add_contact(contact).unwrap();

        // Get contact
        let loaded = manager.get_contact(&contact_id).unwrap().unwrap();
        assert_eq!(loaded.display_name(), "Alice");

        // List contacts
        let contacts = manager.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);

        // Remove contact
        let removed = manager.remove_contact(&contact_id).unwrap();
        assert!(removed);

        assert!(manager.get_contact(&contact_id).unwrap().is_none());
    }

    #[test]
    fn test_contact_not_found() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        let result = manager.get_contact_required("nonexistent");
        assert!(matches!(result, Err(VauchiError::ContactNotFound(_))));
    }

    #[test]
    fn test_search_contacts() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        manager
            .add_contact(create_test_contact("Alice", [1u8; 32]))
            .unwrap();
        manager
            .add_contact(create_test_contact("Bob", [2u8; 32]))
            .unwrap();
        manager
            .add_contact(create_test_contact("Alice Smith", [3u8; 32]))
            .unwrap();

        // Search for "Alice"
        let results = manager.search_contacts("alice").unwrap();
        assert_eq!(results.len(), 2);

        // Search for "Bob"
        let results = manager.search_contacts("bob").unwrap();
        assert_eq!(results.len(), 1);

        // Search for non-existent
        let results = manager.search_contacts("Charlie").unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_contact_count() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        assert_eq!(manager.contact_count().unwrap(), 0);

        manager
            .add_contact(create_test_contact("Alice", [1u8; 32]))
            .unwrap();
        assert_eq!(manager.contact_count().unwrap(), 1);

        manager
            .add_contact(create_test_contact("Bob", [2u8; 32]))
            .unwrap();
        assert_eq!(manager.contact_count().unwrap(), 2);
    }

    #[test]
    fn test_verify_fingerprint() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        let contact = create_test_contact("Alice", [1u8; 32]);
        let contact_id = contact.id().to_string();
        manager.add_contact(contact).unwrap();

        // Initially not verified
        let loaded = manager.get_contact(&contact_id).unwrap().unwrap();
        assert!(!loaded.is_fingerprint_verified());

        // Verify
        manager.verify_fingerprint(&contact_id).unwrap();

        let loaded = manager.get_contact(&contact_id).unwrap().unwrap();
        assert!(loaded.is_fingerprint_verified());
    }

    #[test]
    fn test_visibility_operations() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        let contact = create_test_contact("Alice", [1u8; 32]);
        let contact_id = contact.id().to_string();
        manager.add_contact(contact).unwrap();

        // Set field private
        manager.set_field_private(&contact_id, "email").unwrap();

        let loaded = manager.get_contact(&contact_id).unwrap().unwrap();
        assert!(!loaded.visibility_rules().can_see("email", "anyone"));

        // Set field public
        manager.set_field_public(&contact_id, "email").unwrap();

        let loaded = manager.get_contact(&contact_id).unwrap().unwrap();
        assert!(loaded.visibility_rules().can_see("email", "anyone"));
    }

    #[test]
    fn test_event_dispatch_on_add_contact() {
        let storage = create_test_storage();
        let mut dispatcher = EventDispatcher::new();
        let event_count = Arc::new(AtomicUsize::new(0));

        let count = event_count.clone();
        let handler = Arc::new(super::super::events::CallbackHandler::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        dispatcher.add_handler(handler);

        let events = Arc::new(dispatcher);
        let manager = ContactManager::new(&storage, events);
        manager
            .add_contact(create_test_contact("Alice", [1u8; 32]))
            .unwrap();

        assert_eq!(event_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_duplicate_contact_error() {
        let storage = create_test_storage();
        let events = Arc::new(EventDispatcher::new());
        let manager = ContactManager::new(&storage, events);

        let contact = create_test_contact("Alice", [1u8; 32]);
        manager.add_contact(contact.clone()).unwrap();

        // Try to add again
        let result = manager.add_contact(contact);
        assert!(matches!(result, Err(VauchiError::InvalidState(_))));
    }

    #[test]
    fn test_compute_changed_fields() {
        let mut card1 = ContactCard::new("User");
        card1
            .add_field(ContactField::new(
                FieldType::Email,
                "email",
                "old@example.com",
            ))
            .unwrap();

        let mut card2 = ContactCard::new("User");
        card2
            .add_field(ContactField::new(
                FieldType::Email,
                "email",
                "new@example.com",
            ))
            .unwrap();
        card2
            .add_field(ContactField::new(FieldType::Phone, "phone", "+1234567890"))
            .unwrap();

        let changed = ContactManager::<'_>::compute_changed_fields(&card1, &card2);

        assert!(changed.contains(&"email".to_string())); // Modified
        assert!(changed.contains(&"phone".to_string())); // Added
    }
}
