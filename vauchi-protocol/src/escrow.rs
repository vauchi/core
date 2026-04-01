// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Escrow protocol types for relay-mediated card exchange.
//!
//! These types define the wire format for the escrow endpoints used by
//! Link mode (and relay fallback). Clients encrypt cards locally and
//! deposit encrypted blobs via frontends; the relay stores them
//! without seeing plaintext (ADR-002, ADR-004).
//!
//! Hash fields are hex-encoded strings (64 hex chars = 32 bytes).
//! Blob fields are base64-encoded strings.

use serde::{Deserialize, Serialize};

// =========================================================================
// Constants
// =========================================================================

/// Maximum blob size in bytes (64 KiB).
pub const MAX_BLOB_BYTES: usize = 65_536;

/// Maximum TTL in seconds (7 days).
pub const MAX_TTL_SECONDS: u32 = 604_800;

/// Maximum slots per gate.
pub const MAX_SLOTS_PER_GATE: u8 = 2;

/// Expected length of a hex-encoded 32-byte hash.
pub const HASH_HEX_LENGTH: usize = 64;

// =========================================================================
// Messages (client → relay)
// =========================================================================

/// Escrow request from client to relay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action")]
pub enum EscrowMessage {
    /// Deposit an encrypted blob into a gate slot.
    Put {
        /// Hex-encoded 32-byte gate hash.
        gate_hash: String,
        /// Hex-encoded 32-byte slot hash.
        slot_hash: String,
        /// Base64-encoded encrypted blob.
        blob: String,
        /// Time-to-live in seconds (max 7 days).
        ttl_seconds: u32,
    },
    /// Retrieve a blob from a gate slot (requires both slots filled).
    Get {
        /// Hex-encoded 32-byte gate hash.
        gate_hash: String,
        /// Hex-encoded 32-byte slot hash identifying which blob to retrieve.
        slot_hash: String,
    },
    /// Query how many slots are filled in a gate.
    Count {
        /// Hex-encoded 32-byte gate hash.
        gate_hash: String,
    },
}

// =========================================================================
// Responses (relay → client)
// =========================================================================

/// Escrow response from relay to client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status")]
pub enum EscrowResponse {
    /// Blob stored successfully.
    Stored,
    /// Duplicate put — same gate+slot already has a blob.
    AlreadyExists,
    /// Gate already has the maximum number of slots filled.
    GateFull,
    /// Blob exceeds the 64 KiB limit.
    BlobTooLarge,
    /// Successfully retrieved blob.
    Blob {
        /// Base64-encoded encrypted blob.
        blob: String,
    },
    /// Gate exists but not all slots are filled yet.
    NotReady {
        /// Number of slots currently filled.
        count: u8,
    },
    /// Slot count for the requested gate.
    Count {
        /// Number of slots currently filled (0–2).
        count: u8,
    },
    /// Gate or slot not found (or expired).
    NotFound,
}

// =========================================================================
// Validation
// =========================================================================

/// Validation error for escrow messages.
#[derive(Debug, Clone, PartialEq)]
pub enum EscrowValidationError {
    /// Gate hash is not a valid 64-character hex string.
    InvalidGateHash,
    /// Slot hash is not a valid 64-character hex string.
    InvalidSlotHash,
    /// Blob exceeds MAX_BLOB_BYTES after base64 decoding.
    BlobTooLarge { size: usize },
    /// TTL exceeds MAX_TTL_SECONDS.
    TtlTooLong { ttl: u32 },
}

impl std::fmt::Display for EscrowValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGateHash => write!(f, "gate_hash must be 64 hex characters"),
            Self::InvalidSlotHash => write!(f, "slot_hash must be 64 hex characters"),
            Self::BlobTooLarge { size } => {
                write!(f, "blob is {size} bytes, max is {MAX_BLOB_BYTES}")
            }
            Self::TtlTooLong { ttl } => {
                write!(f, "ttl_seconds is {ttl}, max is {MAX_TTL_SECONDS}")
            }
        }
    }
}

impl std::error::Error for EscrowValidationError {}

fn is_valid_hex_hash(s: &str) -> bool {
    s.len() == HASH_HEX_LENGTH && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl EscrowMessage {
    /// Validate this message against escrow constraints.
    ///
    /// Returns all validation errors (not just the first).
    pub fn validate(&self) -> Result<(), Vec<EscrowValidationError>> {
        let mut errors = Vec::new();

        match self {
            EscrowMessage::Put {
                gate_hash,
                slot_hash,
                blob,
                ttl_seconds,
            } => {
                if !is_valid_hex_hash(gate_hash) {
                    errors.push(EscrowValidationError::InvalidGateHash);
                }
                if !is_valid_hex_hash(slot_hash) {
                    errors.push(EscrowValidationError::InvalidSlotHash);
                }
                // Estimate decoded blob size: base64 encodes 3 bytes as 4 chars.
                // Exact decoded size = ceil(len * 3 / 4) minus padding, but
                // a conservative upper bound is sufficient for rejection.
                let decoded_upper_bound = blob.len() * 3 / 4;
                if decoded_upper_bound > MAX_BLOB_BYTES {
                    errors.push(EscrowValidationError::BlobTooLarge {
                        size: decoded_upper_bound,
                    });
                }
                if *ttl_seconds > MAX_TTL_SECONDS {
                    errors.push(EscrowValidationError::TtlTooLong { ttl: *ttl_seconds });
                }
            }
            EscrowMessage::Get {
                gate_hash,
                slot_hash,
            } => {
                if !is_valid_hex_hash(gate_hash) {
                    errors.push(EscrowValidationError::InvalidGateHash);
                }
                if !is_valid_hex_hash(slot_hash) {
                    errors.push(EscrowValidationError::InvalidSlotHash);
                }
            }
            EscrowMessage::Count { gate_hash } => {
                if !is_valid_hex_hash(gate_hash) {
                    errors.push(EscrowValidationError::InvalidGateHash);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
