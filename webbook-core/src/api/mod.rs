//! WebBook API Layer
//!
//! High-level API for the WebBook privacy-focused contact card exchange library.
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
//! use webbook_core::api::{WebBook, WebBookConfig};
//! use webbook_core::contact_card::{ContactField, FieldType};
//!
//! // Create WebBook with default configuration
//! let mut wb = WebBook::new(WebBookConfig::default())?;
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
//! - [`error`] - Error types for the API layer
//! - [`config`] - Configuration types
//! - [`events`] - Event system for callbacks
//! - [`contact_manager`] - High-level contact operations
//! - [`sync_controller`] - Sync and network orchestration
//! - [`webbook`] - Main WebBook orchestrator

#[cfg(feature = "testing")]
pub mod config;
#[cfg(not(feature = "testing"))]
mod config;

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
pub mod sync_controller;
#[cfg(not(feature = "testing"))]
mod sync_controller;

#[cfg(feature = "testing")]
pub mod webbook;
#[cfg(not(feature = "testing"))]
mod webbook;

// Error types
pub use error::{WebBookError, WebBookResult};

// Configuration
pub use config::{RelayConfig, SyncConfig, WebBookConfig};

// Events
pub use events::{CallbackHandler, EventDispatcher, EventHandler, WebBookEvent};

// Contact Manager
pub use contact_manager::ContactManager;

// Sync Controller
pub use sync_controller::{SyncController, SyncResult};

// WebBook
pub use webbook::{WebBook, WebBookBuilder};
