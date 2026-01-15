//! WebBook Mobile Bindings
//!
//! UniFFI bindings for Android and iOS platforms.
//! Exposes a simplified, mobile-friendly API on top of webbook-core.
//!
//! Note: Storage connections are created on-demand for thread safety,
//! as rusqlite's Connection is not Sync.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use webbook_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, IdentityBackup,
    SocialNetworkRegistry, Storage, SymmetricKey,
};

uniffi::setup_scaffolding!();

// === Error Types ===

/// Mobile-friendly error type.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    #[error("Library not initialized")]
    NotInitialized,

    #[error("Already initialized")]
    AlreadyInitialized,

    #[error("Identity not found")]
    IdentityNotFound,

    #[error("Contact not found: {0}")]
    ContactNotFound(String),

    #[error("Invalid QR code")]
    InvalidQrCode,

    #[error("Exchange failed: {0}")]
    ExchangeFailed(String),

    #[error("Sync failed: {0}")]
    SyncFailed(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Crypto error: {0}")]
    CryptoError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<webbook_core::StorageError> for MobileError {
    fn from(err: webbook_core::StorageError) -> Self {
        MobileError::StorageError(err.to_string())
    }
}

// === Data Types ===

/// Mobile-friendly field type enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileFieldType {
    Email,
    Phone,
    Website,
    Address,
    Social,
    Custom,
}

impl From<FieldType> for MobileFieldType {
    fn from(ft: FieldType) -> Self {
        match ft {
            FieldType::Email => MobileFieldType::Email,
            FieldType::Phone => MobileFieldType::Phone,
            FieldType::Website => MobileFieldType::Website,
            FieldType::Address => MobileFieldType::Address,
            FieldType::Social => MobileFieldType::Social,
            FieldType::Custom => MobileFieldType::Custom,
        }
    }
}

impl From<MobileFieldType> for FieldType {
    fn from(mft: MobileFieldType) -> Self {
        match mft {
            MobileFieldType::Email => FieldType::Email,
            MobileFieldType::Phone => FieldType::Phone,
            MobileFieldType::Website => FieldType::Website,
            MobileFieldType::Address => FieldType::Address,
            MobileFieldType::Social => FieldType::Social,
            MobileFieldType::Custom => FieldType::Custom,
        }
    }
}

/// Mobile-friendly contact field.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContactField {
    pub id: String,
    pub field_type: MobileFieldType,
    pub label: String,
    pub value: String,
}

impl From<&ContactField> for MobileContactField {
    fn from(field: &ContactField) -> Self {
        MobileContactField {
            id: field.id().to_string(),
            field_type: field.field_type().into(),
            label: field.label().to_string(),
            value: field.value().to_string(),
        }
    }
}

/// Mobile-friendly contact card.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContactCard {
    pub display_name: String,
    pub fields: Vec<MobileContactField>,
}

impl From<&ContactCard> for MobileContactCard {
    fn from(card: &ContactCard) -> Self {
        MobileContactCard {
            display_name: card.display_name().to_string(),
            fields: card.fields().iter().map(MobileContactField::from).collect(),
        }
    }
}

/// Mobile-friendly contact.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContact {
    pub id: String,
    pub display_name: String,
    pub is_verified: bool,
    pub card: MobileContactCard,
    pub added_at: u64,
}

impl From<&Contact> for MobileContact {
    fn from(contact: &Contact) -> Self {
        MobileContact {
            id: contact.id().to_string(),
            display_name: contact.display_name().to_string(),
            is_verified: contact.is_fingerprint_verified(),
            card: MobileContactCard::from(contact.card()),
            added_at: contact.exchange_timestamp(),
        }
    }
}

/// Exchange QR data.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileExchangeData {
    pub qr_data: String,
    pub public_id: String,
    pub expires_at: u64,
}

/// Exchange result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileExchangeResult {
    pub contact_id: String,
    pub contact_name: String,
    pub success: bool,
    pub error_message: Option<String>,
}

/// Sync status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileSyncStatus {
    Idle,
    Syncing,
    Error,
}

/// Social network info.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSocialNetwork {
    pub id: String,
    pub display_name: String,
    pub url_template: String,
}

// === Thread-safe state ===

/// Serializable identity data for thread-safe storage.
#[derive(Clone)]
#[allow(dead_code)]
struct IdentityData {
    backup_data: Vec<u8>,
    display_name: String,  // Reserved for future use
}

/// Main WebBook interface for mobile platforms.
///
/// Uses on-demand storage connections for thread safety.
#[derive(uniffi::Object)]
pub struct WebBookMobile {
    storage_path: PathBuf,
    storage_key: SymmetricKey,
    #[allow(dead_code)]
    relay_url: String,  // Reserved for future sync implementation
    identity_data: Mutex<Option<IdentityData>>,
    social_registry: SocialNetworkRegistry,
    sync_status: Mutex<MobileSyncStatus>,
}

impl WebBookMobile {
    /// Opens a storage connection.
    fn open_storage(&self) -> Result<Storage, MobileError> {
        Storage::open(&self.storage_path, self.storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))
    }

    /// Gets the identity from stored data.
    fn get_identity(&self) -> Result<Identity, MobileError> {
        let data = self.identity_data.lock().unwrap();
        let identity_data = data.as_ref().ok_or(MobileError::IdentityNotFound)?;

        let backup = IdentityBackup::new(identity_data.backup_data.clone());
        // Use a fixed internal password for in-memory storage
        Identity::import_backup(&backup, "__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))
    }
}

#[uniffi::export]
impl WebBookMobile {
    /// Create a new WebBookMobile instance.
    #[uniffi::constructor]
    pub fn new(data_dir: String, relay_url: String) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        // Ensure directory exists
        std::fs::create_dir_all(&data_path)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let storage_path = data_path.join("webbook.db");
        let storage_key = SymmetricKey::generate();

        // Initialize storage to ensure database is created
        let _storage = Storage::open(&storage_path, storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        Ok(Arc::new(WebBookMobile {
            storage_path,
            storage_key,
            relay_url,
            identity_data: Mutex::new(None),
            social_registry: SocialNetworkRegistry::with_defaults(),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
        }))
    }

    // === Identity Operations ===

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        self.identity_data.lock().unwrap().is_some()
    }

    /// Create a new identity.
    pub fn create_identity(&self, display_name: String) -> Result<(), MobileError> {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return Err(MobileError::AlreadyInitialized);
            }
        }

        let identity = Identity::create(&display_name);

        // Store identity as backup data
        let backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let identity_data = IdentityData {
            backup_data: backup.as_bytes().to_vec(),
            display_name: display_name.clone(),
        };

        *self.identity_data.lock().unwrap() = Some(identity_data);

        // Create initial contact card
        let storage = self.open_storage()?;
        let card = ContactCard::new(&display_name);
        storage.save_own_card(&card)?;

        Ok(())
    }

    /// Get public ID.
    pub fn get_public_id(&self) -> Result<String, MobileError> {
        let identity = self.get_identity()?;
        Ok(identity.public_id())
    }

    /// Get display name.
    pub fn get_display_name(&self) -> Result<String, MobileError> {
        let storage = self.open_storage()?;
        let card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;
        Ok(card.display_name().to_string())
    }

    // === Contact Card Operations ===

    /// Get own contact card.
    pub fn get_own_card(&self) -> Result<MobileContactCard, MobileError> {
        let storage = self.open_storage()?;
        let card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;
        Ok(MobileContactCard::from(&card))
    }

    /// Add field to own card.
    pub fn add_field(
        &self,
        field_type: MobileFieldType,
        label: String,
        value: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;

        let field = ContactField::new(field_type.into(), &label, &value);
        card.add_field(field).map_err(|e| MobileError::InvalidInput(e.to_string()))?;

        storage.save_own_card(&card)?;
        Ok(())
    }

    /// Update field value.
    pub fn update_field(&self, label: String, new_value: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;

        // Find field by label to get its ID
        let field_id = card.fields().iter()
            .find(|f| f.label() == label)
            .ok_or_else(|| MobileError::InvalidInput(format!("Field '{}' not found", label)))?
            .id()
            .to_string();

        card.update_field_value(&field_id, &new_value)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;

        storage.save_own_card(&card)?;
        Ok(())
    }

    /// Remove field from card.
    pub fn remove_field(&self, label: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;

        // Find field by label to get its ID
        let field_id = match card.fields().iter().find(|f| f.label() == label) {
            Some(f) => f.id().to_string(),
            None => return Ok(false),  // Field doesn't exist
        };

        card.remove_field(&field_id)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_own_card(&card)?;

        Ok(true)
    }

    /// Set display name.
    pub fn set_display_name(&self, name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;

        card.set_display_name(&name)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_own_card(&card)?;

        Ok(())
    }

    // === Contact Operations ===

    /// List all contacts.
    pub fn list_contacts(&self) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    /// Get single contact by ID.
    pub fn get_contact(&self, id: String) -> Result<Option<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contact = storage.load_contact(&id)?;
        Ok(contact.as_ref().map(MobileContact::from))
    }

    /// Search contacts.
    pub fn search_contacts(&self, query: String) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let query_lower = query.to_lowercase();

        let results: Vec<MobileContact> = contacts
            .iter()
            .filter(|c| c.display_name().to_lowercase().contains(&query_lower))
            .map(MobileContact::from)
            .collect();

        Ok(results)
    }

    /// Get contact count.
    pub fn contact_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        Ok(contacts.len() as u32)
    }

    /// Remove contact.
    pub fn remove_contact(&self, id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let removed = storage.delete_contact(&id)?;
        Ok(removed)
    }

    /// Verify contact fingerprint.
    pub fn verify_contact(&self, id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&id)?
            .ok_or_else(|| MobileError::ContactNotFound(id.clone()))?;

        contact.mark_fingerprint_verified();
        storage.save_contact(&contact)?;

        Ok(())
    }

    // === Visibility Operations ===

    /// Hide field from contact.
    pub fn hide_field_from_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::ContactNotFound(contact_id.clone()))?;

        // Find field ID by label
        let card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput(format!("Field not found: {}", field_label)))?;

        contact.visibility_rules_mut().set_nobody(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Show field to contact.
    pub fn show_field_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::ContactNotFound(contact_id.clone()))?;

        let card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput(format!("Field not found: {}", field_label)))?;

        contact.visibility_rules_mut().set_everyone(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Check if field is visible to contact.
    pub fn is_field_visible_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        let contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::ContactNotFound(contact_id.clone()))?;

        let card = storage.load_own_card()?.ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput(format!("Field not found: {}", field_label)))?;

        Ok(contact.visibility_rules().can_see(field.id(), &contact_id))
    }

    // === Exchange Operations ===

    /// Generate exchange QR data.
    pub fn generate_exchange_qr(&self) -> Result<MobileExchangeData, MobileError> {
        let identity = self.get_identity()?;

        let qr = webbook_core::ExchangeQR::generate(&identity);
        let qr_data = format!("wb://{}", qr.to_data_string());

        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 300; // 5 minutes

        Ok(MobileExchangeData {
            qr_data,
            public_id: identity.public_id(),
            expires_at,
        })
    }

    /// Complete exchange with scanned QR data.
    pub fn complete_exchange(&self, qr_data: String) -> Result<MobileExchangeResult, MobileError> {
        use webbook_core::{Contact, ExchangeQR, X3DH};
        use webbook_core::crypto::ratchet::DoubleRatchetState;

        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        // Parse QR data (remove "wb://" prefix if present)
        let data_str = qr_data.strip_prefix("wb://").unwrap_or(&qr_data);
        let their_qr = ExchangeQR::from_data_string(data_str)
            .map_err(|_| MobileError::InvalidQrCode)?;

        // Check if expired
        if their_qr.is_expired() {
            return Err(MobileError::ExchangeFailed("QR code expired".to_string()));
        }

        // Get their keys
        let their_signing_key = their_qr.public_key();
        let their_exchange_key = their_qr.exchange_key();
        let their_public_id = hex::encode(their_signing_key);

        // Check for duplicate
        if storage.load_contact(&their_public_id)?.is_some() {
            return Err(MobileError::ExchangeFailed("Contact already exists".to_string()));
        }

        // Perform X3DH key agreement
        let our_x3dh = identity.x3dh_keypair();
        let (shared_secret, _ephemeral_public) = X3DH::initiate(&our_x3dh, their_exchange_key)
            .map_err(|e| MobileError::ExchangeFailed(format!("Key agreement failed: {:?}", e)))?;

        // Create placeholder contact (real name comes via sync)
        let their_card = webbook_core::ContactCard::new("New Contact");
        let contact = Contact::from_exchange(
            *their_signing_key,
            their_card,
            shared_secret.clone(),
        );

        let contact_id = contact.id().to_string();
        let contact_name = contact.display_name().to_string();

        // Save contact
        storage.save_contact(&contact)?;

        // Initialize Double Ratchet as initiator
        let ratchet = DoubleRatchetState::initialize_initiator(
            &shared_secret,
            *their_exchange_key,
        );
        storage.save_ratchet_state(&contact_id, &ratchet, true)?;

        Ok(MobileExchangeResult {
            contact_id,
            contact_name,
            success: true,
            error_message: None,
        })
    }

    // === Sync Operations ===

    /// Sync with relay (placeholder - full implementation requires async).
    pub fn sync(&self) -> Result<(), MobileError> {
        *self.sync_status.lock().unwrap() = MobileSyncStatus::Syncing;

        // TODO: Implement actual relay sync
        // For now, this is a placeholder that just changes status

        *self.sync_status.lock().unwrap() = MobileSyncStatus::Idle;
        Ok(())
    }

    /// Get sync status.
    pub fn get_sync_status(&self) -> MobileSyncStatus {
        *self.sync_status.lock().unwrap()
    }

    /// Get pending update count.
    pub fn pending_update_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let mut total = 0u32;
        for contact in contacts {
            let pending = storage.get_pending_updates(contact.id())?;
            total += pending.len() as u32;
        }
        Ok(total)
    }

    // === Backup Operations ===

    /// Export encrypted backup.
    pub fn export_backup(&self, password: String) -> Result<String, MobileError> {
        let identity = self.get_identity()?;

        let backup = identity
            .export_backup(&password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        // Encode as base64
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(backup.as_bytes());

        Ok(encoded)
    }

    /// Import backup.
    pub fn import_backup(&self, backup_data: String, password: String) -> Result<(), MobileError> {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return Err(MobileError::AlreadyInitialized);
            }
        }

        // Decode from base64
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&backup_data)
            .map_err(|_| MobileError::InvalidInput("Invalid base64".to_string()))?;

        let backup = IdentityBackup::new(bytes);
        let identity = Identity::import_backup(&backup, &password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        // Re-export with internal key for storage
        let internal_backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let identity_data = IdentityData {
            backup_data: internal_backup.as_bytes().to_vec(),
            display_name: identity.display_name().to_string(),
        };

        *self.identity_data.lock().unwrap() = Some(identity_data);

        // Create contact card if it doesn't exist
        let storage = self.open_storage()?;
        if storage.load_own_card()?.is_none() {
            let card = ContactCard::new(identity.display_name());
            storage.save_own_card(&card)?;
        }

        Ok(())
    }

    // === Social Networks ===

    /// List available social networks.
    pub fn list_social_networks(&self) -> Vec<MobileSocialNetwork> {
        self.social_registry
            .all()
            .iter()
            .map(|sn| MobileSocialNetwork {
                id: sn.id().to_string(),
                display_name: sn.display_name().to_string(),
                url_template: sn.profile_url_template().to_string(),
            })
            .collect()
    }

    /// Search social networks.
    pub fn search_social_networks(&self, query: String) -> Vec<MobileSocialNetwork> {
        self.social_registry
            .search(&query)
            .iter()
            .map(|sn| MobileSocialNetwork {
                id: sn.id().to_string(),
                display_name: sn.display_name().to_string(),
                url_template: sn.profile_url_template().to_string(),
            })
            .collect()
    }

    /// Get profile URL for a social field.
    pub fn get_profile_url(&self, network_id: String, username: String) -> Option<String> {
        self.social_registry.profile_url(&network_id, &username)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_instance() -> (Arc<WebBookMobile>, TempDir) {
        let dir = TempDir::new().unwrap();
        let wb = WebBookMobile::new(
            dir.path().to_string_lossy().to_string(),
            "ws://localhost:8080".to_string(),
        )
        .unwrap();
        (wb, dir)
    }

    #[test]
    fn test_create_identity() {
        let (wb, _dir) = create_test_instance();
        assert!(!wb.has_identity());

        wb.create_identity("Alice".to_string()).unwrap();
        assert!(wb.has_identity());

        let name = wb.get_display_name().unwrap();
        assert_eq!(name, "Alice");
    }

    #[test]
    fn test_add_field() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Email,
            "work".to_string(),
            "alice@company.com".to_string(),
        )
        .unwrap();

        let card = wb.get_own_card().unwrap();
        assert_eq!(card.fields.len(), 1);
        assert_eq!(card.fields[0].label, "work");
        assert_eq!(card.fields[0].value, "alice@company.com");
    }

    #[test]
    fn test_update_field() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Phone,
            "mobile".to_string(),
            "+1234567890".to_string(),
        )
        .unwrap();

        wb.update_field("mobile".to_string(), "+0987654321".to_string())
            .unwrap();

        let card = wb.get_own_card().unwrap();
        assert_eq!(card.fields[0].value, "+0987654321");
    }

    #[test]
    fn test_remove_field() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Email,
            "work".to_string(),
            "alice@company.com".to_string(),
        )
        .unwrap();

        let removed = wb.remove_field("work".to_string()).unwrap();
        assert!(removed);

        let card = wb.get_own_card().unwrap();
        assert!(card.fields.is_empty());
    }

    #[test]
    fn test_social_networks() {
        let (wb, _dir) = create_test_instance();

        let networks = wb.list_social_networks();
        assert!(!networks.is_empty());

        let github = networks.iter().find(|n| n.id == "github");
        assert!(github.is_some());

        let url = wb.get_profile_url("github".to_string(), "octocat".to_string());
        assert_eq!(url, Some("https://github.com/octocat".to_string()));
    }

    #[test]
    fn test_exchange_qr_generation() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let exchange_data = wb.generate_exchange_qr().unwrap();
        assert!(exchange_data.qr_data.starts_with("wb://"), "QR data should start with wb://");
        assert!(!exchange_data.public_id.is_empty());
        assert!(exchange_data.expires_at > 0);
    }

    #[test]
    fn test_backup_restore() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Email,
            "work".to_string(),
            "alice@company.com".to_string(),
        )
        .unwrap();

        let backup = wb.export_backup("SecurePassword123".to_string()).unwrap();
        assert!(!backup.is_empty());

        // Create new instance and restore
        let dir2 = TempDir::new().unwrap();
        let wb2 = WebBookMobile::new(
            dir2.path().to_string_lossy().to_string(),
            "ws://localhost:8080".to_string(),
        )
        .unwrap();

        wb2.import_backup(backup, "SecurePassword123".to_string())
            .unwrap();

        assert!(wb2.has_identity());
        let name = wb2.get_display_name().unwrap();
        assert_eq!(name, "Alice");
    }
}
