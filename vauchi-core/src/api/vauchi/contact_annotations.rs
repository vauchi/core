// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local per-contact personalization: encrypted personal notes,
//! nicknames, custom avatars, and display preferences.
//!
//! Carved out of `contacts.rs` — the split prescribed by
//! `.file-size-baseline` ("the nickname/display-preference cluster is
//! the natural carve-out target").

use crate::contact::display::{AvatarPreference, ContactDisplayOptions, DisplayNamePreference};
use crate::contact_card::normalize_avatar;

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    // === Personal Notes Operations ===

    /// Adds or replaces a personal note for a contact.
    ///
    /// Encrypts the plaintext note using the contact's shared key.
    /// Frontends MUST use this instead of calling crypto::encrypt directly.
    ///
    /// The change is journaled for linked devices — otherwise the note
    /// stays device-local and the owner-private state diverges (RG-10).
    pub fn add_personal_note(&self, contact_id: &str, note_text: &str) -> VauchiResult<()> {
        use crate::crypto::encrypt;

        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;
        let shared_key = contact
            .shared_key()
            .ok_or_else(|| VauchiError::Configuration("Contact has no shared key".into()))?;
        let encrypted = encrypt(shared_key, note_text.as_bytes())
            .map_err(|e| VauchiError::Configuration(format!("Encryption failed: {}", e)))?;
        self.storage
            .contacts()
            .save_personal_notes(contact_id, &encrypted)?;
        self.record_sync_item(crate::sync::SyncItem::PersonalNoteChanged {
            contact_id: contact_id.to_string(),
            note: note_text.to_string(),
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    /// Reads the personal note for a contact, decrypting it.
    ///
    /// Returns None if no note exists.
    pub fn read_personal_note(&self, contact_id: &str) -> VauchiResult<Option<String>> {
        use crate::crypto::decrypt;

        let encrypted = match self.storage.contacts().load_personal_notes(contact_id)? {
            Some(data) => data,
            None => return Ok(None),
        };
        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;
        let shared_key = contact
            .shared_key()
            .ok_or_else(|| VauchiError::Configuration("Contact has no shared key".into()))?;
        let plaintext = decrypt(shared_key, &encrypted)
            .map_err(|e| VauchiError::Configuration(format!("Decryption failed: {}", e)))?;
        Ok(Some(String::from_utf8(plaintext).map_err(|e| {
            VauchiError::Configuration(format!("Note is not valid UTF-8: {}", e))
        })?))
    }

    /// Saves encrypted personal notes for a contact (raw bytes).
    ///
    /// Low-level API for sync/migration. Prefer `add_personal_note()`.
    pub fn save_personal_notes(
        &self,
        contact_id: &str,
        notes_encrypted: &[u8],
    ) -> VauchiResult<()> {
        self.storage
            .contacts()
            .save_personal_notes(contact_id, notes_encrypted)?;
        Ok(())
    }

    /// Loads encrypted personal notes for a contact (raw bytes).
    ///
    /// Low-level API for sync/migration. Prefer `read_personal_note()`.
    pub fn load_personal_notes(&self, contact_id: &str) -> VauchiResult<Option<Vec<u8>>> {
        Ok(self.storage.contacts().load_personal_notes(contact_id)?)
    }

    /// Deletes personal notes for a contact and journals a tombstone for linked devices.
    pub fn delete_personal_notes(&self, contact_id: &str) -> VauchiResult<()> {
        self.storage.contacts().delete_personal_notes(contact_id)?;
        self.record_sync_item(crate::sync::SyncItem::PersonalNoteRemoved {
            contact_id: contact_id.to_string(),
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }

    // === Contact Nickname Operations ===

    /// Sets a local nickname for a contact. Validates: non-empty after trim, <= 100 chars.
    pub fn set_contact_nickname(&self, contact_id: &str, nickname: &str) -> VauchiResult<()> {
        let trimmed = nickname.trim();
        if trimmed.is_empty() {
            return Err(VauchiError::InvalidState("Nickname cannot be empty".into()));
        }
        if trimmed.chars().count() > 100 {
            return Err(VauchiError::InvalidState(
                "Nickname exceeds 100 characters".into(),
            ));
        }
        self.storage
            .contacts()
            .save_contact_nickname(contact_id, trimmed.as_bytes())?;
        Ok(())
    }

    /// Clears the local nickname for a contact.
    ///
    /// Resets display name preference to Primary if it was Custom.
    pub fn clear_contact_nickname(&self, contact_id: &str) -> VauchiResult<()> {
        self.storage
            .contacts()
            .delete_contact_nickname(contact_id)?;
        let (name_pref, _) = self
            .storage
            .contacts()
            .load_display_preferences(contact_id)?;
        if name_pref == DisplayNamePreference::Custom {
            self.storage
                .contacts()
                .save_display_name_preference(contact_id, &DisplayNamePreference::Primary)?;
        }
        Ok(())
    }

    /// Sets the locally displayed name for a contact.
    ///
    /// A contact's card is signed and immutable, and for CEK-protected
    /// contacts the plaintext name is not stored at rest (ADR-015) — so an
    /// edited name is persisted as a local encrypted nickname with a
    /// `Custom` display preference, which the read paths resolve into the
    /// shown name. Setting the name back to the card's primary name clears
    /// the override.
    pub fn set_contact_display_name(&self, contact_id: &str, name: &str) -> VauchiResult<()> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(VauchiError::InvalidState(
                "Display name cannot be empty".into(),
            ));
        }
        let contact = self
            .get_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;
        if trimmed == contact.card().display_name() {
            self.clear_contact_nickname(contact_id)?;
        } else {
            self.set_contact_nickname(contact_id, trimmed)?;
            self.set_display_name_preference(contact_id, DisplayNamePreference::Custom)?;
        }
        Ok(())
    }

    /// Returns the local nickname for a contact, or None if unset.
    pub fn get_contact_nickname(&self, contact_id: &str) -> VauchiResult<Option<String>> {
        Ok(self.storage.contacts().load_contact_nickname(contact_id)?)
    }

    // === Contact Custom Avatar Operations ===

    /// Sets a custom avatar for a contact.
    ///
    /// Accepts any supported image format (PNG, JPEG, BMP, WebP).
    /// Core normalizes to WebP <= 32 KB internally (ADR-042).
    pub fn set_contact_custom_avatar(&self, contact_id: &str, data: &[u8]) -> VauchiResult<()> {
        let webp = normalize_avatar(data).map_err(|e: crate::contact_card::ContactCardError| {
            VauchiError::InvalidState(e.to_string())
        })?;
        self.storage
            .contacts()
            .save_contact_custom_avatar(contact_id, &webp)?;
        Ok(())
    }

    /// Clears the custom avatar for a contact.
    ///
    /// Resets avatar preference to Primary if it was Custom.
    pub fn clear_contact_custom_avatar(&self, contact_id: &str) -> VauchiResult<()> {
        self.storage
            .contacts()
            .delete_contact_custom_avatar(contact_id)?;
        let (_, avatar_pref) = self
            .storage
            .contacts()
            .load_display_preferences(contact_id)?;
        if avatar_pref == AvatarPreference::Custom {
            self.storage
                .contacts()
                .save_avatar_preference(contact_id, &AvatarPreference::Primary)?;
        }
        Ok(())
    }

    /// Returns the custom avatar for a contact, or None if unset.
    pub fn get_contact_custom_avatar(&self, contact_id: &str) -> VauchiResult<Option<Vec<u8>>> {
        Ok(self
            .storage
            .contacts()
            .load_contact_custom_avatar(contact_id)?)
    }

    // === Display Preference Operations ===

    /// Sets the display name preference for a contact.
    ///
    /// Setting `Custom` when no nickname is set returns `InvalidState`.
    /// Setting `SharedName` when name not in shared set returns `InvalidState`.
    pub fn set_display_name_preference(
        &self,
        contact_id: &str,
        pref: DisplayNamePreference,
    ) -> VauchiResult<()> {
        match &pref {
            DisplayNamePreference::Primary => {}
            DisplayNamePreference::Custom => {
                let nick = self.storage.contacts().load_contact_nickname(contact_id)?;
                if nick.is_none() {
                    return Err(VauchiError::InvalidState(
                        "Cannot set Custom name preference without a nickname".into(),
                    ));
                }
            }
            DisplayNamePreference::SharedName { name } => {
                let names = self.storage.contacts().list_shared_names(contact_id)?;
                if !names.iter().any(|n| n.name == *name) {
                    return Err(VauchiError::InvalidState(format!(
                        "Shared name '{}' not found",
                        name
                    )));
                }
            }
        }
        self.storage
            .contacts()
            .save_display_name_preference(contact_id, &pref)?;
        Ok(())
    }

    /// Sets the avatar preference for a contact.
    ///
    /// Setting `Custom` when no custom avatar is set returns `InvalidState`.
    /// Setting `SharedAvatar` when hash not in shared set returns `InvalidState`.
    pub fn set_avatar_preference(
        &self,
        contact_id: &str,
        pref: AvatarPreference,
    ) -> VauchiResult<()> {
        match &pref {
            AvatarPreference::Primary => {}
            AvatarPreference::Custom => {
                let has = self
                    .storage
                    .contacts()
                    .has_contact_custom_avatar(contact_id)?;
                if !has {
                    return Err(VauchiError::InvalidState(
                        "Cannot set Custom avatar preference without a custom avatar".into(),
                    ));
                }
            }
            AvatarPreference::SharedAvatar { hash } => {
                let avatars = self.storage.contacts().list_shared_avatars(contact_id)?;
                if !avatars.iter().any(|a| a.avatar_hash == *hash) {
                    return Err(VauchiError::InvalidState(format!(
                        "Shared avatar '{}' not found",
                        hash
                    )));
                }
            }
        }
        self.storage
            .contacts()
            .save_avatar_preference(contact_id, &pref)?;
        Ok(())
    }

    /// Returns all display options for a contact (for the chooser screen).
    pub fn get_contact_display_options(
        &self,
        contact_id: &str,
    ) -> VauchiResult<ContactDisplayOptions> {
        use crate::contact::display::*;

        let _contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(contact_id.to_string()))?;
        let nickname = self.storage.contacts().load_contact_nickname(contact_id)?;
        let shared_names = self.storage.contacts().list_shared_names(contact_id)?;
        let shared_avatars = self.storage.contacts().list_shared_avatars(contact_id)?;
        let (name_pref, avatar_pref) = self
            .storage
            .contacts()
            .load_display_preferences(contact_id)?;
        let has_custom_avatar = self
            .storage
            .contacts()
            .has_contact_custom_avatar(contact_id)?;

        // Build name options: shared names + custom nickname
        let mut names: Vec<NameOption> = shared_names
            .iter()
            .map(|n| NameOption {
                source: if n.is_primary {
                    DisplayNamePreference::Primary
                } else {
                    DisplayNamePreference::SharedName {
                        name: n.name.clone(),
                    }
                },
                name: n.name.clone(),
                is_primary: n.is_primary,
            })
            .collect();
        if let Some(ref nick) = nickname {
            names.push(NameOption {
                source: DisplayNamePreference::Custom,
                name: nick.clone(),
                is_primary: false,
            });
        }

        // Build avatar options: shared avatars + custom avatar
        let mut avatars: Vec<AvatarOption> = shared_avatars
            .iter()
            .map(|a| AvatarOption {
                source: if a.is_primary {
                    AvatarPreference::Primary
                } else {
                    AvatarPreference::SharedAvatar {
                        hash: a.avatar_hash.clone(),
                    }
                },
                has_data: true,
                is_primary: a.is_primary,
            })
            .collect();
        avatars.push(AvatarOption {
            source: AvatarPreference::Custom,
            has_data: has_custom_avatar,
            is_primary: false,
        });

        Ok(ContactDisplayOptions {
            names,
            avatars,
            active_name_preference: name_pref,
            active_avatar_preference: avatar_pref,
        })
    }
}
