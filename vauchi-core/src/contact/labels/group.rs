// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group type definition and field-level operations.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::GroupError;

/// A visibility label for organizing contacts.
///
/// Labels allow grouping contacts and assigning field visibility to groups.
/// For example, a "Family" label might show personal phone and home address,
/// while "Professional" shows only work email and phone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    /// Unique identifier for this label (UUID).
    id: String,
    /// Human-readable name.
    name: String,
    /// IDs of contacts assigned to this label.
    contacts: HashSet<String>,
    /// IDs of fields visible to contacts in this label.
    visible_fields: HashSet<String>,
    /// Optional display name override for this label's contacts.
    ///
    /// When set, contacts in this label see this name instead of the
    /// user's default display name.
    #[serde(default)]
    display_name_override: Option<String>,
    /// Optional per-group bio override (ADR-054 D2). Shown to this group's
    /// contacts instead of the card's default bio.
    #[serde(default)]
    bio_override: Option<String>,
    /// Optional per-group avatar override (WebP, ADR-042). Shown to this
    /// group's contacts instead of the card's default avatar.
    #[serde(default)]
    avatar_override: Option<Vec<u8>>,
    /// Timestamp when the label was created.
    created_at: u64,
    /// Timestamp when the label was last modified.
    modified_at: u64,
}

impl Group {
    /// Creates a new label with the given name.
    ///
    /// `now` is the Unix-epoch timestamp stamped into both
    /// `created_at` and `modified_at`. Production callers source
    /// it from `storage.clock().unix_seconds()` (via
    /// `Storage::create_group`) or `self.clock.unix_seconds()`
    /// (via `Vauchi::create_group`); tests pass any fixed value.
    pub fn new(name: &str, now: u64) -> Self {
        Group {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            contacts: HashSet::new(),
            visible_fields: HashSet::new(),
            display_name_override: None,
            bio_override: None,
            avatar_override: None,
            created_at: now,
            modified_at: now,
        }
    }

    /// Creates a label from storage data.
    // Positional row args mirror the `visibility_labels` columns one-to-one;
    // a GroupRow struct refactor is deliberately out of scope (ADR-054
    // Phase 2b plan, Tidy-First seam).
    #[allow(clippy::too_many_arguments)]
    pub fn from_storage(
        id: String,
        name: String,
        contacts: HashSet<String>,
        visible_fields: HashSet<String>,
        display_name_override: Option<String>,
        bio_override: Option<String>,
        avatar_override: Option<Vec<u8>>,
        created_at: u64,
        modified_at: u64,
    ) -> Self {
        Group {
            id,
            name,
            contacts,
            visible_fields,
            display_name_override,
            bio_override,
            avatar_override,
            created_at,
            modified_at,
        }
    }

    /// Returns the label ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the label name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the label name.
    pub fn set_name(&mut self, name: &str, now: u64) {
        self.name = name.to_string();
        self.touch(now);
    }

    /// Returns the display name override, if set.
    pub fn display_name_override(&self) -> Option<&str> {
        self.display_name_override.as_deref()
    }

    /// Sets or clears the display name override.
    ///
    /// When set, contacts in this label see this name instead of the
    /// user's default display name. Pass `None` to clear the override.
    ///
    /// Validates that the name is non-empty, not whitespace-only, and
    /// at most 100 characters (after trimming).
    pub fn set_display_name_override(
        &mut self,
        name: Option<&str>,
        now: u64,
    ) -> Result<(), GroupError> {
        match name {
            None => {
                self.display_name_override = None;
                self.touch(now);
                Ok(())
            }
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(GroupError::InvalidName(
                        "Display name override cannot be empty".to_string(),
                    ));
                }
                if trimmed.chars().count() > 100 {
                    return Err(GroupError::InvalidName(
                        "Display name override cannot exceed 100 characters".to_string(),
                    ));
                }
                self.display_name_override = Some(trimmed.to_string());
                self.touch(now);
                Ok(())
            }
        }
    }

    /// Resolves the display name for contacts in this label.
    ///
    /// Returns the override if set, otherwise the provided default name.
    pub fn resolve_display_name<'a>(&'a self, default_name: &'a str) -> &'a str {
        match &self.display_name_override {
            Some(override_name) => override_name.as_str(),
            None => default_name,
        }
    }

    /// Returns the bio override, if set.
    pub fn bio_override(&self) -> Option<&str> {
        self.bio_override.as_deref()
    }

    /// Sets or clears the bio override (ADR-054 D2). Trimmed; an empty result
    /// clears it. Rejects a bio longer than `MAX_BIO_LENGTH` characters.
    pub fn set_bio_override(&mut self, bio: Option<&str>, now: u64) -> Result<(), GroupError> {
        match bio {
            None => self.bio_override = None,
            Some(raw) => {
                let normalized = crate::text::normalize_text(raw);
                if normalized.is_empty() {
                    self.bio_override = None;
                } else if normalized.chars().count() > crate::contact_card::MAX_BIO_LENGTH {
                    return Err(GroupError::InvalidBio(
                        "Bio override cannot exceed 160 characters".to_string(),
                    ));
                } else {
                    self.bio_override = Some(normalized);
                }
            }
        }
        self.touch(now);
        Ok(())
    }

    /// Resolves the bio for this group's contacts: the override if set,
    /// otherwise the provided default (the card's bio).
    pub fn resolve_bio<'a>(&'a self, default_bio: Option<&'a str>) -> Option<&'a str> {
        self.bio_override.as_deref().or(default_bio)
    }

    /// Returns the avatar override bytes, if set.
    pub fn avatar_override(&self) -> Option<&[u8]> {
        self.avatar_override.as_deref()
    }

    /// Sets or clears the avatar override (WebP, ADR-042). `None` clears.
    /// Accepts any common format and normalizes to WebP <= 32 KB internally.
    pub fn set_avatar_override(
        &mut self,
        avatar: Option<&[u8]>,
        now: u64,
    ) -> Result<(), GroupError> {
        match avatar {
            None => self.avatar_override = None,
            Some(raw) => {
                let webp = crate::contact_card::normalize_avatar(raw)
                    .map_err(|e| GroupError::InvalidAvatar(e.to_string()))?;
                self.avatar_override = Some(webp);
            }
        }
        self.touch(now);
        Ok(())
    }

    /// Resolves the avatar for this group's contacts: the override if set,
    /// otherwise the provided default (the card's avatar).
    pub fn resolve_avatar<'a>(&'a self, default_avatar: Option<&'a [u8]>) -> Option<&'a [u8]> {
        self.avatar_override.as_deref().or(default_avatar)
    }

    /// Returns the set of contact IDs in this label.
    pub fn contacts(&self) -> &HashSet<String> {
        &self.contacts
    }

    /// Returns the number of contacts in this label.
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Checks if a contact is in this label.
    pub fn contains_contact(&self, contact_id: &str) -> bool {
        self.contacts.contains(contact_id)
    }

    /// Adds a contact to this label.
    ///
    /// Returns true if the contact was added (wasn't already present).
    pub fn add_contact(&mut self, contact_id: &str, now: u64) -> bool {
        let added = self.contacts.insert(contact_id.to_string());
        if added {
            self.touch(now);
        }
        added
    }

    /// Removes a contact from this label.
    ///
    /// Returns true if the contact was removed (was present).
    pub fn remove_contact(&mut self, contact_id: &str, now: u64) -> bool {
        let removed = self.contacts.remove(contact_id);
        if removed {
            self.touch(now);
        }
        removed
    }

    /// Returns the set of field IDs visible to contacts in this label.
    pub fn visible_fields(&self) -> &HashSet<String> {
        &self.visible_fields
    }

    /// Checks if a field is visible to contacts in this label.
    pub fn is_field_visible(&self, field_id: &str) -> bool {
        self.visible_fields.contains(field_id)
    }

    /// Adds a field to the visible fields for this label.
    ///
    /// Returns true if the field was added (wasn't already present).
    pub fn add_visible_field(&mut self, field_id: &str, now: u64) -> bool {
        let added = self.visible_fields.insert(field_id.to_string());
        if added {
            self.touch(now);
        }
        added
    }

    /// Removes a field from the visible fields for this label.
    ///
    /// Returns true if the field was removed (was present).
    pub fn remove_visible_field(&mut self, field_id: &str, now: u64) -> bool {
        let removed = self.visible_fields.remove(field_id);
        if removed {
            self.touch(now);
        }
        removed
    }

    /// Sets all visible fields at once.
    pub fn set_visible_fields(&mut self, field_ids: HashSet<String>, now: u64) {
        self.visible_fields = field_ids;
        self.touch(now);
    }

    /// Returns the creation timestamp.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Returns the last modification timestamp.
    pub fn modified_at(&self) -> u64 {
        self.modified_at
    }

    /// Updates the modification timestamp.
    fn touch(&mut self, now: u64) {
        self.modified_at = now;
    }
}

// INLINE_TEST_REQUIRED: tests access private Group fields
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_label_display_name_override() {
        let mut label = Group::new("Family", 0);

        assert_eq!(label.display_name_override(), None);

        label
            .set_display_name_override(Some("Matt"), 0)
            .expect("valid name should succeed");
        assert_eq!(label.display_name_override(), Some("Matt"));

        label
            .set_display_name_override(None, 0)
            .expect("clearing should succeed");
        assert_eq!(label.display_name_override(), None);
    }

    #[test]
    fn test_label_display_name_override_validation() {
        let mut label = Group::new("Friends", 0);

        let result = label.set_display_name_override(Some(""), 0);
        assert!(matches!(result, Err(GroupError::InvalidName(_))));

        let result = label.set_display_name_override(Some("   "), 0);
        assert!(matches!(result, Err(GroupError::InvalidName(_))));

        let long_name = "a".repeat(101);
        let result = label.set_display_name_override(Some(&long_name), 0);
        assert!(matches!(result, Err(GroupError::InvalidName(_))));

        let max_name = "b".repeat(100);
        label
            .set_display_name_override(Some(&max_name), 0)
            .expect("100 chars should succeed");
        assert_eq!(label.display_name_override(), Some(max_name.as_str()));

        label
            .set_display_name_override(Some("  Dr. Egloff  "), 0)
            .expect("trimmed name should succeed");
        assert_eq!(label.display_name_override(), Some("Dr. Egloff"));
    }

    #[test]
    fn test_label_resolve_display_name() {
        let mut label = Group::new("Business", 0);

        assert_eq!(label.resolve_display_name("Mattia Egloff"), "Mattia Egloff");

        label
            .set_display_name_override(Some("Dr. Egloff"), 0)
            .expect("valid name");
        assert_eq!(label.resolve_display_name("Mattia Egloff"), "Dr. Egloff");

        label
            .set_display_name_override(None, 0)
            .expect("clearing should succeed");
        assert_eq!(label.resolve_display_name("Mattia Egloff"), "Mattia Egloff");
    }

    // @internal
    #[test]
    fn test_label_bio_override() {
        let mut label = Group::new("Family", 0);

        assert_eq!(label.bio_override(), None);

        label
            .set_bio_override(Some("Dad of three"), 0)
            .expect("valid bio should succeed");
        assert_eq!(label.bio_override(), Some("Dad of three"));

        label
            .set_bio_override(None, 0)
            .expect("clearing should succeed");
        assert_eq!(label.bio_override(), None);
    }

    // @internal
    #[test]
    fn test_label_bio_override_validation() {
        let mut label = Group::new("Friends", 0);

        // Empty / whitespace clears (mirrors ContactCard::set_bio), not an error.
        label.set_bio_override(Some(""), 0).expect("empty clears");
        assert_eq!(label.bio_override(), None);
        label
            .set_bio_override(Some("   "), 0)
            .expect("whitespace clears");
        assert_eq!(label.bio_override(), None);

        let too_long = "a".repeat(crate::contact_card::MAX_BIO_LENGTH + 1);
        let result = label.set_bio_override(Some(&too_long), 0);
        assert!(matches!(result, Err(GroupError::InvalidBio(_))));

        let max_bio = "b".repeat(crate::contact_card::MAX_BIO_LENGTH);
        label
            .set_bio_override(Some(&max_bio), 0)
            .expect("160 chars should succeed");
        assert_eq!(label.bio_override(), Some(max_bio.as_str()));

        label
            .set_bio_override(Some("  trimmed bio  "), 0)
            .expect("trimmed bio should succeed");
        assert_eq!(label.bio_override(), Some("trimmed bio"));
    }

    // @internal
    #[test]
    fn test_label_resolve_bio() {
        let mut label = Group::new("Business", 0);

        assert_eq!(label.resolve_bio(Some("default bio")), Some("default bio"));
        assert_eq!(label.resolve_bio(None), None);

        label
            .set_bio_override(Some("work bio"), 0)
            .expect("valid bio");
        assert_eq!(label.resolve_bio(Some("default bio")), Some("work bio"));
        // Override wins even when the card has no default bio.
        assert_eq!(label.resolve_bio(None), Some("work bio"));

        label.set_bio_override(None, 0).expect("clearing");
        assert_eq!(label.resolve_bio(Some("default bio")), Some("default bio"));
    }

    // @internal
    #[test]
    fn test_label_avatar_override() {
        let mut label = Group::new("Family", 0);
        assert_eq!(label.avatar_override(), None);

        let png = encode_test_png();
        label
            .set_avatar_override(Some(&png), 0)
            .expect("valid image should succeed");
        let stored = label.avatar_override().expect("avatar set");
        // Stored as WebP (normalized), not the raw PNG bytes.
        assert_eq!(&stored[0..4], b"RIFF", "avatar override should be WebP");
        assert_eq!(&stored[8..12], b"WEBP");

        label
            .set_avatar_override(None, 0)
            .expect("clearing should succeed");
        assert_eq!(label.avatar_override(), None);
    }

    // @internal
    #[test]
    fn test_label_avatar_override_rejects_invalid() {
        let mut label = Group::new("Friends", 0);

        let result = label.set_avatar_override(Some(&[]), 0);
        assert!(matches!(result, Err(GroupError::InvalidAvatar(_))));

        let result = label.set_avatar_override(Some(b"not an image"), 0);
        assert!(matches!(result, Err(GroupError::InvalidAvatar(_))));
        // A rejected set leaves the prior value untouched.
        assert_eq!(label.avatar_override(), None);
    }

    // @internal
    #[test]
    fn test_label_resolve_avatar() {
        let mut label = Group::new("Business", 0);
        let default_avatar = vec![1u8, 2, 3];

        assert_eq!(
            label.resolve_avatar(Some(&default_avatar)),
            Some(default_avatar.as_slice())
        );
        assert_eq!(label.resolve_avatar(None), None);

        let png = encode_test_png();
        label
            .set_avatar_override(Some(&png), 0)
            .expect("valid image");
        let resolved = label
            .resolve_avatar(Some(&default_avatar))
            .expect("override resolves");
        assert_eq!(&resolved[0..4], b"RIFF");
        assert_ne!(resolved, default_avatar.as_slice());
    }

    // @internal
    #[test]
    fn test_label_bio_override_caps_by_chars_not_bytes() {
        let mut label = Group::new("Family", 0);
        let cap = crate::contact_card::MAX_BIO_LENGTH;

        // 'ä' (U+00E4) is NFC-stable and 2 bytes: `cap` of them is within the
        // char cap but exceeds it in bytes — the cap must count chars.
        let at_cap = "ä".repeat(cap);
        assert!(
            at_cap.len() > cap,
            "precondition: byte length exceeds char cap"
        );
        label
            .set_bio_override(Some(&at_cap), 0)
            .expect("cap multibyte chars should succeed (cap is chars, not bytes)");
        assert_eq!(label.bio_override().map(|b| b.chars().count()), Some(cap));

        let over_cap = "ä".repeat(cap + 1);
        assert!(matches!(
            label.set_bio_override(Some(&over_cap), 0),
            Err(GroupError::InvalidBio(_))
        ));
    }

    /// Encodes a 1x1 PNG that `normalize_avatar` accepts (mirrors avatar_normalize_tests).
    fn encode_test_png() -> Vec<u8> {
        use image::{ImageBuffer, Rgb};
        let img: image::RgbImage = ImageBuffer::from_pixel(1, 1, Rgb([42u8, 128, 200]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("PNG encoding should succeed");
        buf
    }

    #[test]
    fn test_labels_are_local() {
        // Labels exist only in GroupManager, not in Contact
        // This test verifies the design doesn't leak labels to contacts
        let label = Group::new("Secret Name", 0);

        // The label name is never serialized in a way that would be sent to contacts
        // Label data should only be synced to the user's own devices
        assert_eq!(label.name(), "Secret Name");
        // The contact sees field visibility, not labels
    }
}
