# API Design (Core Library)

```rust
// Identity Management
pub fn create_identity() -> Result<Identity, Error>;
pub fn export_identity_backup(password: &str) -> Result<EncryptedBackup, Error>;
pub fn import_identity_backup(backup: &EncryptedBackup, password: &str) -> Result<Identity, Error>;

// Contact Card Management
pub fn get_my_card() -> Result<ContactCard, Error>;
pub fn update_my_card(updates: CardUpdates) -> Result<ContactCard, Error>;
pub fn add_field(field: ContactField) -> Result<FieldId, Error>;
pub fn remove_field(field_id: &FieldId) -> Result<(), Error>;
pub fn set_field_visibility(field_id: &FieldId, rules: Vec<VisibilityRule>) -> Result<(), Error>;

// Contact Management
pub fn get_contacts() -> Result<Vec<Contact>, Error>;
pub fn get_contact(contact_id: &ContactId) -> Result<Contact, Error>;
pub fn remove_contact(contact_id: &ContactId) -> Result<(), Error>;
pub fn block_contact(contact_id: &ContactId) -> Result<(), Error>;

// Exchange
pub fn generate_exchange_qr() -> Result<QRData, Error>;
pub fn process_scanned_qr(qr_data: &QRData) -> Result<ExchangeSession, Error>;
pub fn complete_exchange(session: ExchangeSession) -> Result<Contact, Error>;

// Sync
pub fn start_sync_service() -> Result<SyncHandle, Error>;
pub fn stop_sync_service(handle: SyncHandle) -> Result<(), Error>;
pub fn get_sync_status() -> Result<SyncStatus, Error>;

// Device Management
pub fn link_device(qr_data: &QRData) -> Result<DeviceLink, Error>;
pub fn get_linked_devices() -> Result<Vec<Device>, Error>;
pub fn unlink_device(device_id: &DeviceId) -> Result<(), Error>;
```

## Mobile API (UniFFI)

The mobile API wraps the core library for iOS/Android:

- `MobileVauchi` - Main entry point
- `MobileContact` - Contact with card
- `MobileContactCard` - Card with fields
- `MobileExchangeData` - QR exchange data
- `MobileSyncResult` - Sync operation result

See [vauchi-mobile/README.md](../../vauchi-mobile/README.md) for details.
