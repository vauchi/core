// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact custom avatar CRUD operations.
//!
//! @scenario: contacts_management.feature - Upload custom avatar

use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

fn setup_with_contact() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();

    let mut pk = [0u8; 32];
    pk[0] = 1;
    let card = ContactCard::new("Bob");
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    (wb, contact_id)
}

/// Minimal valid WebP file: RIFF header + WEBP signature + VP8 chunk.
fn minimal_webp() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&18u32.to_le_bytes()); // file size - 8
    data.extend_from_slice(b"WEBP");
    data.extend_from_slice(b"VP8 ");
    data.extend_from_slice(&6u32.to_le_bytes()); // chunk size
    data.extend_from_slice(&[0x30, 0x01, 0x00, 0x9d, 0x01, 0x2a]); // minimal VP8 bitstream
    data
}

#[test]
fn test_set_and_get_custom_avatar_roundtrip() {
    let (wb, cid) = setup_with_contact();
    let webp = minimal_webp();
    wb.set_contact_custom_avatar(&cid, &webp).unwrap();
    let avatar = wb.get_contact_custom_avatar(&cid).unwrap();
    assert_eq!(avatar.as_deref(), Some(webp.as_slice()));
}

#[test]
fn test_get_avatar_returns_none_when_unset() {
    let (wb, cid) = setup_with_contact();
    let avatar = wb.get_contact_custom_avatar(&cid).unwrap();
    assert!(avatar.is_none(), "Unset avatar must return None");
}

#[test]
fn test_clear_custom_avatar() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_custom_avatar(&cid, &minimal_webp()).unwrap();
    wb.clear_contact_custom_avatar(&cid).unwrap();
    let avatar = wb.get_contact_custom_avatar(&cid).unwrap();
    assert!(avatar.is_none(), "Cleared avatar must return None");
}

#[test]
fn test_avatar_rejects_non_webp() {
    let (wb, cid) = setup_with_contact();
    let png_data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let result = wb.set_contact_custom_avatar(&cid, &png_data);
    assert!(result.is_err(), "Non-WebP data must be rejected");
}

#[test]
fn test_avatar_rejects_too_large() {
    let (wb, cid) = setup_with_contact();
    let mut big = minimal_webp();
    big.resize(32 * 1024 + 1, 0);
    let result = wb.set_contact_custom_avatar(&cid, &big);
    assert!(result.is_err(), "Avatar >32 KB must be rejected");
}

#[test]
fn test_avatar_rejects_empty() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_contact_custom_avatar(&cid, &[]);
    assert!(result.is_err(), "Empty avatar must be rejected");
}

#[test]
fn test_avatar_for_missing_contact_fails() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let result = wb.set_contact_custom_avatar("nonexistent", &minimal_webp());
    assert!(result.is_err(), "Avatar on nonexistent contact must fail");
}
