//! Backend wrapper for webbook-core

use std::path::Path;

use anyhow::{Context, Result};
#[cfg(feature = "secure-storage")]
use webbook_core::storage::secure::{PlatformKeyring, SecureStorage};
use webbook_core::{
    contact_card::ContactAction, ContactCard, ContactField, FieldType, Identity, IdentityBackup,
    Storage, SymmetricKey,
};

#[cfg(not(feature = "secure-storage"))]
use webbook_core::storage::secure::{FileKeyStorage, SecureStorage};

/// Internal password for local identity storage.
/// This is not for security - just for TUI persistence.
const LOCAL_STORAGE_PASSWORD: &str = "webbook-local-storage";

/// Backend for WebBook operations.
pub struct Backend {
    storage: Storage,
    identity: Option<Identity>,
    backup_data: Option<Vec<u8>>,
    display_name: Option<String>,
}

/// Contact card field information for display.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub field_type: String,
    pub label: String,
    pub value: String,
}

/// Contact information for display.
#[derive(Debug, Clone)]
pub struct ContactInfo {
    pub id: String,
    pub display_name: String,
    pub verified: bool,
}

impl Backend {
    /// Loads or creates the storage encryption key using SecureStorage.
    ///
    /// When the `secure-storage` feature is enabled, uses the OS keychain.
    /// Otherwise, falls back to encrypted file storage.
    #[allow(unused_variables)]
    fn load_or_create_storage_key(data_dir: &Path) -> Result<SymmetricKey> {
        const KEY_NAME: &str = "storage_key";

        #[cfg(feature = "secure-storage")]
        {
            let storage = PlatformKeyring::new("webbook-tui");
            match storage.load_key(KEY_NAME) {
                Ok(Some(bytes)) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Ok(SymmetricKey::from_bytes(arr))
                }
                Ok(Some(_)) => {
                    anyhow::bail!("Invalid storage key length in keychain");
                }
                Ok(None) => {
                    // Generate and save new key
                    let key = SymmetricKey::generate();
                    storage
                        .save_key(KEY_NAME, key.as_bytes())
                        .map_err(|e| anyhow::anyhow!("Failed to save key to keychain: {}", e))?;
                    Ok(key)
                }
                Err(e) => {
                    anyhow::bail!("Keychain error: {}", e);
                }
            }
        }

        #[cfg(not(feature = "secure-storage"))]
        {
            // Fall back to encrypted file storage
            // Use a derived key for encrypting the storage key file
            // Note: This provides defense-in-depth, not strong security
            let fallback_key = SymmetricKey::from_bytes([
                0x57, 0x65, 0x62, 0x42, 0x6f, 0x6f, 0x6b, 0x54, // "WebBookT"
                0x55, 0x49, 0x53, 0x74, 0x6f, 0x72, 0x61, 0x67, // "UIStorag"
                0x65, 0x4b, 0x65, 0x79, 0x46, 0x61, 0x6c, 0x6c, // "eKeyFall"
                0x62, 0x61, 0x63, 0x6b, 0x56, 0x31, 0x00, 0x00, // "backV1\0\0"
            ]);

            let key_dir = data_dir.join("keys");
            let storage = FileKeyStorage::new(key_dir, fallback_key);

            match storage.load_key(KEY_NAME) {
                Ok(Some(bytes)) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Ok(SymmetricKey::from_bytes(arr))
                }
                Ok(Some(_)) => {
                    anyhow::bail!("Invalid storage key length");
                }
                Ok(None) => {
                    // Generate and save new key
                    let key = SymmetricKey::generate();
                    storage
                        .save_key(KEY_NAME, key.as_bytes())
                        .map_err(|e| anyhow::anyhow!("Failed to save storage key: {}", e))?;
                    Ok(key)
                }
                Err(e) => {
                    anyhow::bail!("Storage error: {}", e);
                }
            }
        }
    }

    /// Create a new backend.
    pub fn new(data_dir: &Path) -> Result<Self> {
        // Ensure data directory exists
        std::fs::create_dir_all(data_dir).context("Failed to create data directory")?;

        let db_path = data_dir.join("webbook.db");

        // Generate or load encryption key using SecureStorage
        let key = Self::load_or_create_storage_key(data_dir)?;

        let storage = Storage::open(&db_path, key).context("Failed to open storage")?;

        // Try to load existing identity
        let (identity, backup_data, display_name) =
            if let Ok(Some((backup, name))) = storage.load_identity() {
                let backup_obj = IdentityBackup::new(backup.clone());
                let identity = Identity::import_backup(&backup_obj, LOCAL_STORAGE_PASSWORD).ok();
                (identity, Some(backup), Some(name))
            } else {
                (None, None, None)
            };

        Ok(Backend {
            storage,
            identity,
            backup_data,
            display_name,
        })
    }

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        self.identity.is_some() || self.backup_data.is_some()
    }

    /// Create a new identity.
    #[allow(dead_code)]
    pub fn create_identity(&mut self, name: &str) -> Result<()> {
        let identity = Identity::create(name);
        let backup = identity
            .export_backup(LOCAL_STORAGE_PASSWORD)
            .map_err(|e| anyhow::anyhow!("Failed to create backup: {:?}", e))?;
        let backup_data = backup.as_bytes().to_vec();

        self.storage
            .save_identity(&backup_data, name)
            .context("Failed to save identity")?;

        self.identity = Some(identity);
        self.backup_data = Some(backup_data);
        self.display_name = Some(name.to_string());
        Ok(())
    }

    /// Get the display name.
    pub fn display_name(&self) -> Option<&str> {
        self.identity
            .as_ref()
            .map(|i| i.display_name())
            .or(self.display_name.as_deref())
    }

    /// Get the public ID (truncated).
    pub fn public_id(&self) -> Option<String> {
        self.identity.as_ref().map(|i| {
            let full = i.public_id();
            format!("{}...", &full[..16.min(full.len())])
        })
    }

    /// Get the own contact card.
    pub fn get_card(&self) -> Result<Option<ContactCard>> {
        self.storage
            .load_own_card()
            .context("Failed to load own card")
    }

    /// Get card fields for display.
    pub fn get_card_fields(&self) -> Result<Vec<FieldInfo>> {
        let card = self.get_card()?;
        Ok(card
            .map(|c| {
                c.fields()
                    .iter()
                    .map(|f| FieldInfo {
                        field_type: format!("{:?}", f.field_type()),
                        label: f.label().to_string(),
                        value: f.value().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Add a field to the card.
    pub fn add_field(&self, field_type: FieldType, label: &str, value: &str) -> Result<()> {
        let mut card = self
            .get_card()?
            .unwrap_or_else(|| ContactCard::new(self.display_name().unwrap_or("User")));

        let field = ContactField::new(field_type, label, value);
        card.add_field(field)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        self.storage
            .save_own_card(&card)
            .context("Failed to save card")?;

        Ok(())
    }

    /// Remove a field from the card.
    pub fn remove_field(&self, field_id: &str) -> Result<()> {
        let mut card = self.get_card()?.context("No card found")?;
        card.remove_field(field_id)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        self.storage
            .save_own_card(&card)
            .context("Failed to save card")?;
        Ok(())
    }

    /// List all contacts.
    pub fn list_contacts(&self) -> Result<Vec<ContactInfo>> {
        let contacts = self
            .storage
            .list_contacts()
            .context("Failed to list contacts")?;

        Ok(contacts
            .into_iter()
            .map(|c| ContactInfo {
                id: c.id().to_string(),
                display_name: c.display_name().to_string(),
                verified: c.is_fingerprint_verified(),
            })
            .collect())
    }

    /// Get contact count.
    pub fn contact_count(&self) -> Result<usize> {
        let contacts = self
            .storage
            .list_contacts()
            .context("Failed to list contacts")?;
        Ok(contacts.len())
    }

    /// Generate exchange QR data.
    pub fn generate_exchange_qr(&self) -> Result<String> {
        let identity = self.identity.as_ref().context("No identity")?;
        let card = self
            .get_card()?
            .unwrap_or_else(|| ContactCard::new(identity.display_name()));

        // Generate QR data (simplified - actual implementation uses X3DH)
        let public_id = identity.public_id();
        let display_name = card.display_name();

        Ok(format!("wb://{}?name={}", public_id, display_name))
    }

    /// Parse a field type string.
    pub fn parse_field_type(s: &str) -> FieldType {
        match s.to_lowercase().as_str() {
            "email" => FieldType::Email,
            "phone" => FieldType::Phone,
            "website" => FieldType::Website,
            "address" => FieldType::Address,
            "social" => FieldType::Social,
            _ => FieldType::Custom,
        }
    }

    // ========== Visibility Controls ==========

    /// Get a contact by index.
    pub fn get_contact_by_index(&self, index: usize) -> Result<Option<ContactInfo>> {
        let contacts = self.list_contacts()?;
        Ok(contacts.get(index).cloned())
    }

    /// Get visibility info for a contact (what fields they can see).
    pub fn get_contact_visibility(&self, contact_id: &str) -> Result<Vec<FieldVisibilityInfo>> {
        let contact = self
            .storage
            .load_contact(contact_id)
            .context("Failed to get contact")?
            .context("Contact not found")?;

        let card = self
            .get_card()?
            .unwrap_or_else(|| ContactCard::new(self.display_name().unwrap_or("User")));

        let rules = contact.visibility_rules();

        Ok(card
            .fields()
            .iter()
            .map(|field| {
                let can_see = rules.can_see(field.label(), contact_id);
                FieldVisibilityInfo {
                    field_label: field.label().to_string(),
                    can_see,
                }
            })
            .collect())
    }

    /// Toggle visibility of a field for a contact.
    pub fn toggle_field_visibility(&self, contact_id: &str, field_label: &str) -> Result<bool> {
        let mut contact = self
            .storage
            .load_contact(contact_id)
            .context("Failed to get contact")?
            .context("Contact not found")?;

        let current_can_see = contact.visibility_rules().can_see(field_label, contact_id);

        // Toggle: if currently visible, set to nobody; if hidden, set to everyone
        if current_can_see {
            contact.visibility_rules_mut().set_nobody(field_label);
        } else {
            contact.visibility_rules_mut().set_everyone(field_label);
        }

        let new_can_see = !current_can_see;

        self.storage
            .save_contact(&contact)
            .context("Failed to save contact")?;

        Ok(new_can_see)
    }

    /// Remove a contact by ID.
    pub fn remove_contact(&self, contact_id: &str) -> Result<()> {
        self.storage
            .delete_contact(contact_id)
            .context("Failed to delete contact")?;
        Ok(())
    }

    /// Get fields for a contact by index.
    pub fn get_contact_fields(&self, contact_index: usize) -> Result<Vec<ContactFieldInfo>> {
        let contacts = self
            .storage
            .list_contacts()
            .context("Failed to list contacts")?;

        let contact = contacts.get(contact_index).context("Contact not found")?;

        Ok(contact
            .card()
            .fields()
            .iter()
            .map(|f| {
                let action = f.to_action();
                let action_type = match &action {
                    ContactAction::Call(_) => "call",
                    ContactAction::SendSms(_) => "sms",
                    ContactAction::SendEmail(_) => "email",
                    ContactAction::OpenUrl(_) => "web",
                    ContactAction::OpenMap(_) => "map",
                    ContactAction::CopyToClipboard => "copy",
                };
                ContactFieldInfo {
                    label: f.label().to_string(),
                    value: f.value().to_string(),
                    field_type: format!("{:?}", f.field_type()),
                    action_type: action_type.to_string(),
                    uri: f.to_uri(),
                }
            })
            .collect())
    }

    /// Open a contact field in the system default app.
    pub fn open_contact_field(&self, contact_index: usize, field_index: usize) -> Result<String> {
        let fields = self.get_contact_fields(contact_index)?;
        let field = fields.get(field_index).context("Field not found")?;

        if let Some(ref uri) = field.uri {
            open::that(uri).context("Failed to open URI")?;
            Ok(format!("Opened {} in {}", field.label, field.action_type))
        } else {
            Ok(format!("No action available for {}", field.label))
        }
    }

    // ========== Device Management ==========

    /// List all linked devices.
    pub fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        // Try to load device registry from storage
        if let Ok(Some(registry)) = self.storage.load_device_registry() {
            Ok(registry
                .all_devices()
                .iter()
                .enumerate()
                .map(|(i, device)| {
                    DeviceInfo {
                        device_index: i as u32,
                        device_name: device.device_name.clone(),
                        public_key_prefix: hex::encode(&device.device_id[..8]),
                        is_current: i == 0, // First device is current for now
                        is_active: !device.revoked,
                    }
                })
                .collect())
        } else {
            // Return current device only
            let identity = self.identity.as_ref().context("No identity")?;
            Ok(vec![DeviceInfo {
                device_index: 0,
                device_name: "This Device".to_string(),
                public_key_prefix: hex::encode(&identity.device_id()[..8]),
                is_current: true,
                is_active: true,
            }])
        }
    }

    /// Generate device link data.
    pub fn generate_device_link(&self) -> Result<String> {
        let identity = self.identity.as_ref().context("No identity")?;
        // Generate a simplified link invitation
        let public_id = identity.public_id();
        Ok(format!(
            "wb://link/{}",
            &public_id[..32.min(public_id.len())]
        ))
    }

    // ========== Recovery ==========

    /// Get recovery status.
    pub fn get_recovery_status(&self) -> Result<RecoveryStatus> {
        // For now, return a stub status
        Ok(RecoveryStatus {
            has_active_claim: false,
            voucher_count: 0,
            required_vouchers: 3,
            claim_expires: None,
        })
    }

    // ========== Backup/Restore ==========

    /// Export identity backup with password.
    pub fn export_backup(&self, password: &str) -> Result<String> {
        let identity = self.identity.as_ref().context("No identity")?;
        let backup = identity
            .export_backup(password)
            .map_err(|e| anyhow::anyhow!("Export failed: {:?}", e))?;
        Ok(hex::encode(backup.as_bytes()))
    }

    /// Import identity from backup with password.
    pub fn import_backup(&mut self, backup_data: &str, password: &str) -> Result<()> {
        let bytes = hex::decode(backup_data.trim()).context("Invalid hex data")?;
        let backup = IdentityBackup::new(bytes.clone());
        let identity = Identity::import_backup(&backup, password)
            .map_err(|e| anyhow::anyhow!("Import failed: {:?}", e))?;

        let name = identity.display_name().to_string();
        self.storage
            .save_identity(&bytes, &name)
            .context("Failed to save identity")?;

        self.identity = Some(identity);
        self.backup_data = Some(bytes);
        self.display_name = Some(name);
        Ok(())
    }

    /// Perform sync (placeholder - actual sync requires async runtime).
    pub fn sync_status(&self) -> &'static str {
        if self.identity.is_some() {
            "Ready to sync"
        } else {
            "No identity"
        }
    }
}

/// Field visibility information for display.
#[derive(Debug, Clone)]
pub struct FieldVisibilityInfo {
    pub field_label: String,
    pub can_see: bool,
}

/// Contact field information for display.
#[derive(Debug, Clone)]
pub struct ContactFieldInfo {
    pub label: String,
    pub value: String,
    #[allow(dead_code)]
    pub field_type: String,
    pub action_type: String,
    pub uri: Option<String>,
}

/// Device information for display.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DeviceInfo {
    pub device_index: u32,
    pub device_name: String,
    pub public_key_prefix: String,
    pub is_current: bool,
    pub is_active: bool,
}

/// Recovery status information.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RecoveryStatus {
    pub has_active_claim: bool,
    pub voucher_count: u32,
    pub required_vouchers: u32,
    pub claim_expires: Option<String>,
}

/// Available field types for selection.
pub const FIELD_TYPES: &[&str] = &["Email", "Phone", "Website", "Address", "Social", "Custom"];
