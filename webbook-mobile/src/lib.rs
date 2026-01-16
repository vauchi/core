//! WebBook Mobile Bindings
//!
//! UniFFI bindings for Android and iOS platforms.
//! Exposes a simplified, mobile-friendly API on top of webbook-core.
//!
//! Note: Storage connections are created on-demand for thread safety,
//! as rusqlite's Connection is not Sync.

use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use webbook_core::{
    Contact, ContactCard, ContactField, FieldType, Identity, IdentityBackup,
    SocialNetworkRegistry, Storage, SymmetricKey,
};
use webbook_core::crypto::ratchet::DoubleRatchetState;
use webbook_core::exchange::X3DH;

use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;

uniffi::setup_scaffolding!();

// === Sync Protocol ===

/// Wire protocol for relay communication (matches relay server expectations).
mod sync_protocol {
    use serde::{Deserialize, Serialize};

    pub const PROTOCOL_VERSION: u8 = 1;
    pub const FRAME_HEADER_SIZE: usize = 4;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MessageEnvelope {
        pub version: u8,
        pub message_id: String,
        pub timestamp: u64,
        pub payload: MessagePayload,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type")]
    pub enum MessagePayload {
        EncryptedUpdate(EncryptedUpdate),
        Acknowledgment(Acknowledgment),
        Handshake(Handshake),
        #[serde(other)]
        Unknown,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EncryptedUpdate {
        pub recipient_id: String,
        pub sender_id: String,
        pub ciphertext: Vec<u8>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Acknowledgment {
        pub message_id: String,
        pub status: AckStatus,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum AckStatus {
        Delivered,
        ReceivedByRecipient,
        #[allow(dead_code)]
        Failed,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Handshake {
        pub client_id: String,
    }

    /// Exchange message for contact exchange.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ExchangeMessage {
        pub msg_type: String,
        pub identity_public_key: String,
        pub ephemeral_public_key: String,
        pub display_name: String,
        #[serde(default)]
        pub is_response: bool,
    }

    impl ExchangeMessage {
        pub fn is_exchange(data: &[u8]) -> bool {
            if let Ok(msg) = serde_json::from_slice::<ExchangeMessage>(data) {
                msg.msg_type == "exchange"
            } else {
                false
            }
        }

        pub fn from_bytes(data: &[u8]) -> Option<Self> {
            serde_json::from_slice(data).ok()
        }

        pub fn new_response(identity_key: &[u8; 32], exchange_key: &[u8; 32], name: &str) -> Self {
            ExchangeMessage {
                msg_type: "exchange".to_string(),
                identity_public_key: hex::encode(identity_key),
                ephemeral_public_key: hex::encode(exchange_key),
                display_name: name.to_string(),
                is_response: true,
            }
        }

        pub fn to_bytes(&self) -> Vec<u8> {
            serde_json::to_vec(self).unwrap_or_default()
        }
    }

    pub fn create_envelope(payload: MessagePayload) -> MessageEnvelope {
        MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: uuid::Uuid::new_v4().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            payload,
        }
    }

    pub fn encode_message(envelope: &MessageEnvelope) -> Result<Vec<u8>, String> {
        let json = serde_json::to_vec(envelope).map_err(|e| e.to_string())?;
        let len = json.len() as u32;

        let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + json.len());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(&json);

        Ok(frame)
    }

    pub fn decode_message(data: &[u8]) -> Result<MessageEnvelope, String> {
        if data.len() < FRAME_HEADER_SIZE {
            return Err("Frame too short".to_string());
        }

        let json = &data[FRAME_HEADER_SIZE..];
        serde_json::from_slice(json).map_err(|e| e.to_string())
    }

    pub fn create_ack(message_id: &str, status: AckStatus) -> MessageEnvelope {
        create_envelope(MessagePayload::Acknowledgment(Acknowledgment {
            message_id: message_id.to_string(),
            status,
        }))
    }
}

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

/// Sync result with statistics.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSyncResult {
    /// Number of new contacts added from exchange messages.
    pub contacts_added: u32,
    /// Number of contact cards updated.
    pub cards_updated: u32,
    /// Number of outbound updates sent.
    pub updates_sent: u32,
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

    // === Internal Sync Methods ===

    /// Internal sync implementation.
    fn do_sync(&self) -> Result<MobileSyncResult, MobileError> {
        let identity = self.get_identity()?;
        let client_id = identity.public_id();

        // Connect to relay via WebSocket
        let (mut socket, _response) = connect(&self.relay_url)
            .map_err(|e| MobileError::NetworkError(format!("Connection failed: {}", e)))?;

        // Set read timeout for non-blocking receive
        if let MaybeTlsStream::Plain(ref stream) = socket.get_ref() {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
        }

        // Send handshake
        Self::send_handshake(&mut socket, &client_id)?;

        // Wait briefly for server to send pending messages
        std::thread::sleep(Duration::from_millis(500));

        // Receive and process pending messages
        let storage = self.open_storage()?;
        let (exchange_messages, card_updates) = Self::receive_pending(&mut socket)?;

        // Process exchange messages (creates new contacts)
        let contacts_added = self.process_exchange_messages(&identity, &storage, exchange_messages)?;

        // Process card updates from existing contacts
        let cards_updated = Self::process_card_updates(&storage, card_updates)?;

        // Send our pending outbound updates
        let updates_sent = Self::send_pending_updates(&identity, &storage, &mut socket)?;

        // Close connection
        let _ = socket.close(None);

        Ok(MobileSyncResult {
            contacts_added,
            cards_updated,
            updates_sent,
        })
    }

    /// Sends handshake to relay.
    fn send_handshake(
        socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
        client_id: &str,
    ) -> Result<(), MobileError> {
        use sync_protocol::*;

        let handshake = Handshake {
            client_id: client_id.to_string(),
        };
        let envelope = create_envelope(MessagePayload::Handshake(handshake));
        let data = encode_message(&envelope)
            .map_err(|e| MobileError::SyncFailed(format!("Encode error: {}", e)))?;
        socket.send(Message::Binary(data))
            .map_err(|e| MobileError::NetworkError(e.to_string()))?;
        Ok(())
    }

    /// Receives pending messages from relay.
    #[allow(clippy::type_complexity)]
    fn receive_pending(
        socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    ) -> Result<(Vec<sync_protocol::ExchangeMessage>, Vec<(String, Vec<u8>)>), MobileError> {
        use sync_protocol::*;

        let mut exchange_messages = Vec::new();
        let mut card_updates = Vec::new();

        loop {
            match socket.read() {
                Ok(Message::Binary(data)) => {
                    match decode_message(&data) {
                        Ok(envelope) => {
                            if let MessagePayload::EncryptedUpdate(update) = envelope.payload {
                                // Check if this is an exchange message
                                if ExchangeMessage::is_exchange(&update.ciphertext) {
                                    if let Some(exchange) = ExchangeMessage::from_bytes(&update.ciphertext) {
                                        exchange_messages.push(exchange);
                                    }
                                } else {
                                    // This is a card update
                                    card_updates.push((update.sender_id.clone(), update.ciphertext));
                                }

                                // Send acknowledgment
                                let ack = create_ack(&envelope.message_id, AckStatus::ReceivedByRecipient);
                                if let Ok(ack_data) = encode_message(&ack) {
                                    let _ = socket.send(Message::Binary(ack_data));
                                }
                            }
                        }
                        Err(_) => { /* Skip malformed messages */ }
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
                    // No more messages
                    break;
                }
                Err(_) => break,
            }
        }

        Ok((exchange_messages, card_updates))
    }

    /// Processes exchange messages and creates new contacts.
    fn process_exchange_messages(
        &self,
        identity: &Identity,
        storage: &Storage,
        messages: Vec<sync_protocol::ExchangeMessage>,
    ) -> Result<u32, MobileError> {
        let mut added = 0u32;
        let our_x3dh = identity.x3dh_keypair();

        for exchange in messages {
            // Parse identity key
            let identity_key = match hex::decode(&exchange.identity_public_key) {
                Ok(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    arr
                }
                _ => continue,
            };

            let public_id = hex::encode(identity_key);

            // Handle response to our exchange (update contact name)
            if exchange.is_response {
                if let Ok(Some(mut contact)) = storage.load_contact(&public_id) {
                    if contact.display_name() != exchange.display_name
                        && contact.set_display_name(&exchange.display_name).is_ok()
                    {
                        let _ = storage.save_contact(&contact);
                    }
                }
                continue;
            }

            // Check if contact already exists
            if storage.load_contact(&public_id)?.is_some() {
                continue;
            }

            // Parse ephemeral key
            let ephemeral_key = match hex::decode(&exchange.ephemeral_public_key) {
                Ok(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    arr
                }
                _ => continue,
            };

            // Perform X3DH as responder
            let shared_secret = match X3DH::respond(&our_x3dh, &identity_key, &ephemeral_key) {
                Ok(secret) => secret,
                Err(_) => continue,
            };

            // Create contact
            let card = ContactCard::new(&exchange.display_name);
            let contact = Contact::from_exchange(identity_key, card, shared_secret.clone());
            let contact_id = contact.id().to_string();

            // Save contact
            storage.save_contact(&contact)?;

            // Initialize ratchet as responder
            let ratchet_dh = webbook_core::exchange::X3DHKeyPair::from_bytes(our_x3dh.secret_bytes());
            let ratchet = DoubleRatchetState::initialize_responder(&shared_secret, ratchet_dh);
            let _ = storage.save_ratchet_state(&contact_id, &ratchet, true);

            added += 1;

            // Send exchange response with our name
            let _ = self.send_exchange_response(identity, &public_id);
        }

        Ok(added)
    }

    /// Sends exchange response with our name.
    fn send_exchange_response(
        &self,
        identity: &Identity,
        recipient_id: &str,
    ) -> Result<(), MobileError> {
        use sync_protocol::*;

        let (mut socket, _) = connect(&self.relay_url)
            .map_err(|e| MobileError::NetworkError(e.to_string()))?;

        let our_id = identity.public_id();
        Self::send_handshake(&mut socket, &our_id)?;

        let exchange_key_slice = identity.exchange_public_key();
        let exchange_key: [u8; 32] = exchange_key_slice.try_into()
            .map_err(|_| MobileError::CryptoError("Invalid key length".to_string()))?;

        let exchange_msg = ExchangeMessage::new_response(
            identity.signing_public_key(),
            &exchange_key,
            identity.display_name(),
        );

        let update = EncryptedUpdate {
            recipient_id: recipient_id.to_string(),
            sender_id: our_id,
            ciphertext: exchange_msg.to_bytes(),
        };

        let envelope = create_envelope(MessagePayload::EncryptedUpdate(update));
        let data = encode_message(&envelope)
            .map_err(MobileError::SyncFailed)?;
        socket.send(Message::Binary(data))
            .map_err(|e| MobileError::NetworkError(e.to_string()))?;

        std::thread::sleep(Duration::from_millis(100));
        let _ = socket.close(None);

        Ok(())
    }

    /// Processes incoming card updates.
    fn process_card_updates(
        storage: &Storage,
        updates: Vec<(String, Vec<u8>)>,
    ) -> Result<u32, MobileError> {
        let mut processed = 0u32;

        for (sender_id, ciphertext) in updates {
            // Get contact
            let mut contact = match storage.load_contact(&sender_id)? {
                Some(c) => c,
                None => continue,
            };

            // Get ratchet state
            let (mut ratchet, _is_initiator) = match storage.load_ratchet_state(&sender_id)? {
                Some(state) => state,
                None => continue,
            };

            // Try to parse as a RatchetMessage from JSON
            let ratchet_msg: webbook_core::crypto::ratchet::RatchetMessage =
                match serde_json::from_slice(&ciphertext) {
                    Ok(msg) => msg,
                    Err(_) => continue,
                };

            // Decrypt the card delta
            let plaintext = match ratchet.decrypt(&ratchet_msg) {
                Ok(pt) => pt,
                Err(_) => continue,
            };

            // Parse and apply delta
            if let Ok(delta) = serde_json::from_slice::<webbook_core::sync::CardDelta>(&plaintext) {
                let mut card = contact.card().clone();
                if delta.apply(&mut card).is_ok() {
                    // Update contact's card
                    contact.update_card(card);
                    storage.save_contact(&contact)?;
                    processed += 1;
                }
            }

            // Save updated ratchet state
            let _ = storage.save_ratchet_state(&sender_id, &ratchet, false);
        }

        Ok(processed)
    }

    /// Sends pending outbound updates.
    fn send_pending_updates(
        identity: &Identity,
        storage: &Storage,
        socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    ) -> Result<u32, MobileError> {
        use sync_protocol::*;

        let contacts = storage.list_contacts()?;
        let our_id = identity.public_id();
        let mut sent = 0u32;

        for contact in contacts {
            let pending = storage.get_pending_updates(contact.id())?;

            for update in pending {
                // Create encrypted update message
                let msg = EncryptedUpdate {
                    recipient_id: contact.id().to_string(),
                    sender_id: our_id.clone(),
                    ciphertext: update.payload,
                };

                let envelope = create_envelope(MessagePayload::EncryptedUpdate(msg));
                if let Ok(data) = encode_message(&envelope) {
                    if socket.send(Message::Binary(data)).is_ok() {
                        let _ = storage.delete_pending_update(&update.id);
                        sent += 1;
                    }
                }
            }
        }

        Ok(sent)
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
        let key_path = data_path.join("storage.key");

        // Load or generate storage key (must be consistent across sessions)
        let storage_key = if key_path.exists() {
            // Load existing key
            let key_bytes = std::fs::read(&key_path)
                .map_err(|e| MobileError::StorageError(format!("Failed to read key: {}", e)))?;
            let key_array: [u8; 32] = key_bytes.try_into()
                .map_err(|_| MobileError::StorageError("Invalid key length".to_string()))?;
            SymmetricKey::from_bytes(key_array)
        } else {
            // Generate and save new key
            let key = SymmetricKey::generate();
            std::fs::write(&key_path, key.as_bytes())
                .map_err(|e| MobileError::StorageError(format!("Failed to save key: {}", e)))?;
            key
        };

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
    /// This checks both in-memory cache and persistent storage.
    pub fn has_identity(&self) -> bool {
        // First check in-memory cache
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return true;
            }
        }

        // Check storage and load if found
        if let Ok(storage) = self.open_storage() {
            if let Ok(Some((backup_data, display_name))) = storage.load_identity() {
                // Load into memory cache
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

        // Store identity as backup data
        let backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let backup_data = backup.as_bytes().to_vec();

        // Persist identity to storage
        let storage = self.open_storage()?;
        storage.save_identity(&backup_data, &display_name)?;

        // Cache in memory
        let identity_data = IdentityData {
            backup_data,
            display_name: display_name.clone(),
        };
        *self.identity_data.lock().unwrap() = Some(identity_data);

        // Create initial contact card
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

    /// Sync with relay server.
    ///
    /// Connects to the configured relay, sends pending updates,
    /// and receives incoming updates from contacts.
    pub fn sync(&self) -> Result<MobileSyncResult, MobileError> {
        *self.sync_status.lock().unwrap() = MobileSyncStatus::Syncing;

        let result = self.do_sync();

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

        let internal_backup_data = internal_backup.as_bytes().to_vec();
        let display_name = identity.display_name().to_string();

        // Persist identity to storage
        let storage = self.open_storage()?;
        storage.save_identity(&internal_backup_data, &display_name)?;

        // Cache in memory
        let identity_data = IdentityData {
            backup_data: internal_backup_data,
            display_name: display_name.clone(),
        };
        *self.identity_data.lock().unwrap() = Some(identity_data);

        // Create contact card if it doesn't exist
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

        let backup = wb.export_backup("correct-horse-battery-staple".to_string()).unwrap();
        assert!(!backup.is_empty());

        // Create new instance and restore
        let dir2 = TempDir::new().unwrap();
        let wb2 = WebBookMobile::new(
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
