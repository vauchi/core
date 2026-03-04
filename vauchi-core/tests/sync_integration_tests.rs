// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for sync integration: Merkle tree from contacts, compression wiring,
//! sync settings, wifi-only gate, identity key change detection.
//!
//! Feature tags: @sync @merkle @compression @settings @key-rotation

use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::sync::{CardDelta, MerkleTree};

// ============================================================================
// Merkle Tree From Contacts
// ============================================================================

/// Feature: sync_updates.feature @merkle
/// Scenario: Build Merkle tree from contact cards for efficient sync comparison
#[test]
fn test_merkle_tree_from_contacts_deterministic() {
    let mut card1 = ContactCard::new("Alice");
    card1
        .add_field(ContactField::new(
            FieldType::Email,
            "Work",
            "alice@test.com",
        ))
        .unwrap();

    let mut card2 = ContactCard::new("Bob");
    card2
        .add_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+15551234567",
        ))
        .unwrap();

    let tree1 = MerkleTree::from_contacts(&[&card1, &card2]);
    let tree2 = MerkleTree::from_contacts(&[&card1, &card2]);

    assert_eq!(
        tree1.root_hash(),
        tree2.root_hash(),
        "Same contacts should produce same root"
    );
    assert_eq!(tree1.leaves().len(), 2);
}

/// Feature: sync_updates.feature @merkle
/// Scenario: Different contact sets produce different Merkle roots
#[test]
fn test_merkle_tree_different_contacts_differ() {
    let card1 = ContactCard::new("Alice");
    let card2 = ContactCard::new("Bob");
    let card3 = ContactCard::new("Charlie");

    let tree_ab = MerkleTree::from_contacts(&[&card1, &card2]);
    let tree_ac = MerkleTree::from_contacts(&[&card1, &card3]);

    assert_ne!(tree_ab.root_hash(), tree_ac.root_hash());
}

/// Feature: sync_updates.feature @merkle
/// Scenario: from_contacts is sorted by public key for deterministic ordering
#[test]
fn test_merkle_tree_order_independent() {
    let card1 = ContactCard::new("Alice");
    let card2 = ContactCard::new("Bob");

    // Order shouldn't matter because from_contacts sorts by card content hash
    let tree1 = MerkleTree::from_contacts(&[&card1, &card2]);
    let tree2 = MerkleTree::from_contacts(&[&card2, &card1]);

    assert_eq!(
        tree1.root_hash(),
        tree2.root_hash(),
        "Order-independent: sorted by card hash"
    );
}

/// Feature: sync_updates.feature @merkle
/// Scenario: Diff identifies changed contacts
#[test]
fn test_merkle_diff_detects_contact_changes() {
    let card1 = ContactCard::new("Alice");
    let card2 = ContactCard::new("Bob");

    let tree1 = MerkleTree::from_contacts(&[&card1, &card2]);

    let mut card2_modified = card2.clone();
    card2_modified
        .add_field(ContactField::new(FieldType::Email, "Work", "bob@new.com"))
        .unwrap();

    let tree2 = MerkleTree::from_contacts(&[&card1, &card2_modified]);

    let diffs = tree1.diff(&tree2);
    assert!(!diffs.is_empty(), "Should detect changes");
}

// ============================================================================
// Compression Round-Trip
// ============================================================================

/// Feature: sync_updates.feature @compression
/// Scenario: Compress and decompress sync payload
#[test]
fn test_compression_roundtrip() {
    let payload = b"Hello, this is a test payload that should compress well. \
        AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    let compressed = CardDelta::compress_payload(payload);
    let decompressed = CardDelta::decompress_payload(&compressed).unwrap();

    assert_eq!(decompressed, payload);
    assert!(
        compressed.len() < payload.len(),
        "Compression should reduce size"
    );
}

/// Feature: sync_updates.feature @compression
/// Scenario: Empty payload compresses and decompresses
#[test]
fn test_compression_empty_payload() {
    let payload = b"";

    let compressed = CardDelta::compress_payload(payload);
    let decompressed = CardDelta::decompress_payload(&compressed).unwrap();

    assert_eq!(decompressed, payload);
}

// ============================================================================
// Sync Settings
// ============================================================================

/// Feature: sync_updates.feature @settings
/// Scenario: SyncConfig includes wifi-only and background sync settings
#[test]
fn test_sync_config_new_fields() {
    use vauchi_core::api::SyncConfig;

    let config = SyncConfig::default();

    // New fields should have sensible defaults
    assert!(!config.wifi_only_sync, "wifi_only_sync defaults to false");
    assert!(
        config.background_sync_enabled,
        "background_sync defaults to true"
    );
}

/// Feature: sync_updates.feature @settings
/// Scenario: SyncConfig can be customized
#[test]
fn test_sync_config_customization() {
    use vauchi_core::api::SyncConfig;

    let config = SyncConfig {
        wifi_only_sync: true,
        background_sync_enabled: false,
        ..Default::default()
    };

    assert!(config.wifi_only_sync);
    assert!(!config.background_sync_enabled);
}

// ============================================================================
// WiFi-Only Gate
// ============================================================================

/// Feature: sync_updates.feature @settings @wifi
/// Scenario: RuntimeStateProvider has is_on_wifi() convenience method
#[test]
fn test_connection_type_wifi_check() {
    use vauchi_core::capability::ConnectionType;

    assert!(ConnectionType::WiFi.is_wifi());
    assert!(!ConnectionType::Cellular.is_wifi());
    assert!(!ConnectionType::Ethernet.is_wifi());
    assert!(!ConnectionType::Offline.is_wifi());
}

// ============================================================================
// Identity Key Change Detection
// ============================================================================

/// Feature: sync_updates.feature @key-rotation
/// Scenario: Identity key change detected when incoming key differs from stored
#[test]
fn test_identity_key_change_detected() {
    use vauchi_core::sync::device_sync::detect_identity_key_change;

    let stored_key = [1u8; 32];
    let incoming_key = [2u8; 32];

    assert!(
        detect_identity_key_change(&stored_key, &incoming_key),
        "Different keys should trigger change detection"
    );
}

/// Feature: sync_updates.feature @key-rotation
/// Scenario: No change detected when keys match
#[test]
fn test_identity_key_no_change() {
    use vauchi_core::sync::device_sync::detect_identity_key_change;

    let key = [1u8; 32];

    assert!(
        !detect_identity_key_change(&key, &key),
        "Same keys should not trigger change"
    );
}
