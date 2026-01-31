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
//! - [`error`] - Error types for the API layer
//! - [`config`] - Configuration types
//! - [`events`] - Event system for callbacks
//! - [`contact_manager`] - High-level contact operations
//! - [`sync_controller`] - Sync and network orchestration
//! - [`vauchi`] - Main Vauchi orchestrator

#[cfg(feature = "testing")]
pub mod account;
#[cfg(not(feature = "testing"))]
mod account;

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
pub mod sync_controller;
#[cfg(not(feature = "testing"))]
mod sync_controller;

#[cfg(feature = "testing")]
pub mod vauchi;
#[cfg(not(feature = "testing"))]
mod vauchi;

// Error types
pub use error::{VauchiError, VauchiResult};

// Configuration
pub use config::{RelayConfig, SyncConfig, VauchiConfig};

// Events
pub use events::{CallbackHandler, EventDispatcher, EventHandler, VauchiEvent};

// Contact Manager
pub use contact_manager::ContactManager;

// Sync Controller
pub use sync_controller::{SyncController, SyncResult};

// Vauchi
pub use vauchi::{Vauchi, VauchiBuilder};
