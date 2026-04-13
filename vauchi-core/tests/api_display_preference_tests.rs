// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for display name and avatar preference operations.
//!
//! @scenario: contacts_management.feature - Display preference chooser

use vauchi_core::{
    AvatarPreference, Contact, ContactCard, DisplayNamePreference, SymmetricKey, Vauchi,
    VauchiError,
};

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
    data.extend_from_slice(&18u32.to_le_bytes());
    data.extend_from_slice(b"WEBP");
    data.extend_from_slice(b"VP8 ");
    data.extend_from_slice(&6u32.to_le_bytes());
    data.extend_from_slice(&[0x30, 0x01, 0x00, 0x9d, 0x01, 0x2a]);
    data
}

// @internal
#[test]
fn test_default_preference_is_primary() {
    let (wb, cid) = setup_with_contact();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_name_preference, DisplayNamePreference::Primary);
    assert_eq!(opts.active_avatar_preference, AvatarPreference::Primary);
}

// @scenario: contacts_management.feature :: Choose between shared names and nickname
#[test]
fn test_set_name_preference_to_custom() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.set_display_name_preference(&cid, DisplayNamePreference::Custom)
        .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_name_preference, DisplayNamePreference::Custom);
}

// @internal
#[test]
fn test_custom_name_preference_without_nickname_fails() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_display_name_preference(&cid, DisplayNamePreference::Custom);
    assert!(
        matches!(result, Err(VauchiError::InvalidState(_))),
        "Setting Custom name pref without nickname must return InvalidState; got: {result:?}"
    );
}

// @internal
#[test]
fn test_shared_name_preference_without_name_fails() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_display_name_preference(
        &cid,
        DisplayNamePreference::SharedName {
            name: "Nonexistent".to_string(),
        },
    );
    assert!(
        matches!(result, Err(VauchiError::InvalidState(_))),
        "Setting SharedName pref for unknown name must return InvalidState; got: {result:?}"
    );
}

// @scenario: contacts_management.feature :: Select a shared name
#[test]
fn test_shared_name_preference_with_name_succeeds() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Bobby", false).unwrap();
    wb.set_display_name_preference(
        &cid,
        DisplayNamePreference::SharedName {
            name: "Bobby".to_string(),
        },
    )
    .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(
        opts.active_name_preference,
        DisplayNamePreference::SharedName {
            name: "Bobby".to_string()
        }
    );
}

// @scenario: contacts_management.feature :: Choose between shared names and nickname
#[test]
fn test_display_options_includes_shared_names_and_nickname() {
    let (wb, cid) = setup_with_contact();
    wb.add_contact_shared_name(&cid, "Bobby", true).unwrap();
    wb.add_contact_shared_name(&cid, "Rob", false).unwrap();
    wb.set_contact_nickname(&cid, "B-Man").unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    // 2 shared names + 1 nickname option
    assert_eq!(
        opts.names.len(),
        3,
        "Expected 2 shared names + 1 nickname; got {} options",
        opts.names.len()
    );
}

// @internal
#[test]
fn test_avatar_preference_independent_from_name() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.set_display_name_preference(&cid, DisplayNamePreference::Custom)
        .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_name_preference, DisplayNamePreference::Custom);
    assert_eq!(
        opts.active_avatar_preference,
        AvatarPreference::Primary,
        "Avatar preference must remain Primary when only name pref changes"
    );
}

// @internal
#[test]
fn test_custom_avatar_preference_without_avatar_fails() {
    let (wb, cid) = setup_with_contact();
    let result = wb.set_avatar_preference(&cid, AvatarPreference::Custom);
    assert!(
        matches!(result, Err(VauchiError::InvalidState(_))),
        "Setting Custom avatar pref without custom avatar must return InvalidState; got: {result:?}"
    );
}

// @scenario: contacts_management.feature :: Upload custom avatar
#[test]
fn test_custom_avatar_preference_with_avatar_succeeds() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_custom_avatar(&cid, &minimal_webp()).unwrap();
    wb.set_avatar_preference(&cid, AvatarPreference::Custom)
        .unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(opts.active_avatar_preference, AvatarPreference::Custom);
}

// @internal
#[test]
fn test_clear_nickname_resets_custom_preference() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_nickname(&cid, "Bobby").unwrap();
    wb.set_display_name_preference(&cid, DisplayNamePreference::Custom)
        .unwrap();
    wb.clear_contact_nickname(&cid).unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(
        opts.active_name_preference,
        DisplayNamePreference::Primary,
        "Clearing nickname with Custom pref must reset preference to Primary"
    );
}

// @internal
#[test]
fn test_clear_avatar_resets_custom_preference() {
    let (wb, cid) = setup_with_contact();
    wb.set_contact_custom_avatar(&cid, &minimal_webp()).unwrap();
    wb.set_avatar_preference(&cid, AvatarPreference::Custom)
        .unwrap();
    wb.clear_contact_custom_avatar(&cid).unwrap();
    let opts = wb.get_contact_display_options(&cid).unwrap();
    assert_eq!(
        opts.active_avatar_preference,
        AvatarPreference::Primary,
        "Clearing custom avatar with Custom pref must reset preference to Primary"
    );
}

// @internal
#[test]
fn test_display_options_always_shows_custom_avatar_option() {
    let (wb, cid) = setup_with_contact();
    // No custom avatar set — the Custom option must still appear with has_data=false
    let opts = wb.get_contact_display_options(&cid).unwrap();
    let custom_opt = opts
        .avatars
        .iter()
        .find(|a| a.source == AvatarPreference::Custom);
    assert!(
        custom_opt.is_some(),
        "Custom avatar option must always be present in display options"
    );
    assert!(
        !custom_opt.unwrap().has_data,
        "Custom avatar option must have has_data=false when no avatar is set"
    );
}

// @internal
#[test]
fn test_serde_roundtrip_display_name_preference() {
    let variants = [
        DisplayNamePreference::Primary,
        DisplayNamePreference::SharedName {
            name: "Bobby".to_string(),
        },
        DisplayNamePreference::Custom,
    ];
    for pref in &variants {
        let json = serde_json::to_string(pref).expect("serialization must succeed");
        let decoded: DisplayNamePreference =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(
            *pref, decoded,
            "Serde roundtrip must preserve variant {:?}",
            pref
        );
    }
}

// @internal
#[test]
fn test_serde_roundtrip_avatar_preference() {
    let variants = [
        AvatarPreference::Primary,
        AvatarPreference::SharedAvatar {
            hash: "abc123".to_string(),
        },
        AvatarPreference::Custom,
    ];
    for pref in &variants {
        let json = serde_json::to_string(pref).expect("serialization must succeed");
        let decoded: AvatarPreference =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(
            *pref, decoded,
            "Serde roundtrip must preserve variant {:?}",
            pref
        );
    }
}
