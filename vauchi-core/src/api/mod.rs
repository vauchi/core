// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi API Layer
//!
//! High-level API for the Vauchi privacy-focused contact card exchange library.
//!
//! # Overview
//!
//! The API layer provides a clean, easy-to-use interface that coordinates:
//! - Identity management
//! - Contact card operations
//! - Contact management
//! - Synchronization and networking
//! - Event handling
//!
//! # Example
//!
//! ```ignore
//! use vauchi_core::api::{Vauchi, VauchiConfig};
//! use vauchi_core::contact_card::{ContactField, FieldType};
//!
//! // Create Vauchi with default configuration
//! let mut wb = Vauchi::new(VauchiConfig::default())?;
//!
//! // Create identity
//! wb.create_identity("Alice")?;
//!
//! // Update contact card
//! let mut card = wb.own_card()?.unwrap();
//! card.add_field(ContactField::new(FieldType::Email, "email", "alice@example.com"));
//! wb.update_own_card(&card)?;
//!
//! // List contacts
//! let contacts = wb.list_contacts()?;
//! println!("You have {} contacts", contacts.len());
//! ```
//!
//! # Module Structure
//!
//! - `error` - Error types for the API layer
//! - `config` - Configuration types
//! - `events` - Event system for callbacks
//! - `contact_manager` - High-level contact operations
//! - `sync_controller` - Sync and network orchestration
//! - `vauchi` - Main Vauchi orchestrator

pub mod app_password;
pub mod duress;
pub mod emergency;

#[cfg(feature = "testing")]
pub mod deletion;
#[cfg(not(feature = "testing"))]
mod deletion;

#[cfg(feature = "testing")]
pub mod config;
#[cfg(not(feature = "testing"))]
mod config;

#[cfg(feature = "testing")]
pub mod consent;
#[cfg(not(feature = "testing"))]
mod consent;

#[cfg(feature = "testing")]
pub mod contact_manager;
#[cfg(not(feature = "testing"))]
mod contact_manager;

#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;

#[cfg(feature = "testing")]
pub mod events;
#[cfg(not(feature = "testing"))]
mod events;

#[cfg(feature = "testing")]
pub mod gdpr;
#[cfg(not(feature = "testing"))]
mod gdpr;

#[cfg(feature = "testing")]
pub mod pre_signed;
#[cfg(not(feature = "testing"))]
mod pre_signed;

#[cfg(feature = "testing")]
pub mod shred;
#[cfg(not(feature = "testing"))]
mod shred;

#[cfg(feature = "testing")]
pub mod sync_controller;
#[cfg(not(feature = "testing"))]
mod sync_controller;

#[cfg(feature = "testing")]
pub mod vauchi;
#[cfg(not(feature = "testing"))]
mod vauchi;

// Identity Deletion
pub use deletion::{DeletionError, DeletionManager, DeletionResult, delete_identity_data};

// Consent
pub use consent::{ConsentManager, ConsentRecord, ConsentStatus, ConsentType};

// GDPR
pub use gdpr::{
    GDPR_EXPORT_VERSION, GDPR_SALT_LEN, GdprExport, export_all_data, export_encrypted,
    import_encrypted,
};

// Pre-signed shred messages
pub use pre_signed::{PreSignedError, PreSignedPurgeRequest, PreSignedShredMessages};

// Shred Manager
pub use shred::{
    PurgeSender, RevocationSender, ShredError, ShredManager, ShredReport, ShredToken,
    ShredVerification, WidgetConfirmationMode, widget_panic_shred,
};

// Error types
pub use error::{VauchiError, VauchiResult};

// Configuration
pub use config::{OhttpConfig, RecoveryConfig, RelayConfig, SyncConfig, VauchiConfig};

// Events
pub use events::{EventCallback, EventDispatcher, EventOrigin, HandlerId, VauchiEvent};

// Contact Manager
pub use contact_manager::ContactManager;

// Sync Controller
pub use sync_controller::{SyncController, SyncResult};

// App Password / Duress PIN
pub use app_password::{AppPasswordConfig, AuthResult};

// Duress Alert System
pub use duress::{DuressAlert, DuressAlertType};

// Emergency Broadcast System
pub use emergency::{BroadcastResult, EmergencyWipeStatus, MAX_TRUSTED_CONTACTS};

// Vauchi
pub use vauchi::{
    AuthMode, BIOMETRIC_UNLOCK_MIN_DURATION, BiometricUnlockOutcome, DeviceInfo, DeviceLinkResult,
    ExchangeQrData, ImportResult, ImportWarning, RecoveryReadiness, SetupProgress, Vauchi,
    VauchiBuilder, VauchiSyncOutcome,
};
#[cfg(feature = "network-http")]
pub use vauchi::{RelayExchangeOffer, RelayExchangeResult};
