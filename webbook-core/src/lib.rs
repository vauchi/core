//! WebBook Core Library
//!
//! Privacy-focused contact card exchange library.
//! All cryptographic operations use the audited `ring` crate.

pub mod crypto;
pub mod identity;
pub mod contact_card;
pub mod exchange;

pub use crypto::{SigningKeyPair, PublicKey, Signature, SymmetricKey, encrypt, decrypt, ExchangeKeyPair};
pub use identity::{Identity, IdentityBackup};
pub use contact_card::{ContactCard, ContactField, FieldType, ValidationError};
pub use exchange::{ExchangeQR, X3DH, X3DHKeyPair, ExchangeError};
