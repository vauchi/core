// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for shared avatar add/remove/list operations.
//!
//! @scenario: contacts_management.feature - Shared avatar management

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

/// Returns a tiny valid 1×1 red PNG as avatar data.
fn test_avatar_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Returns a second distinct avatar PNG (blue pixel).
fn test_avatar_png_b() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 255, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Computes the SHA-256 hex hash of avatar bytes, matching the API's internal logic.
fn avatar_hash(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

// @scenario: contacts_management.feature :: Sync delta adds and removes shared avatars
#[test]
fn test_add_and_list_shared_avatar() {
    let (wb, cid) = setup_with_contact();
    let data = test_avatar_png();
    wb.add_contact_shared_avatar(&cid, &data, false).unwrap();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert_eq!(avatars.len(), 1);
    assert_eq!(avatars[0].avatar_hash, avatar_hash(&data));
    assert!(!avatars[0].is_primary);
}

// @scenario: contacts_management.feature :: Select a shared avatar
#[test]
fn test_primary_avatar() {
    let (wb, cid) = setup_with_contact();
    let data = test_avatar_png();
    wb.add_contact_shared_avatar(&cid, &data, true).unwrap();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert_eq!(avatars.len(), 1);
    assert_eq!(avatars[0].avatar_hash, avatar_hash(&data));
    assert!(avatars[0].is_primary);
}

// @scenario: contacts_management.feature :: Sync delta adds and removes shared avatars
#[test]
fn test_multiple_avatars() {
    let (wb, cid) = setup_with_contact();
    let data_a = test_avatar_png();
    let data_b = test_avatar_png_b();
    wb.add_contact_shared_avatar(&cid, &data_a, true).unwrap();
    wb.add_contact_shared_avatar(&cid, &data_b, false).unwrap();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert_eq!(avatars.len(), 2, "Expected 2 shared avatars");
}

// @scenario: contacts_management.feature :: Sync delta adds and removes shared avatars
#[test]
fn test_remove_shared_avatar() {
    let (wb, cid) = setup_with_contact();
    let data_a = test_avatar_png();
    let data_b = test_avatar_png_b();
    wb.add_contact_shared_avatar(&cid, &data_a, true).unwrap();
    wb.add_contact_shared_avatar(&cid, &data_b, false).unwrap();
    wb.remove_contact_shared_avatar(&cid, &avatar_hash(&data_b))
        .unwrap();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert_eq!(avatars.len(), 1);
    assert_eq!(avatars[0].avatar_hash, avatar_hash(&data_a));
}

// @internal
#[test]
fn test_dedup_on_same_hash() {
    let (wb, cid) = setup_with_contact();
    let data = test_avatar_png();
    wb.add_contact_shared_avatar(&cid, &data, true).unwrap();
    wb.add_contact_shared_avatar(&cid, &data, false).unwrap();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert_eq!(
        avatars.len(),
        1,
        "Duplicate avatar data must be deduplicated"
    );
    assert_eq!(avatars[0].avatar_hash, avatar_hash(&data));
    assert!(
        !avatars[0].is_primary,
        "Second insert (is_primary=false) must win on conflict"
    );
}

// @internal
#[test]
fn test_empty_list() {
    let (wb, cid) = setup_with_contact();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert!(
        avatars.is_empty(),
        "No shared avatars added — list must be empty"
    );
}

// @internal
#[test]
fn test_primary_listed_first() {
    let (wb, cid) = setup_with_contact();
    let data_a = test_avatar_png();
    let data_b = test_avatar_png_b();
    wb.add_contact_shared_avatar(&cid, &data_a, false).unwrap();
    wb.add_contact_shared_avatar(&cid, &data_b, true).unwrap();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert!(!avatars.is_empty(), "Expected at least one shared avatar");
    assert!(
        avatars[0].is_primary,
        "Primary avatar must appear first; got hash: {:?}",
        avatars[0].avatar_hash
    );
}

// @internal
#[test]
fn test_is_primary_invariant() {
    // Adding avatar B as primary must demote avatar A to non-primary.
    let (wb, cid) = setup_with_contact();
    let data_a = test_avatar_png();
    let data_b = test_avatar_png_b();
    wb.add_contact_shared_avatar(&cid, &data_a, true).unwrap();
    wb.add_contact_shared_avatar(&cid, &data_b, true).unwrap();
    let avatars = wb.list_contact_shared_avatars(&cid).unwrap();
    assert_eq!(avatars.len(), 2, "Both avatars must be present");
    let hash_b = avatar_hash(&data_b);
    let hash_a = avatar_hash(&data_a);
    let b = avatars.iter().find(|a| a.avatar_hash == hash_b).unwrap();
    let a = avatars.iter().find(|a| a.avatar_hash == hash_a).unwrap();
    assert!(
        b.is_primary,
        "Avatar B must be primary after being inserted as primary"
    );
    assert!(
        !a.is_primary,
        "Avatar A must be demoted to non-primary when B becomes primary"
    );
}
