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

/// Create a tiny valid PNG for tests (1x1 red pixel).
fn test_avatar_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

// @scenario: contacts_management.feature :: Upload custom avatar
#[test]
fn test_set_and_get_custom_avatar_roundtrip() {
    let (wb, cid) = setup_with_contact();
    let png = test_avatar_png();
    wb.set_contact_custom_avatar(&cid, &png).unwrap();
    let avatar = wb.get_contact_custom_avatar(&cid).unwrap();
    let avatar = avatar.expect("avatar should be set");
    // Core normalizes to WebP (ADR-042)
    assert_eq!(&avatar[0..4], b"RIFF");
    assert_eq!(&avatar[8..12], b"WEBP");
    assert!(avatar.len() <= 32_768);
}

// @internal
#[test]
fn test_get_avatar_returns_none_when_unset() {
    let (wb, cid) = setup_with_contact();
    let avatar = wb.get_contact_custom_avatar(&cid).unwrap();
    assert!(avatar.is_none(), "Unset avatar must return None");
}

// @internal
#[test]
fn test_clear_custom_avatar() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_custom_avatar(&cid, &test_avatar_png())
        .unwrap();
    wb.clear_contact_custom_avatar(&cid).unwrap();
    let avatar = wb.get_contact_custom_avatar(&cid).unwrap();
    assert!(avatar.is_none(), "Cleared avatar must return None");
}

// @internal
#[test]
fn test_avatar_rejects_invalid_data() {
    let (wb, cid) = setup_with_contact();
    let garbage = vec![0x00, 0x01, 0x02, 0x03, 0xFF];
    let result = wb.set_contact_custom_avatar(&cid, &garbage);
    assert!(result.is_err(), "Invalid image data must be rejected");
}

// @internal
#[test]
fn test_avatar_accepts_jpeg() {
    let (wb, cid) = setup_with_contact();
    let img = image::RgbImage::from_pixel(1, 1, image::Rgb([0, 128, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
    let jpeg = buf.into_inner();
    wb.set_contact_custom_avatar(&cid, &jpeg).unwrap();
    let avatar = wb.get_contact_custom_avatar(&cid).unwrap().unwrap();
    assert_eq!(
        &avatar[0..4],
        b"RIFF",
        "JPEG input should be normalized to WebP"
    );
}

// @internal
#[test]
fn test_avatar_rejects_empty() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_contact_custom_avatar(&cid, &[]);
    assert!(result.is_err(), "Empty avatar must be rejected");
}

// @internal
#[test]
fn test_avatar_for_missing_contact_fails() {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let result = wb.set_contact_custom_avatar("nonexistent", &test_avatar_png());
    assert!(result.is_err(), "Avatar on nonexistent contact must fail");
}
