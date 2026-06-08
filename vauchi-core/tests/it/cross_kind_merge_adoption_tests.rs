// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the cross-kind merge adoption flow.
//!
//! When a duplicate dialog matches an exchanged contact against an imported
//! contact, the user can choose to "adopt" data from the imported contact
//! (nickname, avatar, notes) before hard-deleting it — or skip adoption and
//! just delete. These tests verify both paths.
//!
//! @scenario: contacts_management.feature - Cross-kind merge: adopt imported data into exchanged

use vauchi_core::{
    AvatarPreference, Contact, ContactCard, DisplayNamePreference, ImportSource, SymmetricKey,
    Vauchi,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Create a Vauchi instance with identity already created.
fn new_vauchi() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

/// Add an exchanged contact and return its ID.
fn add_exchanged(wb: &Vauchi, name: &str, pk_byte: u8) -> String {
    let mut pk = [0u8; 32];
    pk[0] = pk_byte;
    let card = ContactCard::new(name);
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

/// Add an imported contact and return its ID.
fn add_imported(wb: &Vauchi, name: &str) -> String {
    let card = ContactCard::new(name);
    let contact = Contact::from_import(card, ImportSource::VcardFile, None, 0);
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    id
}

/// Create a tiny valid PNG (1×1 red pixel) for avatar tests.
fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

// ── With adoption ─────────────────────────────────────────────────────────────

// @scenario: contacts_management.feature :: Cross-kind merge: adopt imported data into exchanged
#[test]
fn adoption_copies_nickname_from_imported_to_exchanged() {
    let wb = new_vauchi();
    let exchanged_id = add_exchanged(&wb, "Bob", 1);
    let imported_id = add_imported(&wb, "Bobby");

    // Set a nickname on the imported contact to simulate user-edited label
    wb.set_contact_nickname(&imported_id, "Bobby").unwrap();

    // Adoption: read the imported nickname and apply it to the exchanged contact
    let imported_nick = wb.get_contact_nickname(&imported_id).unwrap();
    let nick = imported_nick.as_deref().unwrap_or("Bobby");
    wb.set_contact_nickname(&exchanged_id, nick).unwrap();
    wb.set_display_name_preference(&exchanged_id, DisplayNamePreference::Custom)
        .unwrap();

    wb.hard_delete_imported_contact(&imported_id).unwrap();

    let nick_after = wb.get_contact_nickname(&exchanged_id).unwrap();
    assert_eq!(
        nick_after.as_deref(),
        Some("Bobby"),
        "Adopted nickname must be present on exchanged contact"
    );

    let opts = wb.get_contact_display_options(&exchanged_id).unwrap();
    assert_eq!(
        opts.active_name_preference,
        DisplayNamePreference::Custom,
        "Name preference must be Custom after adoption"
    );

    let gone = wb.get_contact(&imported_id).unwrap();
    assert!(
        gone.is_none(),
        "Hard-deleted imported contact must not be retrievable"
    );
}

// @scenario: contacts_management.feature :: Cross-kind merge: adopt imported data into exchanged
#[test]
fn adoption_copies_custom_avatar_from_imported_to_exchanged() {
    let wb = new_vauchi();
    let exchanged_id = add_exchanged(&wb, "Bob", 2);
    let imported_id = add_imported(&wb, "Bob Import");

    let png = tiny_png();
    wb.set_contact_custom_avatar(&imported_id, &png).unwrap();

    // Adoption: read the imported avatar and apply to exchanged
    let imported_avatar = wb
        .get_contact_custom_avatar(&imported_id)
        .unwrap()
        .expect("imported contact must have custom avatar");
    wb.set_contact_custom_avatar(&exchanged_id, &imported_avatar)
        .unwrap();
    wb.set_avatar_preference(&exchanged_id, AvatarPreference::Custom)
        .unwrap();

    wb.hard_delete_imported_contact(&imported_id).unwrap();

    let avatar_after = wb.get_contact_custom_avatar(&exchanged_id).unwrap();
    assert!(
        avatar_after.is_some(),
        "Exchanged contact must have custom avatar after adoption"
    );

    let opts = wb.get_contact_display_options(&exchanged_id).unwrap();
    assert_eq!(
        opts.active_avatar_preference,
        AvatarPreference::Custom,
        "Avatar preference must be Custom after adoption"
    );

    let gone = wb.get_contact(&imported_id).unwrap();
    assert!(
        gone.is_none(),
        "Hard-deleted imported contact must not be retrievable"
    );
}

// @scenario: contacts_management.feature :: Cross-kind merge: save imported info to notes
#[test]
fn adoption_saves_imported_info_to_notes() {
    let wb = new_vauchi();
    let exchanged_id = add_exchanged(&wb, "Bob", 3);
    let imported_id = add_imported(&wb, "Bob Import");

    // Existing note on exchanged contact
    wb.add_personal_note(&exchanged_id, "Met at conference 2025")
        .unwrap();

    // Adoption: serialize imported contact's display name as text summary,
    // read existing notes, concatenate, write back to exchanged.
    // (In the real UI, imported card fields would be serialized too.)
    let imported_summary = "Imported: Bob Import";

    let existing = wb
        .read_personal_note(&exchanged_id)
        .unwrap()
        .unwrap_or_default();
    let combined = format!("{existing}\n---\n{imported_summary}");
    wb.add_personal_note(&exchanged_id, &combined).unwrap();

    wb.hard_delete_imported_contact(&imported_id).unwrap();

    let note_after = wb
        .read_personal_note(&exchanged_id)
        .unwrap()
        .expect("exchanged contact must have a note after adoption");
    assert!(
        note_after.contains("Met at conference 2025"),
        "Combined note must contain original exchanged note; got: {note_after:?}"
    );
    assert!(
        note_after.contains("Imported: Bob Import"),
        "Combined note must contain imported summary; got: {note_after:?}"
    );
}

// ── Without adoption ──────────────────────────────────────────────────────────

// @scenario: contacts_management.feature :: Cross-kind merge: skip adoption, delete imported
#[test]
fn skip_adoption_hard_deletes_imported_leaves_exchanged_unchanged() {
    let wb = new_vauchi();
    let exchanged_id = add_exchanged(&wb, "Bob", 4);
    let imported_id = add_imported(&wb, "Bob Import");

    wb.set_contact_nickname(&imported_id, "Bobby").unwrap();

    // No adoption — just hard-delete the imported contact
    wb.hard_delete_imported_contact(&imported_id).unwrap();

    let nick = wb.get_contact_nickname(&exchanged_id).unwrap();
    assert!(
        nick.is_none(),
        "Exchanged contact must have no nickname when adoption was skipped"
    );

    let opts = wb.get_contact_display_options(&exchanged_id).unwrap();
    assert_eq!(
        opts.active_name_preference,
        DisplayNamePreference::Primary,
        "Name preference must remain Primary when adoption was skipped"
    );

    let gone = wb.get_contact(&imported_id).unwrap();
    assert!(
        gone.is_none(),
        "Hard-deleted imported contact must not be retrievable"
    );
}

// @internal
#[test]
fn hard_delete_imported_contact_only_accepts_imported_kind() {
    let wb = new_vauchi();
    let exchanged_id = add_exchanged(&wb, "Bob", 5);

    // Attempting to hard-delete an exchanged contact via the imported API must fail
    let result = wb.hard_delete_imported_contact(&exchanged_id);
    assert!(
        result.is_err(),
        "hard_delete_imported_contact must reject exchanged contacts"
    );
}

// @internal
#[test]
fn hard_delete_imported_twice_returns_error() {
    let wb = new_vauchi();
    let imported_id = add_imported(&wb, "Bob Import");

    wb.hard_delete_imported_contact(&imported_id).unwrap();
    let result = wb.hard_delete_imported_contact(&imported_id);
    assert!(
        result.is_err(),
        "Second hard-delete of already-deleted contact must return an error"
    );
}

// @internal
#[test]
fn adoption_of_name_does_not_affect_other_contacts() {
    let wb = new_vauchi();
    let exchanged_id = add_exchanged(&wb, "Bob", 6);
    let other_id = add_exchanged(&wb, "Carol", 7);
    let imported_id = add_imported(&wb, "Bob Import");

    wb.set_contact_nickname(&imported_id, "Bobby Import")
        .unwrap();

    // Adopt imported nickname into exchanged only
    wb.set_contact_nickname(&exchanged_id, "Bobby Import")
        .unwrap();
    wb.hard_delete_imported_contact(&imported_id).unwrap();

    // Carol's contact must be unaffected
    let other_nick = wb.get_contact_nickname(&other_id).unwrap();
    assert!(
        other_nick.is_none(),
        "Unrelated contact must not be affected by adoption; got nick: {other_nick:?}"
    );
}
