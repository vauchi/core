// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact favorites and personal notes.
//!
//! Covers Gherkin scenarios from contacts_management.feature:
//! - Mark contact as favorite
//! - Remove favorite
//! - Favorites appear first in list
//! - Add personal note to contact
//! - Edit contact note
//! - Delete contact note
//! - Notes are not shared with contact

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::*;

fn create_test_contact_with_name(name: &str, key_byte: u8) -> Contact {
    let public_key = [key_byte; 32];
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(public_key, card, shared_key)
}

fn create_test_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).unwrap()
}

// ============================================================
// Favorites Tests
// ============================================================

// @scenario: contacts_management.feature:Mark contact as favorite
#[test]
fn test_contact_set_favorite_marks_contact() {
    let mut contact = create_test_contact_with_name("Bob", 0x01);

    // Default should be not favorite
    assert!(
        !contact.is_favorite(),
        "New contact should not be favorite by default"
    );

    // Mark as favorite
    contact.set_favorite(true);
    assert!(
        contact.is_favorite(),
        "Contact should be favorite after set_favorite(true)"
    );
}

// @scenario: contacts_management.feature:Remove favorite
#[test]
fn test_contact_remove_favorite_unmarks_contact() {
    let mut contact = create_test_contact_with_name("Bob", 0x01);

    // Mark as favorite first
    contact.set_favorite(true);
    assert!(contact.is_favorite());

    // Remove favorite
    contact.set_favorite(false);
    assert!(
        !contact.is_favorite(),
        "Contact should not be favorite after set_favorite(false)"
    );
}

// @scenario: contacts_management.feature:Favorites appear first in list
#[test]
fn test_contacts_sorted_favorites_first() {
    // Alice (not favorite) and Bob (favorite)
    let mut alice = create_test_contact_with_name("Alice", 0x01);
    let mut bob = create_test_contact_with_name("Bob", 0x02);

    alice.set_favorite(false);
    bob.set_favorite(true);

    // effective_sort_key should put favorites first
    // Favorites should have a sort key that comes before non-favorites
    let alice_key = alice.effective_sort_key();
    let bob_key = bob.effective_sort_key();

    assert!(
        bob_key < alice_key,
        "Favorite Bob's sort key ({}) should come before non-favorite Alice's ({})",
        bob_key,
        alice_key
    );
}

// @scenario: contacts_management.feature:Mark contact as favorite (storage round-trip)
#[test]
fn test_contact_favorite_persists_in_storage() {
    let storage = create_test_storage();
    let mut contact = create_test_contact_with_name("Bob", 0x01);
    let contact_id = contact.id().to_string();

    // Mark as favorite and save
    contact.set_favorite(true);
    storage.save_contact(&contact).unwrap();

    // Load back and verify
    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(
        loaded.is_favorite(),
        "Favorite status should persist through storage round-trip"
    );
}

// @scenario: contacts_management.feature:Remove favorite (storage round-trip)
#[test]
fn test_contact_remove_favorite_persists_in_storage() {
    let storage = create_test_storage();
    let mut contact = create_test_contact_with_name("Bob", 0x01);
    let contact_id = contact.id().to_string();

    // Mark as favorite, save
    contact.set_favorite(true);
    storage.save_contact(&contact).unwrap();

    // Remove favorite, save again
    contact.set_favorite(false);
    storage.save_contact(&contact).unwrap();

    // Load back
    let loaded = storage.load_contact(&contact_id).unwrap().unwrap();
    assert!(
        !loaded.is_favorite(),
        "Non-favorite status should persist through storage round-trip"
    );
}

// @scenario: Favorites are independent of hidden/blocked
#[test]
fn test_contact_favorite_independent_of_other_flags() {
    let mut contact = create_test_contact_with_name("Bob", 0x01);

    // Can be favorite and blocked
    contact.set_favorite(true);
    contact.set_blocked(true);
    assert!(contact.is_favorite());
    assert!(contact.is_blocked());

    // Can be favorite and hidden
    contact.set_blocked(false);
    contact.set_hidden(true);
    assert!(contact.is_favorite());
    assert!(contact.is_hidden());

    // Removing favorite doesn't affect other flags
    contact.set_favorite(false);
    assert!(!contact.is_favorite());
    assert!(contact.is_hidden());
}

// ============================================================
// Notes Tests
// ============================================================

// @scenario: contacts_management.feature:Add personal note to contact
#[test]
fn test_contact_set_note_stores_note() {
    let storage = create_test_storage();
    let contact = create_test_contact_with_name("Carol", 0x03);
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    // Set a note
    let note = "Met at conference 2024";
    let note_encrypted =
        vauchi_core::crypto::encrypt(&SymmetricKey::generate(), note.as_bytes()).unwrap();
    storage
        .save_personal_notes(&contact_id, &note_encrypted)
        .unwrap();

    // Load back
    let loaded_notes = storage.load_personal_notes(&contact_id).unwrap();
    assert!(
        loaded_notes.is_some(),
        "Personal notes should be present after saving"
    );
    assert_eq!(
        loaded_notes.unwrap(),
        note_encrypted,
        "Loaded encrypted notes should match saved encrypted notes"
    );
}

// @scenario: contacts_management.feature:Edit contact note
#[test]
fn test_contact_edit_note_updates_note() {
    let storage = create_test_storage();
    let contact = create_test_contact_with_name("Carol", 0x03);
    let contact_id = contact.id().to_string();
    let enc_key = SymmetricKey::generate();

    storage.save_contact(&contact).unwrap();

    // Set initial note
    let note1 = "Met at conference 2024";
    let note1_enc = vauchi_core::crypto::encrypt(&enc_key, note1.as_bytes()).unwrap();
    storage
        .save_personal_notes(&contact_id, &note1_enc)
        .unwrap();

    // Edit note (overwrite)
    let note2 = "Met at tech conference, works at Acme";
    let note2_enc = vauchi_core::crypto::encrypt(&enc_key, note2.as_bytes()).unwrap();
    storage
        .save_personal_notes(&contact_id, &note2_enc)
        .unwrap();

    // Load back — should be updated
    let loaded = storage.load_personal_notes(&contact_id).unwrap().unwrap();

    // Decrypt and verify it's the updated note
    let decrypted = vauchi_core::crypto::decrypt(&enc_key, &loaded).unwrap();
    assert_eq!(
        String::from_utf8(decrypted).unwrap(),
        note2,
        "Note should be updated to new text"
    );
}

// @scenario: contacts_management.feature:Delete contact note
#[test]
fn test_contact_delete_note_removes_note() {
    let storage = create_test_storage();
    let contact = create_test_contact_with_name("Carol", 0x03);
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    // Set a note
    let note_enc = vauchi_core::crypto::encrypt(&SymmetricKey::generate(), b"Some note").unwrap();
    storage.save_personal_notes(&contact_id, &note_enc).unwrap();

    // Delete note by saving NULL (empty bytes to clear)
    storage.delete_personal_notes(&contact_id).unwrap();

    // Load back — should be None
    let loaded = storage.load_personal_notes(&contact_id).unwrap();
    assert!(
        loaded.is_none(),
        "Personal notes should be None after deletion"
    );
}

// @scenario: contacts_management.feature:Notes are not shared with contact
#[test]
fn test_contact_notes_not_included_in_exchange_payload() {
    // The ExchangePayload struct (internal to encrypted_message.rs) only contains:
    // - identity_key
    // - exchange_key
    // - display_name
    //
    // Personal notes are stored ONLY in local storage (personal_notes_encrypted column).
    // They never appear in:
    // 1. The Contact struct (not a field)
    // 2. The exchange payload
    // 3. Any sync message

    // Verify by creating a contact, adding notes to storage, then checking
    // that the contact card (which IS shared) does not contain notes
    let storage = create_test_storage();
    let contact = create_test_contact_with_name("Bob", 0x02);
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact).unwrap();

    // Add a note to storage
    let note_enc =
        vauchi_core::crypto::encrypt(&SymmetricKey::generate(), b"Secret note about Bob").unwrap();
    storage.save_personal_notes(&contact_id, &note_enc).unwrap();

    // Load the contact back — the Contact struct should NOT expose notes
    let loaded_contact = storage.load_contact(&contact_id).unwrap().unwrap();

    // The ContactCard (the shared data) must not contain personal notes
    let card = loaded_contact.card();
    let card_json = serde_json::to_string(card).unwrap();
    assert!(
        !card_json.contains("Secret note about Bob"),
        "Personal notes must NOT appear in the contact card (shared data)"
    );
    assert!(
        !card_json.contains("personal_note"),
        "Personal notes field must NOT exist in the contact card serialization"
    );

    // Notes are only accessible through the separate storage API
    let notes = storage.load_personal_notes(&contact_id).unwrap();
    assert!(
        notes.is_some(),
        "Notes should be accessible through storage API"
    );
}

// ============================================================
// Adversarial Tests (CC-14)
// ============================================================

// @cc14: Adversarial test data for notes
#[test]
fn test_contact_note_unicode_edge_cases() {
    let storage = create_test_storage();
    let contact = create_test_contact_with_name("Unicode", 0x04);
    let contact_id = contact.id().to_string();
    let enc_key = SymmetricKey::generate();

    storage.save_contact(&contact).unwrap();

    let test_cases = vec![
        ("emoji", "\u{1F600}\u{1F60D}\u{1F4A9}"),
        (
            "rtl_arabic",
            "\u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064A}\u{0629}",
        ),
        ("cjk", "\u{4E2D}\u{6587}\u{6D4B}\u{8BD5}"),
        (
            "zalgo",
            "H\u{0354}\u{0367}\u{0349}e\u{0344}\u{0302}\u{0360}",
        ),
        ("null_bytes", "note\x00with\x00nulls"),
        ("newlines", "line1\nline2\r\nline3"),
        (
            "mixed_scripts",
            "Hello \u{0410}\u{0411}\u{0412} \u{4E16}\u{754C}",
        ),
    ];

    for (label, note_text) in test_cases {
        let note_enc = vauchi_core::crypto::encrypt(&enc_key, note_text.as_bytes()).unwrap();
        storage.save_personal_notes(&contact_id, &note_enc).unwrap();

        let loaded = storage.load_personal_notes(&contact_id).unwrap().unwrap();
        let decrypted = vauchi_core::crypto::decrypt(&enc_key, &loaded).unwrap();
        assert_eq!(
            decrypted,
            note_text.as_bytes(),
            "Unicode edge case '{}' should round-trip correctly",
            label
        );
    }
}

// @cc14: Max length test
#[test]
fn test_contact_note_max_length() {
    let storage = create_test_storage();
    let contact = create_test_contact_with_name("MaxNote", 0x05);
    let contact_id = contact.id().to_string();
    let enc_key = SymmetricKey::generate();

    storage.save_contact(&contact).unwrap();

    // Test with a very large note (10KB)
    let large_note = "A".repeat(10_000);
    let note_enc = vauchi_core::crypto::encrypt(&enc_key, large_note.as_bytes()).unwrap();
    storage.save_personal_notes(&contact_id, &note_enc).unwrap();

    let loaded = storage.load_personal_notes(&contact_id).unwrap().unwrap();
    let decrypted = vauchi_core::crypto::decrypt(&enc_key, &loaded).unwrap();
    assert_eq!(
        decrypted.len(),
        10_000,
        "Large note (10KB) should be stored and retrieved correctly"
    );
    assert_eq!(
        String::from_utf8(decrypted).unwrap(),
        large_note,
        "Large note content should match"
    );
}

// @cc14: Empty string edge case
#[test]
fn test_contact_note_empty_string() {
    let storage = create_test_storage();
    let contact = create_test_contact_with_name("EmptyNote", 0x06);
    let contact_id = contact.id().to_string();
    let enc_key = SymmetricKey::generate();

    storage.save_contact(&contact).unwrap();

    // Save an empty string note (encrypted empty bytes)
    let empty_note = "";
    let note_enc = vauchi_core::crypto::encrypt(&enc_key, empty_note.as_bytes()).unwrap();
    storage.save_personal_notes(&contact_id, &note_enc).unwrap();

    let loaded = storage.load_personal_notes(&contact_id).unwrap().unwrap();
    let decrypted = vauchi_core::crypto::decrypt(&enc_key, &loaded).unwrap();
    assert_eq!(
        decrypted.len(),
        0,
        "Empty note should decrypt to zero bytes"
    );
}

// ============================================================
// from_sync_data_full with favorite Tests
// ============================================================

// @scenario: sync_updates.feature:Contact sync includes favorite status
#[test]
fn test_contact_from_sync_data_full_with_favorite() {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new("Fav User");
    let shared_key = SymmetricKey::generate();
    let visibility_rules = VisibilityRules::new();

    let contact = Contact::from_sync_data_full(
        public_key,
        card,
        shared_key,
        1234567890,
        false,
        visibility_rules,
        false, // hidden
        false, // blocked
        false, // recovery_trusted
    );

    // from_sync_data_full should create contact with favorite=false by default
    assert!(
        !contact.is_favorite(),
        "Contact from sync data should default to not favorite"
    );
}
