//! Backend wrapper for webbook-core

use std::path::Path;

use anyhow::{Context, Result};
use webbook_core::{
    ContactCard, ContactField, FieldType, Identity, IdentityBackup, Storage, SymmetricKey,
};

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
    /// Create a new backend.
    pub fn new(data_dir: &Path) -> Result<Self> {
        // Ensure data directory exists
        std::fs::create_dir_all(data_dir)
            .context("Failed to create data directory")?;

        let db_path = data_dir.join("webbook.db");

        // Generate or load encryption key
        let key_path = data_dir.join("storage.key");
        let key = if key_path.exists() {
            let key_bytes = std::fs::read(&key_path)
                .context("Failed to read storage key")?;
            if key_bytes.len() != 32 {
                anyhow::bail!("Invalid storage key length");
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&key_bytes);
            SymmetricKey::from_bytes(arr)
        } else {
            let key = SymmetricKey::generate();
            std::fs::write(&key_path, key.as_bytes())
                .context("Failed to write storage key")?;
            key
        };

        let storage = Storage::open(&db_path, key)
            .context("Failed to open storage")?;

        // Try to load existing identity
        let (identity, backup_data, display_name) = if let Ok(Some((backup, name))) = storage.load_identity() {
            let backup_obj = IdentityBackup::new(backup.clone());
            let identity = Identity::import_backup(&backup_obj, LOCAL_STORAGE_PASSWORD).ok();
            (identity, Some(backup), Some(name))
        } else {
            (None, None, None)
        };

        Ok(Backend { storage, identity, backup_data, display_name })
    }

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        self.identity.is_some() || self.backup_data.is_some()
    }

    /// Create a new identity.
    #[allow(dead_code)]
    pub fn create_identity(&mut self, name: &str) -> Result<()> {
        let identity = Identity::create(name);
        let backup = identity.export_backup(LOCAL_STORAGE_PASSWORD)
            .map_err(|e| anyhow::anyhow!("Failed to create backup: {:?}", e))?;
        let backup_data = backup.as_bytes().to_vec();

        self.storage.save_identity(&backup_data, name)
            .context("Failed to save identity")?;

        self.identity = Some(identity);
        self.backup_data = Some(backup_data);
        self.display_name = Some(name.to_string());
        Ok(())
    }

    /// Get the display name.
    pub fn display_name(&self) -> Option<&str> {
        self.identity.as_ref().map(|i| i.display_name())
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
        self.storage.load_own_card()
            .context("Failed to load own card")
    }

    /// Get card fields for display.
    pub fn get_card_fields(&self) -> Result<Vec<FieldInfo>> {
        let card = self.get_card()?;
        Ok(card.map(|c| {
            c.fields().iter().map(|f| FieldInfo {
                field_type: format!("{:?}", f.field_type()),
                label: f.label().to_string(),
                value: f.value().to_string(),
            }).collect()
        }).unwrap_or_default())
    }

    /// Add a field to the card.
    pub fn add_field(&self, field_type: FieldType, label: &str, value: &str) -> Result<()> {
        let mut card = self.get_card()?.unwrap_or_else(|| {
            ContactCard::new(self.display_name().unwrap_or("User"))
        });

        let field = ContactField::new(field_type, label, value);
        card.add_field(field)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        self.storage.save_own_card(&card)
            .context("Failed to save card")?;

        Ok(())
    }

    /// Remove a field from the card.
    pub fn remove_field(&self, field_id: &str) -> Result<()> {
        let mut card = self.get_card()?.context("No card found")?;
        card.remove_field(field_id)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        self.storage.save_own_card(&card)
            .context("Failed to save card")?;
        Ok(())
    }

    /// List all contacts.
    pub fn list_contacts(&self) -> Result<Vec<ContactInfo>> {
        let contacts = self.storage.list_contacts()
            .context("Failed to list contacts")?;

        Ok(contacts.into_iter().map(|c| ContactInfo {
            id: c.id().to_string(),
            display_name: c.display_name().to_string(),
            verified: c.is_fingerprint_verified(),
        }).collect())
    }

    /// Get contact count.
    pub fn contact_count(&self) -> Result<usize> {
        let contacts = self.storage.list_contacts()
            .context("Failed to list contacts")?;
        Ok(contacts.len())
    }

    /// Generate exchange QR data.
    pub fn generate_exchange_qr(&self) -> Result<String> {
        let identity = self.identity.as_ref().context("No identity")?;
        let card = self.get_card()?.unwrap_or_else(|| {
            ContactCard::new(identity.display_name())
        });

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
}

/// Available field types for selection.
pub const FIELD_TYPES: &[&str] = &["Email", "Phone", "Website", "Address", "Social", "Custom"];
