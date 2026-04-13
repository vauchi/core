// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for display preference resolution and validation.
//!
//! @scenario: contacts_management.feature - Choose between card variant names and nickname
//! @scenario: contacts_management.feature - Card update follows selected variant

use vauchi_core::{Contact, ContactCard, DisplayPreference, SymmetricKey, Vauchi};

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

/// Minimal valid WebP for avatar tests.
fn minimal_webp() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&18u32.to_le_bytes());
    data.extend_from_slice(b"WEBP");
    data.extend_from_slice(b"VP8 ");
    data.extend_from_slice(&6u32.to_le_bytes());
    data.extend_from_slice(&[0x30, 0x01, 0x00, 0x9d, 0x01, 0x2a]);
    data
}

#[test]
fn test_default_preference_is_card_default() {
    let (wb, cid) = setup_with_contact();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_name_preference, DisplayPreference::CardDefault);
    assert_eq!(
        opts.active_avatar_preference,
        DisplayPreference::CardDefault
    );
}

#[test]
fn test_set_name_preference_to_custom() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.set_display_name_preference(&cid, DisplayPreference::Custom)
        .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_name_preference, DisplayPreference::Custom);
}

#[test]
fn test_custom_preference_without_nickname_fails() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_display_name_preference(&cid, DisplayPreference::Custom);
    assert!(
        result.is_err(),
        "Custom preference without nickname must fail"
    );
}

#[test]
fn test_card_variant_preference_without_variant_fails() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_display_name_preference(
        &cid,
        DisplayPreference::CardVariant {
            source_label: "Work".to_string(),
        },
    );
    assert!(
        result.is_err(),
        "CardVariant without matching variant must fail"
    );
}

#[test]
fn test_card_variant_preference_with_variant_succeeds() {
    let (wb, cid) = setup_with_contact();
    wb.upsert_contact_name_variant(&cid, "Work", "Dr. Bob", None)
        .unwrap();
    wb.set_display_name_preference(
        &cid,
        DisplayPreference::CardVariant {
            source_label: "Work".to_string(),
        },
    )
    .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(
        opts.active_name_preference,
        DisplayPreference::CardVariant {
            source_label: "Work".to_string()
        }
    );
}

#[test]
fn test_display_options_includes_all_name_sources() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.upsert_contact_name_variant(&cid, "Work", "Dr. Bob", None)
        .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    // CardDefault ("Bob") + CardVariant/Work ("Dr. Bob") + Custom ("Bobby") = 3
    assert_eq!(
        opts.names.len(),
        3,
        "Expected 3 name options, got {}",
        opts.names.len()
    );
}

#[test]
fn test_avatar_preference_independent_from_name() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.set_display_name_preference(&cid, DisplayPreference::Custom)
        .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_name_preference, DisplayPreference::Custom);
    assert_eq!(
        opts.active_avatar_preference,
        DisplayPreference::CardDefault
    );
}

#[test]
fn test_set_avatar_preference_to_custom_without_avatar_fails() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_avatar_preference(&cid, DisplayPreference::Custom);
    assert!(
        result.is_err(),
        "Custom avatar preference without avatar must fail"
    );
}

#[test]
fn test_set_avatar_preference_to_custom_with_avatar_succeeds() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_custom_avatar(&cid, &minimal_webp()).unwrap();
    wb.set_avatar_preference(&cid, DisplayPreference::Custom)
        .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_avatar_preference, DisplayPreference::Custom);
}

#[test]
fn test_display_options_always_shows_custom_avatar_option() {
    let (wb, cid) = setup_with_contact();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    let custom_avatar = opts
        .avatars
        .iter()
        .find(|a| a.source == DisplayPreference::Custom);
    assert!(
        custom_avatar.is_some(),
        "Custom avatar option must always be present"
    );
    assert!(
        !custom_avatar.unwrap().has_data,
        "Custom avatar should report has_data=false when no avatar set"
    );
}
