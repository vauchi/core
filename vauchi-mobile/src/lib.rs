//! Vauchi Mobile Bindings
//!
//! UniFFI bindings for Android and iOS platforms.
//! Exposes a simplified, mobile-friendly API on top of vauchi-core.
//!
//! Note: Storage connections are created on-demand for thread safety,
//! as rusqlite's Connection is not Sync.

use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::EncryptedExchangeMessage;
use vauchi_core::recovery::{RecoveryClaim, RecoveryProof, RecoveryVoucher};
use vauchi_core::{
    Contact, ContactCard, ContactField, Identity, IdentityBackup, SocialNetworkRegistry, Storage,
    SymmetricKey,
};

// === Modules ===

mod audio;
mod cert_pinning;
mod error;
mod protocol;
mod sync;
mod types;

// Re-export public types
pub use audio::{MobileProximityResult, MobileProximityVerifier, PlatformAudioHandler};
pub use error::MobileError;
pub use types::{
    MobileAhaMoment, MobileAhaMomentType, MobileContact, MobileContactCard, MobileContactField,
    MobileDeliveryRecord, MobileDeliveryStatus, MobileDeliverySummary, MobileDemoContact,
    MobileDemoContactState, MobileDeviceDeliveryRecord, MobileDeviceDeliveryStatus,
    MobileExchangeData, MobileExchangeResult, MobileFieldType, MobileFieldValidation,
    MobileRecoveryClaim, MobileRecoveryProgress, MobileRecoveryVerification, MobileRecoveryVoucher,
    MobileRetryEntry, MobileSocialNetwork, MobileSyncResult, MobileSyncStatus, MobileTrustLevel,
    MobileValidationStatus, MobileVisibilityLabel, MobileVisibilityLabelDetail,
};

uniffi::setup_scaffolding!();

// === Password Strength ===

/// Password strength level for display to users.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobilePasswordStrength {
    /// Score 0-1: Too weak to use
    TooWeak,
    /// Score 2: Fair but not recommended
    Fair,
    /// Score 3: Strong enough
    Strong,
    /// Score 4: Very strong
    VeryStrong,
}

/// Result of password strength check.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobilePasswordCheck {
    /// The strength level
    pub strength: MobilePasswordStrength,
    /// Human-readable description
    pub description: String,
    /// Feedback/suggestions for improvement (empty if strong enough)
    pub feedback: String,
    /// Whether the password is acceptable for backup
    pub is_acceptable: bool,
}

/// Check password strength for backup encryption.
///
/// Returns strength level, description, and feedback for improvement.
#[uniffi::export]
pub fn check_password_strength(password: String) -> MobilePasswordCheck {
    use vauchi_core::identity::password::{password_feedback, validate_password};

    // Short passwords get immediate feedback
    if password.len() < 8 {
        return MobilePasswordCheck {
            strength: MobilePasswordStrength::TooWeak,
            description: "Too short".to_string(),
            feedback: "Password must be at least 8 characters".to_string(),
            is_acceptable: false,
        };
    }

    // Check with zxcvbn via core
    match validate_password(&password) {
        Ok(strength) => {
            use vauchi_core::identity::password::PasswordStrength;
            let (level, description) = match strength {
                PasswordStrength::Strong => (MobilePasswordStrength::Strong, "Strong"),
                PasswordStrength::VeryStrong => (MobilePasswordStrength::VeryStrong, "Very strong"),
                _ => (MobilePasswordStrength::Fair, "Fair"),
            };
            MobilePasswordCheck {
                strength: level,
                description: description.to_string(),
                feedback: String::new(),
                is_acceptable: true,
            }
        }
        Err(_) => {
            // Get feedback for weak passwords
            let feedback = password_feedback(&password);
            let estimate = zxcvbn::zxcvbn(&password, &[]);
            let (level, description) = match estimate.score() {
                zxcvbn::Score::Zero | zxcvbn::Score::One => {
                    (MobilePasswordStrength::TooWeak, "Too weak")
                }
                zxcvbn::Score::Two => (MobilePasswordStrength::Fair, "Fair"),
                _ => (MobilePasswordStrength::Fair, "Fair"),
            };
            MobilePasswordCheck {
                strength: level,
                description: description.to_string(),
                feedback: if feedback.is_empty() {
                    "Add more words or use a passphrase".to_string()
                } else {
                    feedback
                },
                is_acceptable: false,
            }
        }
    }
}

// === Thread-safe state ===

/// Serializable identity data for thread-safe storage.
#[derive(Clone)]
#[allow(dead_code)]
struct IdentityData {
    backup_data: Vec<u8>,
    display_name: String, // Reserved for future use
}

/// Generate a new random storage key.
///
/// Use this when setting up a new installation with secure storage.
/// The returned bytes should be stored in platform secure storage
/// (iOS Keychain or Android KeyStore).
#[uniffi::export]
pub fn generate_storage_key() -> Vec<u8> {
    SymmetricKey::generate().as_bytes().to_vec()
}

/// Check if a URL is safe to open in an external application.
///
/// Returns `true` for allowed schemes: http, https, tel, mailto, sms, geo.
/// Returns `false` for blocked schemes (javascript, data, file, etc.) or unknown schemes.
///
/// Use this to validate URLs before opening them to prevent security issues.
#[uniffi::export]
pub fn is_safe_url(url: String) -> bool {
    vauchi_core::is_safe_url(&url)
}

/// Check if a URL scheme is in the allowed list.
///
/// Allowed schemes: tel, mailto, sms, https, http, geo.
#[uniffi::export]
pub fn is_allowed_scheme(scheme: String) -> bool {
    vauchi_core::is_allowed_scheme(&scheme)
}

/// Check if a URL scheme is explicitly blocked.
///
/// Blocked schemes: javascript, vbscript, data, file, ftp, blob.
#[uniffi::export]
pub fn is_blocked_scheme(scheme: String) -> bool {
    vauchi_core::is_blocked_scheme(&scheme)
}

// === Main Interface ===

/// Main Vauchi interface for mobile platforms.
///
/// Uses on-demand storage connections for thread safety.
#[derive(uniffi::Object)]
pub struct VauchiMobile {
    storage_path: PathBuf,
    storage_key: SymmetricKey,
    relay_url: String,
    /// Optional PEM-encoded certificate for TLS pinning.
    pinned_cert_pem: Mutex<Option<String>>,
    identity_data: Mutex<Option<IdentityData>>,
    social_registry: SocialNetworkRegistry,
    sync_status: Mutex<MobileSyncStatus>,
}

impl VauchiMobile {
    /// Opens a storage connection.
    fn open_storage(&self) -> Result<Storage, MobileError> {
        Storage::open(&self.storage_path, self.storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))
    }

    /// Connect to relay with optional certificate pinning.
    fn connect_to_relay(&self) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, MobileError> {
        let pinned_cert = self.pinned_cert_pem.lock().unwrap();
        let cert_pem = pinned_cert.as_deref();
        cert_pinning::connect_with_pinning(&self.relay_url, cert_pem)
            .map_err(MobileError::NetworkError)
    }

    /// Gets the identity from stored data.
    fn get_identity(&self) -> Result<Identity, MobileError> {
        let data = self.identity_data.lock().unwrap();
        let identity_data = data.as_ref().ok_or(MobileError::IdentityNotFound)?;

        let backup = IdentityBackup::new(identity_data.backup_data.clone());
        Identity::import_backup(&backup, "__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))
    }

    /// Get pinned certificate if set.
    fn get_pinned_cert(&self) -> Option<String> {
        self.pinned_cert_pem.lock().unwrap().clone()
    }

    /// Get the path to the recovery proof file.
    fn recovery_proof_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".recovery_proof")
    }

    // === Aha Moments (internal helpers) ===

    /// Get the path to the aha moments state file.
    fn aha_moments_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".aha_moments")
    }

    /// Load the aha moments tracker from storage.
    fn load_aha_tracker(&self) -> vauchi_core::AhaMomentTracker {
        let path = self.aha_moments_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::AhaMomentTracker::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::AhaMomentTracker::new()
        }
    }

    /// Save the aha moments tracker to storage.
    fn save_aha_tracker(&self, tracker: &vauchi_core::AhaMomentTracker) -> Result<(), MobileError> {
        let path = self.aha_moments_path();
        let data = tracker
            .to_json()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(())
    }

    // === Demo Contact (internal helpers) ===

    /// Get the path to the demo contact state file.
    fn demo_contact_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".demo_contact")
    }

    /// Load the demo contact state from storage.
    fn load_demo_state(&self) -> vauchi_core::DemoContactState {
        let path = self.demo_contact_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::DemoContactState::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::DemoContactState::default()
        }
    }

    /// Save the demo contact state to storage.
    fn save_demo_state(&self, state: &vauchi_core::DemoContactState) -> Result<(), MobileError> {
        let path = self.demo_contact_path();
        let data = state
            .to_json()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(())
    }
}

#[uniffi::export]
impl VauchiMobile {
    /// Create a new VauchiMobile instance with a platform-provided secure key.
    ///
    /// This is the recommended constructor. The platform (iOS/Android) should:
    /// 1. Generate a 32-byte key if one doesn't exist in secure storage
    /// 2. Store it in platform-specific secure storage (Keychain/KeyStore)
    /// 3. Pass the key bytes to this constructor
    #[uniffi::constructor]
    pub fn new_with_secure_key(
        data_dir: String,
        relay_url: String,
        storage_key_bytes: Vec<u8>,
    ) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        std::fs::create_dir_all(&data_path)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let storage_path = data_path.join("vauchi.db");

        let key_array: [u8; 32] = storage_key_bytes.try_into().map_err(|_| {
            MobileError::StorageError("Storage key must be exactly 32 bytes".to_string())
        })?;
        let storage_key = SymmetricKey::from_bytes(key_array);

        let _storage = Storage::open(&storage_path, storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        Ok(Arc::new(VauchiMobile {
            storage_path,
            storage_key,
            relay_url,
            pinned_cert_pem: Mutex::new(None),
            identity_data: Mutex::new(None),
            social_registry: SocialNetworkRegistry::with_defaults(),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
        }))
    }

    /// Create a new VauchiMobile instance (legacy constructor).
    ///
    /// WARNING: This constructor stores the encryption key in a plaintext file.
    /// Use `new_with_secure_key` instead for production.
    #[uniffi::constructor]
    pub fn new(data_dir: String, relay_url: String) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        std::fs::create_dir_all(&data_path)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let storage_path = data_path.join("vauchi.db");
        let key_path = data_path.join("storage.key");

        let storage_key = if key_path.exists() {
            let key_bytes = std::fs::read(&key_path)
                .map_err(|e| MobileError::StorageError(format!("Failed to read key: {}", e)))?;
            let key_array: [u8; 32] = key_bytes
                .try_into()
                .map_err(|_| MobileError::StorageError("Invalid key length".to_string()))?;
            SymmetricKey::from_bytes(key_array)
        } else {
            let key = SymmetricKey::generate();
            std::fs::write(&key_path, key.as_bytes())
                .map_err(|e| MobileError::StorageError(format!("Failed to save key: {}", e)))?;
            key
        };

        let _storage = Storage::open(&storage_path, storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        Ok(Arc::new(VauchiMobile {
            storage_path,
            storage_key,
            relay_url,
            pinned_cert_pem: Mutex::new(None),
            identity_data: Mutex::new(None),
            social_registry: SocialNetworkRegistry::with_defaults(),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
        }))
    }

    /// Export the current storage key bytes for migration to secure storage.
    pub fn export_storage_key(&self) -> Vec<u8> {
        self.storage_key.as_bytes().to_vec()
    }

    /// Set the pinned certificate for relay TLS connections.
    ///
    /// The certificate should be in PEM format. Once set, only connections
    /// to relay servers presenting this exact certificate will be allowed.
    pub fn set_pinned_certificate(&self, cert_pem: String) {
        let mut pinned = self.pinned_cert_pem.lock().unwrap();
        if cert_pem.is_empty() {
            *pinned = None;
        } else {
            *pinned = Some(cert_pem);
        }
    }

    /// Check if certificate pinning is enabled.
    pub fn is_certificate_pinning_enabled(&self) -> bool {
        self.pinned_cert_pem.lock().unwrap().is_some()
    }

    // === Identity Operations ===

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return true;
            }
        }

        if let Ok(storage) = self.open_storage() {
            if let Ok(Some((backup_data, display_name))) = storage.load_identity() {
                let identity_data = IdentityData {
                    backup_data,
                    display_name,
                };
                *self.identity_data.lock().unwrap() = Some(identity_data);
                return true;
            }
        }

        false
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

        let backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let backup_data = backup.as_bytes().to_vec();

        let storage = self.open_storage()?;
        storage.save_identity(&backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data,
            display_name: display_name.clone(),
        };
        *self.identity_data.lock().unwrap() = Some(identity_data);

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
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        Ok(card.display_name().to_string())
    }

    // === Contact Card Operations ===

    /// Get own contact card.
    pub fn get_own_card(&self) -> Result<MobileContactCard, MobileError> {
        let storage = self.open_storage()?;
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
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

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field = ContactField::new(field_type.into(), &label, &value);
        card.add_field(field)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;

        storage.save_own_card(&card)?;
        Ok(())
    }

    /// Update field value.
    pub fn update_field(&self, label: String, new_value: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field_id = card
            .fields()
            .iter()
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

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field_id = match card.fields().iter().find(|f| f.label() == label) {
            Some(f) => f.id().to_string(),
            None => return Ok(false),
        };

        card.remove_field(&field_id)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_own_card(&card)?;

        Ok(true)
    }

    /// Set display name.
    pub fn set_display_name(&self, name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

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

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

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

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

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

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        Ok(contact.visibility_rules().can_see(field.id(), &contact_id))
    }

    // === Visibility Labels ===

    /// List all visibility labels.
    pub fn list_labels(&self) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.load_all_labels()?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Create a new visibility label.
    pub fn create_label(&self, name: String) -> Result<MobileVisibilityLabel, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.create_label(&name)?;
        Ok(MobileVisibilityLabel::from(&label))
    }

    /// Get a label by ID with full details.
    pub fn get_label(&self, label_id: String) -> Result<MobileVisibilityLabelDetail, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.load_label(&label_id)?;
        Ok(MobileVisibilityLabelDetail::from(&label))
    }

    /// Rename a label.
    pub fn rename_label(&self, label_id: String, new_name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.rename_label(&label_id, &new_name)?;
        Ok(())
    }

    /// Delete a label.
    pub fn delete_label(&self, label_id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.delete_label(&label_id)?;
        Ok(())
    }

    /// Add a contact to a label.
    pub fn add_contact_to_label(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.add_contact_to_label(&label_id, &contact_id)?;
        Ok(())
    }

    /// Remove a contact from a label.
    pub fn remove_contact_from_label(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.remove_contact_from_label(&label_id, &contact_id)?;
        Ok(())
    }

    /// Get all labels that contain a contact.
    pub fn get_labels_for_contact(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.get_labels_for_contact(&contact_id)?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Set whether a field is visible to contacts in a label.
    pub fn set_label_field_visibility(
        &self,
        label_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        // Get field ID from label
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        storage.set_label_field_visibility(&label_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Set a per-contact override for field visibility.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        storage.save_contact_override(&contact_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Remove a per-contact override for field visibility.
    pub fn remove_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        storage.delete_contact_override(&contact_id, field.id())?;
        Ok(())
    }

    /// Get suggested default labels.
    pub fn get_suggested_labels(&self) -> Vec<String> {
        vauchi_core::SUGGESTED_LABELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    // === Exchange Operations ===

    /// Generate exchange QR data.
    pub fn generate_exchange_qr(&self) -> Result<MobileExchangeData, MobileError> {
        let identity = self.get_identity()?;

        let qr = vauchi_core::ExchangeQR::generate(&identity);
        let qr_data = format!("wb://{}", qr.to_data_string());

        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 300;

        Ok(MobileExchangeData {
            qr_data,
            public_id: identity.public_id(),
            expires_at,
        })
    }

    /// Complete exchange with scanned QR data.
    pub fn complete_exchange(&self, qr_data: String) -> Result<MobileExchangeResult, MobileError> {
        use vauchi_core::ExchangeQR;

        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let data_str = qr_data.strip_prefix("wb://").unwrap_or(&qr_data);
        let their_qr =
            ExchangeQR::from_data_string(data_str).map_err(|_| MobileError::InvalidQrCode)?;

        if their_qr.is_expired() {
            return Err(MobileError::ExchangeFailed("QR code expired".to_string()));
        }

        let their_signing_key = their_qr.public_key();
        let their_exchange_key = their_qr.exchange_key();
        let their_public_id = hex::encode(their_signing_key);

        if storage.load_contact(&their_public_id)?.is_some() {
            return Err(MobileError::ExchangeFailed(
                "Contact already exists".to_string(),
            ));
        }

        let our_x3dh = identity.x3dh_keypair();
        let (encrypted_msg, shared_secret) = EncryptedExchangeMessage::create(
            &our_x3dh,
            their_exchange_key,
            identity.signing_public_key(),
            identity.display_name(),
        )
        .map_err(|e| MobileError::ExchangeFailed(format!("Key agreement failed: {:?}", e)))?;

        let their_card = ContactCard::new("New Contact");
        let contact = Contact::from_exchange(*their_signing_key, their_card, shared_secret.clone());

        let contact_id = contact.id().to_string();
        let contact_name = contact.display_name().to_string();

        storage.save_contact(&contact)?;

        let ratchet = DoubleRatchetState::initialize_initiator(&shared_secret, *their_exchange_key);
        storage.save_ratchet_state(&contact_id, &ratchet, true)?;

        // Send encrypted exchange message
        {
            let mut socket = self.connect_to_relay()?;

            let our_id = identity.public_id();
            sync::send_handshake(&mut socket, &our_id, None)?;

            let update = protocol::EncryptedUpdate {
                recipient_id: their_public_id.clone(),
                sender_id: our_id,
                ciphertext: encrypted_msg.to_bytes(),
            };

            let envelope =
                protocol::create_envelope(protocol::MessagePayload::EncryptedUpdate(update));
            let data = protocol::encode_message(&envelope).map_err(MobileError::SyncFailed)?;
            socket
                .send(Message::Binary(data))
                .map_err(|e| MobileError::NetworkError(e.to_string()))?;

            std::thread::sleep(Duration::from_millis(100));
            let _ = socket.close(None);
        }

        Ok(MobileExchangeResult {
            contact_id,
            contact_name,
            success: true,
            error_message: None,
        })
    }

    // === Sync Operations ===

    /// Sync with relay server.
    pub fn sync(&self) -> Result<MobileSyncResult, MobileError> {
        *self.sync_status.lock().unwrap() = MobileSyncStatus::Syncing;

        let identity = self.get_identity()?;
        let storage = self.open_storage()?;
        let pinned_cert = self.get_pinned_cert();

        let result = sync::do_sync(&identity, &storage, &self.relay_url, pinned_cert.as_deref());

        match &result {
            Ok(_) => *self.sync_status.lock().unwrap() = MobileSyncStatus::Idle,
            Err(_) => *self.sync_status.lock().unwrap() = MobileSyncStatus::Error,
        }

        result
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

    // === Delivery Status Operations ===

    /// Get delivery record for a message.
    pub fn get_delivery_record(
        &self,
        message_id: String,
    ) -> Result<Option<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let record = storage.get_delivery_record(&message_id)?;
        Ok(record.as_ref().map(MobileDeliveryRecord::from))
    }

    /// Get all delivery records.
    pub fn get_all_delivery_records(&self) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_all_delivery_records()?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Get all delivery records for a recipient.
    pub fn get_delivery_records_for_contact(
        &self,
        recipient_id: String,
    ) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_delivery_records_for_recipient(&recipient_id)?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Count failed deliveries.
    pub fn count_failed_deliveries(&self) -> Result<u32, MobileError> {
        use vauchi_core::storage::DeliveryStatus;
        let storage = self.open_storage()?;
        let count = storage.count_deliveries_by_status(&DeliveryStatus::Failed {
            reason: String::new(),
        })?;
        Ok(count as u32)
    }

    /// Manually retry a failed delivery.
    ///
    /// Returns true if the retry entry was found and rescheduled.
    pub fn manual_retry(&self, message_id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        // Check if there's a retry entry for this message
        let entry = storage.get_retry_entry(&message_id)?;
        if entry.is_none() {
            return Ok(false);
        }

        // Reschedule for immediate retry
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        storage.update_retry_next_time(&message_id, now)?;
        Ok(true)
    }

    /// Get all pending (non-terminal) deliveries.
    pub fn get_pending_deliveries(&self) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_pending_deliveries()?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Get delivery count by status.
    pub fn get_delivery_count_by_status(
        &self,
        status: MobileDeliveryStatus,
    ) -> Result<u32, MobileError> {
        use vauchi_core::storage::DeliveryStatus;
        let core_status = match status {
            MobileDeliveryStatus::Queued => DeliveryStatus::Queued,
            MobileDeliveryStatus::Sent => DeliveryStatus::Sent,
            MobileDeliveryStatus::Stored => DeliveryStatus::Stored,
            MobileDeliveryStatus::Delivered => DeliveryStatus::Delivered,
            MobileDeliveryStatus::Expired => DeliveryStatus::Expired,
            MobileDeliveryStatus::Failed => DeliveryStatus::Failed {
                reason: String::new(),
            },
        };
        let storage = self.open_storage()?;
        let count = storage.count_deliveries_by_status(&core_status)?;
        Ok(count as u32)
    }

    // === Retry Queue Operations ===

    /// Get all retry entries that are due for retry.
    pub fn get_due_retries(&self) -> Result<Vec<MobileRetryEntry>, MobileError> {
        let storage = self.open_storage()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entries = storage.get_due_retries(now)?;
        Ok(entries.iter().map(MobileRetryEntry::from).collect())
    }

    /// Get all retry entries for a contact.
    pub fn get_retries_for_contact(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileRetryEntry>, MobileError> {
        let storage = self.open_storage()?;
        let entries = storage.get_retry_entries_for_recipient(&contact_id)?;
        Ok(entries.iter().map(MobileRetryEntry::from).collect())
    }

    /// Get the total count of retry entries.
    pub fn get_retry_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_retry_entries()?;
        Ok(count as u32)
    }

    /// Delete a retry entry (after successful delivery or max attempts).
    pub fn delete_retry(&self, message_id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let deleted = storage.delete_retry_entry(&message_id)?;
        Ok(deleted)
    }

    /// Calculate the backoff time for a given retry attempt.
    ///
    /// Returns seconds until next retry: 2^attempt, max 3600 (1 hour).
    pub fn calculate_retry_backoff(&self, attempt: u32) -> u64 {
        use vauchi_core::storage::RetryQueue;
        let queue = RetryQueue::new();
        queue.backoff_seconds(attempt)
    }

    // === Offline Queue Operations ===

    /// Get total count of all pending updates across all contacts.
    pub fn get_total_pending_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_all_pending_updates()?;
        Ok(count as u32)
    }

    /// Check if the offline queue is full.
    ///
    /// Default max size is 1000 updates.
    pub fn is_offline_queue_full(&self) -> Result<bool, MobileError> {
        use vauchi_core::storage::OfflineQueue;
        let storage = self.open_storage()?;
        let queue = OfflineQueue::new();
        queue
            .is_full(&storage)
            .map_err(|e| MobileError::StorageError(e.to_string()))
    }

    /// Get remaining capacity in the offline queue.
    pub fn get_offline_queue_capacity(&self) -> Result<u32, MobileError> {
        use vauchi_core::storage::OfflineQueue;
        let storage = self.open_storage()?;
        let queue = OfflineQueue::new();
        let remaining = queue
            .remaining_capacity(&storage)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(remaining as u32)
    }

    /// Clear all pending updates for a contact.
    ///
    /// Returns the number of cleared updates.
    pub fn clear_pending_updates_for_contact(
        &self,
        contact_id: String,
    ) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.delete_pending_updates_for_contact(&contact_id)?;
        Ok(count as u32)
    }

    // === Multi-Device Delivery Operations ===

    /// Get delivery summary for a message (X of Y devices delivered).
    pub fn get_delivery_summary(
        &self,
        message_id: String,
    ) -> Result<MobileDeliverySummary, MobileError> {
        let storage = self.open_storage()?;
        let summary = storage.get_delivery_summary(&message_id)?;
        Ok(MobileDeliverySummary::from(&summary))
    }

    /// Get all device delivery records for a message.
    pub fn get_device_deliveries(
        &self,
        message_id: String,
    ) -> Result<Vec<MobileDeviceDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_device_deliveries_for_message(&message_id)?;
        Ok(records
            .iter()
            .map(MobileDeviceDeliveryRecord::from)
            .collect())
    }

    /// Get all pending device deliveries.
    pub fn get_pending_device_deliveries(
        &self,
    ) -> Result<Vec<MobileDeviceDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_pending_device_deliveries()?;
        Ok(records
            .iter()
            .map(MobileDeviceDeliveryRecord::from)
            .collect())
    }

    // === Backup Operations ===

    /// Export encrypted backup.
    pub fn export_backup(&self, password: String) -> Result<String, MobileError> {
        let identity = self.get_identity()?;

        let backup = identity
            .export_backup(&password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

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

        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&backup_data)
            .map_err(|_| MobileError::InvalidInput("Invalid base64".to_string()))?;

        let backup = IdentityBackup::new(bytes);
        let identity = Identity::import_backup(&backup, &password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let internal_backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let internal_backup_data = internal_backup.as_bytes().to_vec();
        let display_name = identity.display_name().to_string();

        let storage = self.open_storage()?;
        storage.save_identity(&internal_backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data: internal_backup_data,
            display_name: display_name.clone(),
        };
        *self.identity_data.lock().unwrap() = Some(identity_data);

        if storage.load_own_card()?.is_none() {
            let card = ContactCard::new(&display_name);
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

    // === Recovery ===

    /// Create a recovery claim for a lost identity.
    ///
    /// The old_pk_hex is the hex-encoded public key of the lost identity.
    /// This starts the recovery process by creating a claim that contacts
    /// can vouch for.
    pub fn create_recovery_claim(
        &self,
        old_pk_hex: String,
    ) -> Result<MobileRecoveryClaim, MobileError> {
        use base64::Engine;
        let identity = self.get_identity()?;

        // Parse old public key
        let old_pk_bytes = hex::decode(&old_pk_hex)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid hex: {}", e)))?;
        let old_pk: [u8; 32] = old_pk_bytes
            .try_into()
            .map_err(|_| MobileError::InvalidInput("Public key must be 32 bytes".to_string()))?;

        // Create claim
        let new_pk = *identity.signing_public_key();
        let claim = RecoveryClaim::new(&old_pk, &new_pk);

        // Create proof to store vouchers and save to file
        let proof = RecoveryProof::new(&old_pk, &new_pk, 3); // Default threshold of 3
        std::fs::write(self.recovery_proof_path(), proof.to_bytes())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        // Encode claim for sharing
        let claim_data = base64::engine::general_purpose::STANDARD.encode(claim.to_bytes());

        Ok(MobileRecoveryClaim {
            old_public_key: old_pk_hex,
            new_public_key: hex::encode(new_pk),
            claim_data,
            is_expired: claim.is_expired(),
        })
    }

    /// Parse a recovery claim from base64.
    ///
    /// Used to inspect a claim before vouching for it.
    pub fn parse_recovery_claim(
        &self,
        claim_b64: String,
    ) -> Result<MobileRecoveryClaim, MobileError> {
        use base64::Engine;
        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(&claim_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let claim = RecoveryClaim::from_bytes(&claim_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid claim: {}", e)))?;

        Ok(MobileRecoveryClaim {
            old_public_key: hex::encode(claim.old_pk()),
            new_public_key: hex::encode(claim.new_pk()),
            claim_data: claim_b64,
            is_expired: claim.is_expired(),
        })
    }

    /// Create a voucher for someone's recovery claim.
    ///
    /// This vouches that you trust the person claiming to own the old identity
    /// is the same person as the new identity.
    pub fn create_recovery_voucher(
        &self,
        claim_b64: String,
    ) -> Result<MobileRecoveryVoucher, MobileError> {
        use base64::Engine;
        let identity = self.get_identity()?;

        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(&claim_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let claim = RecoveryClaim::from_bytes(&claim_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid claim: {}", e)))?;

        if claim.is_expired() {
            return Err(MobileError::InvalidInput("Claim has expired".to_string()));
        }

        let voucher = RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair())
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let voucher_data = base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes());

        Ok(MobileRecoveryVoucher {
            voucher_public_key: hex::encode(voucher.voucher_pk()),
            voucher_data,
        })
    }

    /// Add a voucher to the current recovery claim.
    ///
    /// Returns the updated progress.
    pub fn add_recovery_voucher(
        &self,
        voucher_b64: String,
    ) -> Result<MobileRecoveryProgress, MobileError> {
        use base64::Engine;
        let voucher_bytes = base64::engine::general_purpose::STANDARD
            .decode(&voucher_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let voucher = RecoveryVoucher::from_bytes(&voucher_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid voucher: {}", e)))?;

        if !voucher.verify() {
            return Err(MobileError::InvalidInput(
                "Invalid voucher signature".to_string(),
            ));
        }

        // Load current proof from file
        let proof_path = self.recovery_proof_path();
        let mut proof = if proof_path.exists() {
            let proof_bytes =
                std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;
            RecoveryProof::from_bytes(&proof_bytes)
                .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?
        } else {
            return Err(MobileError::InvalidInput(
                "No recovery in progress".to_string(),
            ));
        };

        // Add voucher
        proof
            .add_voucher(voucher)
            .map_err(|e| MobileError::InvalidInput(format!("Cannot add voucher: {}", e)))?;

        // Save updated proof
        std::fs::write(&proof_path, proof.to_bytes())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let is_complete = proof.voucher_count() >= proof.threshold() as usize;

        Ok(MobileRecoveryProgress {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            vouchers_collected: proof.voucher_count() as u32,
            vouchers_needed: proof.threshold(),
            is_complete,
        })
    }

    /// Get the current recovery progress.
    ///
    /// Returns None if no recovery is in progress.
    pub fn get_recovery_status(&self) -> Result<Option<MobileRecoveryProgress>, MobileError> {
        let proof_path = self.recovery_proof_path();

        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes =
            std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        let is_complete = proof.voucher_count() >= proof.threshold() as usize;

        Ok(Some(MobileRecoveryProgress {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            vouchers_collected: proof.voucher_count() as u32,
            vouchers_needed: proof.threshold(),
            is_complete,
        }))
    }

    /// Get the completed recovery proof as base64.
    ///
    /// Returns None if recovery is not complete.
    pub fn get_recovery_proof(&self) -> Result<Option<String>, MobileError> {
        use base64::Engine;
        let proof_path = self.recovery_proof_path();

        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes =
            std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        if proof.voucher_count() >= proof.threshold() as usize {
            let proof_data = base64::engine::general_purpose::STANDARD.encode(proof.to_bytes());
            Ok(Some(proof_data))
        } else {
            Ok(None)
        }
    }

    /// Verify a recovery proof from a contact.
    ///
    /// This checks if the proof is valid and provides a recommendation
    /// on whether to accept the recovered identity.
    pub fn verify_recovery_proof(
        &self,
        proof_b64: String,
    ) -> Result<MobileRecoveryVerification, MobileError> {
        use base64::Engine;
        let storage = self.open_storage()?;

        let proof_bytes = base64::engine::general_purpose::STANDARD
            .decode(&proof_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        // Validate the proof
        proof
            .validate()
            .map_err(|e| MobileError::InvalidInput(format!("Proof validation failed: {}", e)))?;

        // Count known vouchers (vouchers from our contacts)
        let contacts = storage
            .list_contacts()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let contact_pks: std::collections::HashSet<[u8; 32]> =
            contacts.iter().map(|c| *c.public_key()).collect();

        let known_voucher_count = proof
            .vouchers()
            .iter()
            .filter(|v| contact_pks.contains(v.voucher_pk()))
            .count();

        // Determine confidence
        let (confidence, recommendation) = if known_voucher_count >= 2 {
            (
                "high".to_string(),
                "Multiple contacts you know have vouched. Safe to accept.".to_string(),
            )
        } else if known_voucher_count == 1 {
            (
                "medium".to_string(),
                "One contact you know has vouched. Consider verifying in person.".to_string(),
            )
        } else {
            (
                "low".to_string(),
                "No known contacts have vouched. Verify identity carefully before accepting."
                    .to_string(),
            )
        };

        Ok(MobileRecoveryVerification {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            voucher_count: proof.voucher_count() as u32,
            known_vouchers: known_voucher_count as u32,
            confidence,
            recommendation,
        })
    }

    // === Field Validation Operations ===

    /// Validate a contact's field.
    ///
    /// Creates a cryptographically signed validation record attesting
    /// that you believe this field value belongs to this contact.
    /// Returns the created validation.
    pub fn validate_field(
        &self,
        contact_id: String,
        field_id: String,
        field_value: String,
    ) -> Result<MobileFieldValidation, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        // Check we're not validating our own field
        let my_id = hex::encode(identity.signing_public_key());
        if contact_id == my_id {
            return Err(MobileError::InvalidInput(
                "Cannot validate your own field".to_string(),
            ));
        }

        // Check we haven't already validated this field
        let validator_id = hex::encode(identity.signing_public_key());
        if storage.has_validated(&contact_id, &field_id, &validator_id)? {
            return Err(MobileError::InvalidInput(
                "You have already validated this field".to_string(),
            ));
        }

        // Create signed validation
        let validation = vauchi_core::social::ProfileValidation::create_signed(
            &identity,
            &field_id,
            &field_value,
            &contact_id,
        );

        // Store it
        storage.save_validation(&validation)?;

        Ok(MobileFieldValidation::from(&validation))
    }

    /// Get validation status for a contact's field.
    ///
    /// Returns aggregated validation information including count, trust level,
    /// and whether you have validated this field.
    pub fn get_field_validation_status(
        &self,
        contact_id: String,
        field_id: String,
        field_value: String,
    ) -> Result<MobileValidationStatus, MobileError> {
        let storage = self.open_storage()?;
        let validations = storage.load_validations_for_field(&contact_id, &field_id)?;

        // Get current user's ID if available
        let my_id = {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                match self.get_identity() {
                    Ok(identity) => Some(hex::encode(identity.signing_public_key())),
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        let blocked = std::collections::HashSet::new();
        let status = vauchi_core::social::ValidationStatus::from_validations(
            &validations,
            &field_value,
            my_id.as_deref(),
            &blocked,
        );

        Ok(MobileValidationStatus::from(&status))
    }

    /// Revoke your validation of a contact's field.
    ///
    /// Returns true if a validation was revoked, false if you hadn't validated.
    pub fn revoke_field_validation(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let deleted = storage.delete_validation(&contact_id, &field_id, &validator_id)?;

        Ok(deleted)
    }

    /// List all validations you have made.
    ///
    /// Returns a list of all fields you have validated, sorted by
    /// validation timestamp (most recent first).
    pub fn list_my_validations(&self) -> Result<Vec<MobileFieldValidation>, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validations = storage.load_validations_by_validator(&validator_id)?;

        Ok(validations
            .iter()
            .map(MobileFieldValidation::from)
            .collect())
    }

    /// Check if you have validated a specific field.
    pub fn has_validated_field(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validated = storage.has_validated(&contact_id, &field_id, &validator_id)?;

        Ok(validated)
    }

    /// Get the validation count for a field (quick check without full status).
    pub fn get_field_validation_count(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_validations_for_field(&contact_id, &field_id)?;
        Ok(count as u32)
    }

    // === Aha Moments (public API) ===

    /// Check if an aha moment has been seen.
    pub fn has_seen_aha_moment(&self, moment_type: MobileAhaMomentType) -> bool {
        let tracker = self.load_aha_tracker();
        tracker.has_seen(moment_type.into())
    }

    /// Try to trigger an aha moment. Returns the moment if not yet seen, None otherwise.
    pub fn try_trigger_aha_moment(
        &self,
        moment_type: MobileAhaMomentType,
    ) -> Result<Option<MobileAhaMoment>, MobileError> {
        let mut tracker = self.load_aha_tracker();
        let core_type: vauchi_core::AhaMomentType = moment_type.into();

        if let Some(moment) = tracker.try_trigger(core_type) {
            self.save_aha_tracker(&tracker)?;
            Ok(Some(MobileAhaMoment {
                moment_type,
                title: moment.title().to_string(),
                message: moment.message(),
                has_animation: moment.has_animation(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Try to trigger an aha moment with context (e.g., contact name).
    pub fn try_trigger_aha_moment_with_context(
        &self,
        moment_type: MobileAhaMomentType,
        context: String,
    ) -> Result<Option<MobileAhaMoment>, MobileError> {
        let mut tracker = self.load_aha_tracker();
        let core_type: vauchi_core::AhaMomentType = moment_type.into();

        if let Some(moment) = tracker.try_trigger_with_context(core_type, context) {
            self.save_aha_tracker(&tracker)?;
            Ok(Some(MobileAhaMoment {
                moment_type,
                title: moment.title().to_string(),
                message: moment.message(),
                has_animation: moment.has_animation(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the count of seen aha moments.
    pub fn aha_moments_seen_count(&self) -> u32 {
        let tracker = self.load_aha_tracker();
        tracker.seen_count() as u32
    }

    /// Get the total count of aha moments.
    pub fn aha_moments_total_count(&self) -> u32 {
        let tracker = self.load_aha_tracker();
        tracker.total_count() as u32
    }

    /// Reset all aha moments (for testing/debugging).
    pub fn reset_aha_moments(&self) -> Result<(), MobileError> {
        let mut tracker = self.load_aha_tracker();
        tracker.reset();
        self.save_aha_tracker(&tracker)
    }

    // === Demo Contact (public API) ===

    /// Initialize the demo contact if user has no real contacts.
    /// Call this after onboarding completes.
    pub fn init_demo_contact_if_needed(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        // Check if user has any real contacts
        let storage = self.open_storage()?;
        let contacts = storage
            .list_contacts()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        if !contacts.is_empty() {
            // User has contacts, don't show demo
            return Ok(None);
        }

        // Check current state
        let mut state = self.load_demo_state();
        if state.was_dismissed || state.auto_removed {
            // User dismissed or it was auto-removed
            return Ok(None);
        }

        // Activate demo contact if not already
        if !state.is_active {
            state = vauchi_core::DemoContactState::new_active();
            self.save_demo_state(&state)?;
        }

        // Get current demo card
        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Get the current demo contact if active.
    pub fn get_demo_contact(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let state = self.load_demo_state();
        if !state.is_active {
            return Ok(None);
        }

        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Get the demo contact state.
    pub fn get_demo_contact_state(&self) -> MobileDemoContactState {
        let state = self.load_demo_state();
        MobileDemoContactState {
            is_active: state.is_active,
            was_dismissed: state.was_dismissed,
            auto_removed: state.auto_removed,
            update_count: state.update_count,
        }
    }

    /// Check if a demo update is available.
    pub fn is_demo_update_available(&self) -> bool {
        let state = self.load_demo_state();
        state.is_update_due()
    }

    /// Trigger a demo update and get the new content.
    pub fn trigger_demo_update(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let mut state = self.load_demo_state();
        if !state.is_active {
            return Ok(None);
        }

        if let Some(tip) = state.advance_to_next_tip() {
            self.save_demo_state(&state)?;
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Dismiss the demo contact.
    pub fn dismiss_demo_contact(&self) -> Result<(), MobileError> {
        let mut state = self.load_demo_state();
        state.dismiss();
        self.save_demo_state(&state)
    }

    /// Auto-remove demo contact after first real exchange.
    /// Call this after a successful contact exchange.
    pub fn auto_remove_demo_contact(&self) -> Result<bool, MobileError> {
        let mut state = self.load_demo_state();
        if state.is_active {
            state.auto_remove();
            self.save_demo_state(&state)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Restore the demo contact from Settings.
    pub fn restore_demo_contact(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let mut state = self.load_demo_state();
        state.restore();
        self.save_demo_state(&state)?;

        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }
}

// INLINE_TEST_REQUIRED: Tests require tempfile for VauchiMobile instance creation
// and access to internal Arc<VauchiMobile> which cannot be accessed from external tests.
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_instance() -> (Arc<VauchiMobile>, TempDir) {
        let dir = TempDir::new().unwrap();
        let wb = VauchiMobile::new(
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
        assert!(
            exchange_data.qr_data.starts_with("wb://"),
            "QR data should start with wb://"
        );
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

        let backup = wb
            .export_backup("correct-horse-battery-staple".to_string())
            .unwrap();
        assert!(!backup.is_empty());

        let dir2 = TempDir::new().unwrap();
        let wb2 = VauchiMobile::new(
            dir2.path().to_string_lossy().to_string(),
            "ws://localhost:8080".to_string(),
        )
        .unwrap();

        wb2.import_backup(backup, "correct-horse-battery-staple".to_string())
            .unwrap();

        assert!(wb2.has_identity());
        let name = wb2.get_display_name().unwrap();
        assert_eq!(name, "Alice");
    }
}
