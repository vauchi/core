//! Backend wrapper for vauchi-core

use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[cfg(feature = "secure-storage")]
use vauchi_core::storage::secure::{PlatformKeyring, SecureStorage};
use vauchi_core::{
    contact_card::ContactAction,
    crypto::ratchet::DoubleRatchetState,
    exchange::{EncryptedExchangeMessage, X3DHKeyPair},
    network::simple_message::{
        create_simple_ack, create_simple_envelope, decode_simple_message, encode_simple_message,
        LegacyExchangeMessage, SimpleAckStatus, SimpleEncryptedUpdate, SimpleHandshake,
        SimplePayload,
    },
    Contact, ContactCard, ContactField, ExchangeQR, FieldType, Identity, IdentityBackup, Storage,
    SymmetricKey,
};

#[cfg(not(feature = "secure-storage"))]
use vauchi_core::storage::secure::{FileKeyStorage, SecureStorage};

/// Internal password for local identity storage.
/// This is not for security - just for TUI persistence.
const LOCAL_STORAGE_PASSWORD: &str = "vauchi-local-storage";

/// Default relay URL.
const DEFAULT_RELAY_URL: &str = "wss://relay.vauchi.app";

/// Backend for Vauchi operations.
pub struct Backend {
    storage: Storage,
    identity: Option<Identity>,
    backup_data: Option<Vec<u8>>,
    display_name: Option<String>,
    relay_url: String,
    data_dir: std::path::PathBuf,
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

/// QR code data with expiration info.
#[derive(Debug, Clone)]
pub struct QRData {
    /// The QR code data string.
    pub data: String,
    /// Unix timestamp when the QR was generated.
    pub generated_at: u64,
    /// QR expiration time in seconds.
    pub expires_in_secs: u64,
}

impl QRData {
    /// Calculate remaining seconds until expiration.
    pub fn remaining_secs(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        let expires_at = self.generated_at + self.expires_in_secs;
        expires_at.saturating_sub(now)
    }

    /// Check if the QR code has expired.
    #[allow(dead_code)]
    pub fn is_expired(&self) -> bool {
        self.remaining_secs() == 0
    }
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
            let storage = PlatformKeyring::new("vauchi-tui");
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
                0x57, 0x65, 0x62, 0x42, 0x6f, 0x6f, 0x6b, 0x54, // "VauchiT"
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

        let db_path = data_dir.join("vauchi.db");

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

        // Load relay URL from config file or use default
        let relay_config_path = data_dir.join("relay_url.txt");
        let relay_url = std::fs::read_to_string(&relay_config_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string());

        Ok(Backend {
            storage,
            identity,
            backup_data,
            display_name,
            relay_url,
            data_dir: data_dir.to_path_buf(),
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

    /// Get the relay URL.
    pub fn relay_url(&self) -> &str {
        &self.relay_url
    }

    /// Set the relay URL.
    pub fn set_relay_url(&mut self, url: &str) -> Result<()> {
        let url = url.trim();
        if url.is_empty() {
            anyhow::bail!("Relay URL cannot be empty");
        }
        if !url.starts_with("wss://") && !url.starts_with("ws://") {
            anyhow::bail!("Relay URL must start with wss:// or ws://");
        }

        // Save to config file
        let relay_config_path = self.data_dir.join("relay_url.txt");
        std::fs::write(&relay_config_path, url).context("Failed to save relay URL")?;

        self.relay_url = url.to_string();
        Ok(())
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

    /// Update the display name.
    pub fn update_display_name(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("Display name cannot be empty");
        }
        if name.len() > 100 {
            anyhow::bail!("Display name cannot exceed 100 characters");
        }

        // Update identity
        if let Some(ref mut identity) = self.identity {
            identity.set_display_name(name);

            // Re-export backup with updated identity
            let backup = identity
                .export_backup(LOCAL_STORAGE_PASSWORD)
                .map_err(|e| anyhow::anyhow!("Failed to create backup: {:?}", e))?;
            let backup_data = backup.as_bytes().to_vec();
            self.storage.save_identity(&backup_data, name)?;
            self.backup_data = Some(backup_data);
        }

        // Update card display name
        let mut card = self.get_card()?.unwrap_or_else(|| ContactCard::new(name));
        card.set_display_name(name)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        self.storage.save_own_card(&card)?;

        self.display_name = Some(name.to_string());
        Ok(())
    }

    /// Update a field's value.
    pub fn update_field(&self, field_label: &str, new_value: &str) -> Result<()> {
        let mut card = self.get_card()?.context("No card found")?;

        // Find the field by label and get both ID and type
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .map(|f| (f.id().to_string(), f.field_type(), f.label().to_string()));

        if let Some((field_id, field_type, label)) = field {
            // Remove old field by ID and add new one with updated value
            card.remove_field(&field_id)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let new_field = ContactField::new(field_type, &label, new_value);
            card.add_field(new_field)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            self.storage.save_own_card(&card)?;
            Ok(())
        } else {
            anyhow::bail!("Field not found: {}", field_label)
        }
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

    /// Generate exchange QR data with expiration info.
    pub fn generate_exchange_qr(&self) -> Result<QRData> {
        let identity = self.identity.as_ref().context("No identity")?;

        // Generate actual exchange QR with X3DH
        let qr = ExchangeQR::generate(identity);

        Ok(QRData {
            data: qr.to_data_string(),
            generated_at: qr.timestamp(),
            expires_in_secs: 300, // 5 minutes, matching QR_EXPIRY_SECONDS in vauchi-core
        })
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

    /// Get sync status string for display.
    #[allow(dead_code)]
    pub fn sync_status(&self) -> &'static str {
        if self.identity.is_some() {
            "Ready to sync"
        } else {
            "No identity"
        }
    }

    /// Get count of pending outbound updates.
    pub fn pending_update_count(&self) -> Result<u32> {
        let contacts = self
            .storage
            .list_contacts()
            .context("Failed to list contacts")?;

        let mut total = 0u32;
        for contact in &contacts {
            let pending = self
                .storage
                .get_pending_updates(contact.id())
                .unwrap_or_default();
            total += pending.len() as u32;
        }
        Ok(total)
    }

    /// Perform a full sync with the relay server.
    ///
    /// This connects to the relay, receives pending messages, processes them,
    /// and sends any pending outbound updates.
    pub fn sync(&self) -> SyncResult {
        let identity = match &self.identity {
            Some(id) => id,
            None => return SyncResult::error("No identity found. Create an identity first."),
        };

        let relay_url = &self.relay_url;
        let client_id = identity.public_id();

        // Connect to relay
        let mut socket = match Self::connect_to_relay(relay_url) {
            Ok(s) => s,
            Err(e) => return SyncResult::error(format!("Connection failed: {}", e)),
        };

        // Send handshake
        if let Err(e) = Self::send_handshake(&mut socket, &client_id) {
            return SyncResult::error(format!("Handshake failed: {}", e));
        }

        // Wait for server to send pending messages
        std::thread::sleep(Duration::from_millis(500));

        // Receive pending messages
        let received = match Self::receive_pending(&mut socket) {
            Ok(msgs) => msgs,
            Err(e) => return SyncResult::error(format!("Receive failed: {}", e)),
        };

        // Process legacy exchange messages
        let legacy_added = match self.process_legacy_exchanges(identity, received.legacy_exchange) {
            Ok(count) => count,
            Err(e) => return SyncResult::error(format!("Legacy exchange failed: {}", e)),
        };

        // Process encrypted exchange messages
        let encrypted_added =
            match self.process_encrypted_exchanges(identity, received.encrypted_exchange) {
                Ok(count) => count,
                Err(e) => return SyncResult::error(format!("Encrypted exchange failed: {}", e)),
            };

        let contacts_added = legacy_added + encrypted_added;

        // Process card updates
        let cards_updated = match self.process_card_updates(received.card_updates) {
            Ok(count) => count,
            Err(e) => return SyncResult::error(format!("Card update failed: {}", e)),
        };

        // Send pending outbound updates
        let updates_sent = match self.send_pending_updates(identity, &mut socket) {
            Ok(count) => count,
            Err(e) => return SyncResult::error(format!("Send updates failed: {}", e)),
        };

        // Close connection
        let _ = socket.close(None);

        SyncResult::success(contacts_added, cards_updated, updates_sent)
    }

    /// Connect to relay server via WebSocket.
    fn connect_to_relay(relay_url: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
        let (socket, _response) =
            tungstenite::connect(relay_url).map_err(|e| format!("Failed to connect: {}", e))?;
        Ok(socket)
    }

    /// Send handshake to relay.
    fn send_handshake(
        socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
        client_id: &str,
    ) -> Result<(), String> {
        let handshake = SimpleHandshake {
            client_id: client_id.to_string(),
        };
        let envelope = create_simple_envelope(SimplePayload::Handshake(handshake));
        let data = encode_simple_message(&envelope).map_err(|e| format!("Encode error: {}", e))?;
        socket
            .send(Message::Binary(data))
            .map_err(|e| format!("Send error: {}", e))?;
        Ok(())
    }

    /// Receive pending messages from relay.
    fn receive_pending(
        socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    ) -> Result<ReceivedMessages, String> {
        let mut legacy_exchange = Vec::new();
        let mut encrypted_exchange = Vec::new();
        let mut card_updates = Vec::new();

        // Set read timeout
        if let MaybeTlsStream::Plain(ref stream) = socket.get_ref() {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
        }

        loop {
            match socket.read() {
                Ok(Message::Binary(data)) => {
                    if let Ok(envelope) = decode_simple_message(&data) {
                        if let SimplePayload::EncryptedUpdate(update) = envelope.payload {
                            // Classify the message
                            if LegacyExchangeMessage::is_exchange(&update.ciphertext) {
                                if let Some(exchange) =
                                    LegacyExchangeMessage::from_bytes(&update.ciphertext)
                                {
                                    legacy_exchange.push(exchange);
                                }
                            } else if EncryptedExchangeMessage::from_bytes(&update.ciphertext)
                                .is_ok()
                            {
                                encrypted_exchange.push(update.ciphertext);
                            } else {
                                card_updates.push((update.sender_id, update.ciphertext));
                            }

                            // Send acknowledgment
                            let ack = create_simple_ack(
                                &envelope.message_id,
                                SimpleAckStatus::ReceivedByRecipient,
                            );
                            if let Ok(ack_data) = encode_simple_message(&ack) {
                                let _ = socket.send(Message::Binary(ack_data));
                            }
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    let _ = socket.send(Message::Pong(data));
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => { /* Ignore other message types */ }
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(_) => break,
            }
        }

        Ok(ReceivedMessages {
            legacy_exchange,
            encrypted_exchange,
            card_updates,
        })
    }

    /// Parse a hex-encoded 32-byte key.
    fn parse_hex_key(hex_str: &str) -> Option<[u8; 32]> {
        let bytes = hex::decode(hex_str).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Some(arr)
    }

    /// Process legacy plaintext exchange messages.
    fn process_legacy_exchanges(
        &self,
        identity: &Identity,
        messages: Vec<LegacyExchangeMessage>,
    ) -> Result<u32, String> {
        let mut added = 0u32;
        let our_x3dh = identity.x3dh_keypair();

        for exchange in messages {
            let identity_key = match Self::parse_hex_key(&exchange.identity_public_key) {
                Some(key) => key,
                None => continue,
            };

            let public_id = hex::encode(identity_key);

            // Handle response (update contact name)
            if exchange.is_response {
                if let Ok(Some(mut contact)) = self.storage.load_contact(&public_id) {
                    if contact.display_name() != exchange.display_name
                        && contact.set_display_name(&exchange.display_name).is_ok()
                    {
                        let _ = self.storage.save_contact(&contact);
                    }
                }
                continue;
            }

            // Check if contact exists
            if self
                .storage
                .load_contact(&public_id)
                .map_err(|e| e.to_string())?
                .is_some()
            {
                continue;
            }

            let ephemeral_key = match Self::parse_hex_key(&exchange.ephemeral_public_key) {
                Some(key) => key,
                None => continue,
            };

            // Perform X3DH
            let shared_secret = match vauchi_core::exchange::X3DH::respond(
                &our_x3dh,
                &identity_key,
                &ephemeral_key,
            ) {
                Ok(secret) => secret,
                Err(_) => continue,
            };

            // Create contact
            let card = ContactCard::new(&exchange.display_name);
            let contact = Contact::from_exchange(identity_key, card, shared_secret.clone());
            let contact_id = contact.id().to_string();
            self.storage
                .save_contact(&contact)
                .map_err(|e| e.to_string())?;

            // Initialize ratchet
            let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
            let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
            let _ = self.storage.save_ratchet_state(&contact_id, &ratchet, true);

            added += 1;

            // Send response
            let _ = self.send_exchange_response(identity, &public_id, &ephemeral_key);
        }

        Ok(added)
    }

    /// Process encrypted exchange messages.
    fn process_encrypted_exchanges(
        &self,
        identity: &Identity,
        encrypted_data: Vec<Vec<u8>>,
    ) -> Result<u32, String> {
        let mut added = 0u32;
        let our_x3dh = identity.x3dh_keypair();

        for data in encrypted_data {
            let encrypted_msg = match EncryptedExchangeMessage::from_bytes(&data) {
                Ok(msg) => msg,
                Err(_) => continue,
            };

            let (payload, shared_secret) = match encrypted_msg.decrypt(&our_x3dh) {
                Ok(result) => result,
                Err(_) => continue,
            };

            let public_id = hex::encode(payload.identity_key);

            // Check if contact exists
            if self
                .storage
                .load_contact(&public_id)
                .map_err(|e| e.to_string())?
                .is_some()
            {
                continue;
            }

            // Create contact
            let card = ContactCard::new(&payload.display_name);
            let contact = Contact::from_exchange(payload.identity_key, card, shared_secret.clone());
            let contact_id = contact.id().to_string();
            self.storage
                .save_contact(&contact)
                .map_err(|e| e.to_string())?;

            // Initialize ratchet
            let ratchet_dh = X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
            let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
            let _ = self
                .storage
                .save_ratchet_state(&contact_id, &ratchet, false);

            added += 1;

            // Send response
            let _ = self.send_exchange_response(identity, &public_id, &payload.exchange_key);
        }

        Ok(added)
    }

    /// Send exchange response.
    fn send_exchange_response(
        &self,
        identity: &Identity,
        recipient_id: &str,
        recipient_exchange_key: &[u8; 32],
    ) -> Result<(), String> {
        let mut socket = Self::connect_to_relay(&self.relay_url)?;

        let our_id = identity.public_id();
        Self::send_handshake(&mut socket, &our_id)?;

        let our_x3dh = identity.x3dh_keypair();
        let (encrypted_msg, _) = EncryptedExchangeMessage::create(
            &our_x3dh,
            recipient_exchange_key,
            identity.signing_public_key(),
            identity.display_name(),
        )
        .map_err(|e| format!("Failed to encrypt exchange: {:?}", e))?;

        let update = SimpleEncryptedUpdate {
            recipient_id: recipient_id.to_string(),
            sender_id: our_id,
            ciphertext: encrypted_msg.to_bytes(),
        };

        let envelope = create_simple_envelope(SimplePayload::EncryptedUpdate(update));
        let data = encode_simple_message(&envelope).map_err(|e| e.to_string())?;
        socket
            .send(Message::Binary(data))
            .map_err(|e| e.to_string())?;

        std::thread::sleep(Duration::from_millis(100));
        let _ = socket.close(None);

        Ok(())
    }

    /// Process incoming card updates.
    fn process_card_updates(&self, updates: Vec<(String, Vec<u8>)>) -> Result<u32, String> {
        let mut processed = 0u32;

        for (sender_id, ciphertext) in updates {
            let mut contact = match self
                .storage
                .load_contact(&sender_id)
                .map_err(|e| e.to_string())?
            {
                Some(c) => c,
                None => continue,
            };

            let (mut ratchet, _) = match self
                .storage
                .load_ratchet_state(&sender_id)
                .map_err(|e| e.to_string())?
            {
                Some(state) => state,
                None => continue,
            };

            let ratchet_msg: vauchi_core::crypto::ratchet::RatchetMessage =
                match serde_json::from_slice(&ciphertext) {
                    Ok(msg) => msg,
                    Err(_) => continue,
                };

            let plaintext = match ratchet.decrypt(&ratchet_msg) {
                Ok(pt) => pt,
                Err(_) => continue,
            };

            if let Ok(delta) = serde_json::from_slice::<vauchi_core::sync::CardDelta>(&plaintext) {
                let mut card = contact.card().clone();
                if delta.apply(&mut card).is_ok() {
                    contact.update_card(card);
                    self.storage
                        .save_contact(&contact)
                        .map_err(|e| e.to_string())?;
                    processed += 1;
                }
            }

            let _ = self.storage.save_ratchet_state(&sender_id, &ratchet, false);
        }

        Ok(processed)
    }

    /// Send pending outbound updates.
    fn send_pending_updates(
        &self,
        identity: &Identity,
        socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    ) -> Result<u32, String> {
        let contacts = self.storage.list_contacts().map_err(|e| e.to_string())?;
        let our_id = identity.public_id();
        let mut sent = 0u32;

        for contact in contacts {
            let pending = self
                .storage
                .get_pending_updates(contact.id())
                .map_err(|e| e.to_string())?;

            for update in pending {
                let msg = SimpleEncryptedUpdate {
                    recipient_id: contact.id().to_string(),
                    sender_id: our_id.clone(),
                    ciphertext: update.payload,
                };

                let envelope = create_simple_envelope(SimplePayload::EncryptedUpdate(msg));
                if let Ok(data) = encode_simple_message(&envelope) {
                    if socket.send(Message::Binary(data)).is_ok() {
                        let _ = self.storage.delete_pending_update(&update.id);
                        sent += 1;
                    }
                }
            }
        }

        Ok(sent)
    }

    /// Test connection to the relay server.
    pub fn test_relay_connection(&self) -> Result<bool> {
        let mut socket = Self::connect_to_relay(&self.relay_url)
            .map_err(|e| anyhow::anyhow!("Connection failed: {}", e))?;

        // Close the connection gracefully
        let _ = socket.close(None);
        Ok(true)
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

/// Result of a sync operation.
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// Number of new contacts added from exchange messages.
    pub contacts_added: u32,
    /// Number of contact cards updated.
    pub cards_updated: u32,
    /// Number of outbound updates sent.
    pub updates_sent: u32,
    /// Whether sync completed successfully.
    pub success: bool,
    /// Error message if sync failed.
    pub error: Option<String>,
}

impl SyncResult {
    /// Create a success result.
    pub fn success(contacts_added: u32, cards_updated: u32, updates_sent: u32) -> Self {
        Self {
            contacts_added,
            cards_updated,
            updates_sent,
            success: true,
            error: None,
        }
    }

    /// Create an error result.
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            contacts_added: 0,
            cards_updated: 0,
            updates_sent: 0,
            success: false,
            error: Some(msg.into()),
        }
    }
}

/// Messages received from the relay during sync.
struct ReceivedMessages {
    legacy_exchange: Vec<LegacyExchangeMessage>,
    encrypted_exchange: Vec<Vec<u8>>,
    card_updates: Vec<(String, Vec<u8>)>,
}

// ===========================================================================
// Backend Tests
// Trace: features/identity_management.feature, contact_card_management.feature
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a test backend with isolated data directory.
    fn create_test_backend() -> (Backend, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let backend = Backend::new(temp_dir.path()).expect("Failed to create backend");
        (backend, temp_dir)
    }

    // === Identity Management Tests ===
    // Trace: identity_management.feature

    /// Trace: identity_management.feature - New backend has no identity
    #[test]
    fn test_new_backend_has_no_identity() {
        let (backend, _temp) = create_test_backend();
        assert!(!backend.has_identity());
        assert!(backend.display_name().is_none());
        assert!(backend.public_id().is_none());
    }

    /// Trace: identity_management.feature - Create new identity
    #[test]
    fn test_create_identity() {
        let (mut backend, _temp) = create_test_backend();

        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        assert!(backend.has_identity());
        assert_eq!(backend.display_name(), Some("Alice Smith"));
        assert!(backend.public_id().is_some());
    }

    /// Trace: identity_management.feature - Identity persists across backend instances
    #[test]
    fn test_identity_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Create identity in first backend
        {
            let mut backend = Backend::new(temp_dir.path()).expect("Failed to create backend");
            backend
                .create_identity("Alice Smith")
                .expect("Failed to create identity");
        }

        // Load in second backend
        {
            let backend = Backend::new(temp_dir.path()).expect("Failed to load backend");
            assert!(backend.has_identity());
            assert_eq!(backend.display_name(), Some("Alice Smith"));
        }
    }

    // === Contact Card Management Tests ===
    // Trace: contact_card_management.feature

    /// Trace: contact_card_management.feature - New identity has empty card
    #[test]
    fn test_new_identity_empty_card() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let fields = backend.get_card_fields().expect("Failed to get fields");
        assert!(fields.is_empty());
    }

    /// Trace: contact_card_management.feature - Add phone field
    #[test]
    fn test_add_phone_field() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        backend
            .add_field(FieldType::Phone, "Mobile", "+1-555-123-4567")
            .expect("Failed to add field");

        let fields = backend.get_card_fields().expect("Failed to get fields");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].label, "Mobile");
        assert_eq!(fields[0].value, "+1-555-123-4567");
    }

    /// Trace: contact_card_management.feature - Add email field
    #[test]
    fn test_add_email_field() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        backend
            .add_field(FieldType::Email, "Work", "alice@company.com")
            .expect("Failed to add field");

        let fields = backend.get_card_fields().expect("Failed to get fields");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].label, "Work");
        assert_eq!(fields[0].value, "alice@company.com");
    }

    /// Trace: contact_card_management.feature - Add multiple fields
    #[test]
    fn test_add_multiple_fields() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        backend
            .add_field(FieldType::Phone, "Mobile", "+1-555-123-4567")
            .expect("Failed to add field");
        backend
            .add_field(FieldType::Email, "Work", "alice@company.com")
            .expect("Failed to add field");
        backend
            .add_field(FieldType::Website, "Personal", "https://alice.example.com")
            .expect("Failed to add field");

        let fields = backend.get_card_fields().expect("Failed to get fields");
        assert_eq!(fields.len(), 3);
    }

    /// Trace: contact_card_management.feature - Remove field
    /// Note: Backend.remove_field takes field_id (unique ID), not label
    #[test]
    fn test_remove_field() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        backend
            .add_field(FieldType::Phone, "Mobile", "+1-555-123-4567")
            .expect("Failed to add field");

        // Get the card directly and get the field's unique ID
        let card = backend.get_card().expect("get card").unwrap();
        let field_id = card.fields()[0].id().to_string();
        backend
            .remove_field(&field_id)
            .expect("Failed to remove field");

        let fields = backend.get_card_fields().expect("Failed to get fields");
        assert!(fields.is_empty());
    }

    /// Trace: contact_card_management.feature - Update field value
    /// Note: Backend.update_field takes field label (finds field by label, then modifies)
    #[test]
    fn test_update_field() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        backend
            .add_field(FieldType::Phone, "Mobile", "+1-555-123-4567")
            .expect("Failed to add field");

        // update_field uses label to find and update the field
        backend
            .update_field("Mobile", "+1-555-999-8888")
            .expect("Failed to update field");

        let fields = backend.get_card_fields().expect("Failed to get fields");
        assert_eq!(fields[0].value, "+1-555-999-8888");
    }

    /// Trace: contact_card_management.feature - Update display name
    #[test]
    fn test_update_display_name() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        backend
            .update_display_name("Alice S.")
            .expect("Failed to update name");

        assert_eq!(backend.display_name(), Some("Alice S."));
    }

    /// Trace: contact_card_management.feature - Empty display name rejected
    #[test]
    fn test_empty_display_name_rejected() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let result = backend.update_display_name("");
        assert!(result.is_err());
        assert_eq!(backend.display_name(), Some("Alice Smith"));
    }

    /// Trace: contact_card_management.feature - Display name too long rejected
    #[test]
    fn test_long_display_name_rejected() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let long_name = "A".repeat(101);
        let result = backend.update_display_name(&long_name);
        assert!(result.is_err());
    }

    // === Field Type Parsing Tests ===

    #[test]
    fn test_parse_field_type_email() {
        assert!(matches!(
            Backend::parse_field_type("email"),
            FieldType::Email
        ));
        assert!(matches!(
            Backend::parse_field_type("EMAIL"),
            FieldType::Email
        ));
    }

    #[test]
    fn test_parse_field_type_phone() {
        assert!(matches!(
            Backend::parse_field_type("phone"),
            FieldType::Phone
        ));
    }

    #[test]
    fn test_parse_field_type_website() {
        assert!(matches!(
            Backend::parse_field_type("website"),
            FieldType::Website
        ));
    }

    #[test]
    fn test_parse_field_type_address() {
        assert!(matches!(
            Backend::parse_field_type("address"),
            FieldType::Address
        ));
    }

    #[test]
    fn test_parse_field_type_social() {
        assert!(matches!(
            Backend::parse_field_type("social"),
            FieldType::Social
        ));
    }

    #[test]
    fn test_parse_field_type_custom() {
        assert!(matches!(
            Backend::parse_field_type("other"),
            FieldType::Custom
        ));
        assert!(matches!(
            Backend::parse_field_type("unknown"),
            FieldType::Custom
        ));
    }

    // === Contacts Tests ===
    // Trace: contacts_management.feature

    /// Trace: contacts_management.feature - New identity has no contacts
    #[test]
    fn test_new_identity_no_contacts() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let contacts = backend.list_contacts().expect("Failed to list contacts");
        assert!(contacts.is_empty());
        assert_eq!(backend.contact_count().unwrap(), 0);
    }

    // === Settings Tests ===

    /// Test relay URL configuration
    #[test]
    fn test_relay_url_default() {
        let (backend, _temp) = create_test_backend();
        assert_eq!(backend.relay_url(), "wss://relay.vauchi.app");
    }

    /// Test setting relay URL
    #[test]
    fn test_set_relay_url() {
        let (mut backend, _temp) = create_test_backend();

        backend
            .set_relay_url("wss://custom.relay.example.com")
            .expect("Failed to set relay URL");

        assert_eq!(backend.relay_url(), "wss://custom.relay.example.com");
    }

    /// Test relay URL persistence
    #[test]
    fn test_relay_url_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        // Set relay URL in first backend
        {
            let mut backend = Backend::new(temp_dir.path()).expect("Failed to create backend");
            backend
                .set_relay_url("wss://custom.relay.example.com")
                .expect("Failed to set relay URL");
        }

        // Load in second backend
        {
            let backend = Backend::new(temp_dir.path()).expect("Failed to load backend");
            assert_eq!(backend.relay_url(), "wss://custom.relay.example.com");
        }
    }

    /// Test invalid relay URL rejected
    #[test]
    fn test_invalid_relay_url_rejected() {
        let (mut backend, _temp) = create_test_backend();

        let result = backend.set_relay_url("invalid-url");
        assert!(result.is_err());
    }

    /// Test empty relay URL rejected
    #[test]
    fn test_empty_relay_url_rejected() {
        let (mut backend, _temp) = create_test_backend();

        let result = backend.set_relay_url("");
        assert!(result.is_err());
    }

    // === Backup Tests ===
    // Trace: identity_management.feature - backup/restore

    /// Trace: identity_management.feature - Export backup
    #[test]
    fn test_export_backup() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        // Use a strong password that meets requirements
        let backup = backend
            .export_backup("Str0ng!P@ssw0rd#2024")
            .expect("Failed to export backup");

        // Backup should be hex-encoded
        assert!(hex::decode(&backup).is_ok());
        assert!(!backup.is_empty());
    }

    /// Trace: identity_management.feature - Import backup
    #[test]
    fn test_import_backup() {
        let backup_data;
        let password = "Str0ng!P@ssw0rd#2024";

        // Create identity and export backup
        {
            let (mut backend1, _temp1) = create_test_backend();
            backend1
                .create_identity("Alice Smith")
                .expect("Failed to create identity");
            backend1
                .add_field(FieldType::Email, "Work", "alice@work.com")
                .expect("Failed to add field");
            backup_data = backend1
                .export_backup(password)
                .expect("Failed to export backup");
        }

        // Import backup into new backend
        let (mut backend2, _temp2) = create_test_backend();
        backend2
            .import_backup(&backup_data, password)
            .expect("Failed to import backup");

        assert!(backend2.has_identity());
        assert_eq!(backend2.display_name(), Some("Alice Smith"));
    }

    /// Trace: identity_management.feature - Import with wrong password fails
    #[test]
    fn test_import_backup_wrong_password() {
        let (mut backend1, _temp1) = create_test_backend();
        backend1
            .create_identity("Alice Smith")
            .expect("Failed to create identity");
        let backup_data = backend1
            .export_backup("C0rrect!P@ssw0rd#2024")
            .expect("Failed to export backup");

        let (mut backend2, _temp2) = create_test_backend();
        let result = backend2.import_backup(&backup_data, "Wr0ng!P@ssw0rd#2024");

        assert!(result.is_err());
    }

    // === Exchange Tests ===
    // Trace: contact_exchange.feature

    /// Trace: contact_exchange.feature - Generate exchange QR
    #[test]
    fn test_generate_exchange_qr() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let qr = backend
            .generate_exchange_qr()
            .expect("Failed to generate QR");

        assert!(!qr.data.is_empty());
        assert!(qr.expires_in_secs > 0);
        assert!(qr.remaining_secs() <= qr.expires_in_secs);
    }

    // === Device Tests ===
    // Trace: device_management.feature

    /// Trace: device_management.feature - List devices shows current device
    #[test]
    fn test_list_devices() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let devices = backend.list_devices().expect("Failed to list devices");

        assert_eq!(devices.len(), 1);
        assert!(devices[0].is_current);
        assert!(devices[0].is_active);
    }

    /// Trace: device_management.feature - Generate device link
    #[test]
    fn test_generate_device_link() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let link = backend
            .generate_device_link()
            .expect("Failed to generate link");

        assert!(link.starts_with("wb://link/"));
    }

    // === Sync Tests ===
    // Trace: sync_updates.feature

    /// Trace: sync_updates.feature - Sync status without identity
    #[test]
    fn test_sync_status_no_identity() {
        let (backend, _temp) = create_test_backend();
        assert_eq!(backend.sync_status(), "No identity");
    }

    /// Trace: sync_updates.feature - Sync status with identity
    #[test]
    fn test_sync_status_with_identity() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");
        assert_eq!(backend.sync_status(), "Ready to sync");
    }

    /// Trace: sync_updates.feature - Pending update count starts at zero
    #[test]
    fn test_pending_update_count_zero() {
        let (mut backend, _temp) = create_test_backend();
        backend
            .create_identity("Alice Smith")
            .expect("Failed to create identity");

        let count = backend.pending_update_count().expect("Failed to get count");
        assert_eq!(count, 0);
    }

    // === QRData Tests ===

    #[test]
    fn test_qr_data_remaining_secs() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let qr = QRData {
            data: "test".to_string(),
            generated_at: now,
            expires_in_secs: 300,
        };

        // Should have close to 300 seconds remaining
        assert!(qr.remaining_secs() <= 300);
        assert!(qr.remaining_secs() >= 299);
    }

    #[test]
    fn test_qr_data_expired() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let qr = QRData {
            data: "test".to_string(),
            generated_at: now - 400, // 400 seconds ago
            expires_in_secs: 300,    // Expires after 300
        };

        assert_eq!(qr.remaining_secs(), 0);
        assert!(qr.is_expired());
    }

    // === SyncResult Tests ===

    #[test]
    fn test_sync_result_success() {
        let result = SyncResult::success(2, 3, 1);
        assert!(result.success);
        assert_eq!(result.contacts_added, 2);
        assert_eq!(result.cards_updated, 3);
        assert_eq!(result.updates_sent, 1);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_sync_result_error() {
        let result = SyncResult::error("Connection failed");
        assert!(!result.success);
        assert_eq!(result.contacts_added, 0);
        assert_eq!(result.error, Some("Connection failed".to_string()));
    }
}
