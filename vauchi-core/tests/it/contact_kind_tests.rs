// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ContactKind types (Exchanged vs Imported).

use vauchi_core::contact::VisibilityRules;
use vauchi_core::contact::kind::{ContactKind, ExchangedData, ImportSource, ImportedData};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::{ExchangeTransport, ProximityConfidence};

/// Helper: builds a minimal ExchangedData for testing.
fn test_exchanged_data() -> ExchangedData {
    ExchangedData::new_for_test(
        [0xAB; 32],
        SymmetricKey::from_bytes([1u8; 32]),
        1_700_000_000,
        ExchangeTransport::Qr,
        false,
        false,
        false,
        ProximityConfidence::Unknown,
        false,
        None,
        None,
        VisibilityRules::new(),
    )
}

/// Helper: builds a minimal ImportedData for testing.
fn test_imported_data() -> ImportedData {
    ImportedData {
        source: ImportSource::VcardFile,
        imported_at: 1_700_000_000,
        original_uid: None,
    }
}

// ========================================
// is_exchanged / is_imported
// ========================================

// @internal
#[test]
fn exchanged_kind_reports_is_exchanged_true() {
    let kind = ContactKind::Exchanged(test_exchanged_data());
    assert!(kind.is_exchanged());
    assert!(!kind.is_imported());
}

// @internal
#[test]
fn imported_kind_reports_is_imported_true() {
    let kind = ContactKind::Imported(test_imported_data());
    assert!(kind.is_imported());
    assert!(!kind.is_exchanged());
}

// ========================================
// ========================================

// @internal
#[test]
fn exchanged_data_accessor_returns_some_for_exchanged() {
    let kind = ContactKind::Exchanged(test_exchanged_data());
    let data = kind.exchanged_data();
    assert!(data.is_some());
    assert_eq!(*data.unwrap().public_key(), [0xAB; 32]);
}

// @internal
#[test]
fn exchanged_data_accessor_returns_none_for_imported() {
    let kind = ContactKind::Imported(test_imported_data());
    assert!(kind.exchanged_data().is_none());
}

// @internal
#[test]
fn exchanged_data_mut_accessor_returns_some_for_exchanged() {
    let mut kind = ContactKind::Exchanged(test_exchanged_data());
    let data = kind.exchanged_data_mut();
    assert!(data.is_some());
    data.unwrap().set_fingerprint_verified(true);

    // Verify the mutation stuck
    assert!(kind.exchanged_data().unwrap().fingerprint_verified());
}

// @internal
#[test]
fn exchanged_data_mut_accessor_returns_none_for_imported() {
    let mut kind = ContactKind::Imported(test_imported_data());
    assert!(kind.exchanged_data_mut().is_none());
}

// @internal
#[test]
fn imported_data_accessor_returns_some_for_imported() {
    let kind = ContactKind::Imported(test_imported_data());
    let data = kind.imported_data();
    assert!(data.is_some());
    assert_eq!(data.unwrap().source, ImportSource::VcardFile);
}

// @internal
#[test]
fn imported_data_accessor_returns_none_for_exchanged() {
    let kind = ContactKind::Exchanged(test_exchanged_data());
    assert!(kind.imported_data().is_none());
}

// ========================================
// ========================================

// @internal
#[test]
fn import_source_serde_roundtrip() {
    let sources = [
        ImportSource::VcardFile,
        ImportSource::CsvFile,
        ImportSource::IosPlatform,
        ImportSource::AndroidPlatform,
        ImportSource::Manual,
    ];

    for source in &sources {
        let json = serde_json::to_string(source).expect("serialize");
        let deserialized: ImportSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(&deserialized, source, "roundtrip failed for {json}");
    }
}

// ========================================
// ========================================

// @internal
#[test]
fn imported_data_preserves_original_uid() {
    let data = ImportedData {
        source: ImportSource::IosPlatform,
        imported_at: 1_700_000_000,
        original_uid: Some("ios-contact-abc123".to_string()),
    };
    let kind = ContactKind::Imported(data);
    let imported = kind.imported_data().unwrap();
    assert_eq!(imported.original_uid.as_deref(), Some("ios-contact-abc123"));
}
