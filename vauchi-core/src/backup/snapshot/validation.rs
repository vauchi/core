// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Validate the whole authenticated snapshot before any persistent mutation.

use std::collections::HashSet;

use crate::card::{
    ContactCard, MAX_AVATAR_SIZE, MAX_BIO_LENGTH, MAX_DISPLAY_NAME_LENGTH, MAX_FIELDS,
};
use crate::contact::{Contact, MAX_LABELS};

use super::super::{BackupError, extract_master_seed};
use super::BackupSnapshot;

fn invalid() -> BackupError {
    BackupError::Deserialization("Invalid backup state or references".into())
}

fn valid_name(name: &str, maximum: usize) -> bool {
    !name.trim().is_empty() && name.chars().count() <= maximum
}

fn validate_avatar(bytes: &[u8]) -> Result<(), BackupError> {
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_SIZE {
        return Err(invalid());
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(512);
    limits.max_image_height = Some(512);
    limits.max_alloc = Some(4 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| invalid())?;
    Ok(())
}

fn unique<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<HashSet<&'a str>, BackupError> {
    let mut set = HashSet::new();
    for value in values {
        if value.is_empty() || value.len() > 1024 || !set.insert(value) {
            return Err(invalid());
        }
    }
    Ok(set)
}

fn validate_card(card: &ContactCard) -> Result<HashSet<&str>, BackupError> {
    if !valid_name(card.display_name(), MAX_DISPLAY_NAME_LENGTH)
        || card.fields().len() > MAX_FIELDS
        || card
            .bio()
            .is_some_and(|bio| bio.chars().count() > MAX_BIO_LENGTH)
        || card
            .avatar()
            .is_some_and(|avatar| avatar.len() > MAX_AVATAR_SIZE)
    {
        return Err(invalid());
    }
    for field in card.fields() {
        field.validate().map_err(|_| invalid())?;
    }
    card.validate_size().map_err(|_| invalid())?;
    if let Some(avatar) = card.avatar() {
        validate_avatar(avatar)?;
    }
    unique(card.fields().iter().map(|field| field.id()))
}

impl BackupSnapshot {
    pub fn validate(&self) -> Result<Vec<Contact>, BackupError> {
        if !valid_name(&self.identity.display_name, MAX_DISPLAY_NAME_LENGTH)
            || self.identity.device_name.trim().is_empty()
            || self.identity.device_name.len() > 1024
            || self.created_at > i64::MAX as u64
            || self.groups.len() > MAX_LABELS
        {
            return Err(invalid());
        }
        let _seed = extract_master_seed(&self.identity)?;
        let own_fields = match &self.own_card {
            Some(card) => validate_card(card)?,
            None => HashSet::new(),
        };
        let contacts = self.restore_contacts()?;
        let ids = unique(contacts.iter().map(|contact| contact.id()))?;
        for (state, contact) in self.contacts.iter().zip(&contacts) {
            state.validate(contact)?;
            validate_card(contact.card())?;
        }
        unique(self.groups.iter().map(|group| group.id.as_str()))?;
        unique(self.groups.iter().map(|group| group.name.as_str()))?;
        for group in &self.groups {
            if !valid_name(&group.name, 50)
                || group
                    .display_name_override
                    .as_ref()
                    .is_some_and(|name| !valid_name(name, MAX_DISPLAY_NAME_LENGTH))
                || group
                    .bio_override
                    .as_ref()
                    .is_some_and(|bio| bio.chars().count() > MAX_BIO_LENGTH)
                || group
                    .avatar_override
                    .as_ref()
                    .is_some_and(|avatar| avatar.len() > MAX_AVATAR_SIZE)
                || group.created_at > group.modified_at
                || group.modified_at > i64::MAX as u64
            {
                return Err(invalid());
            }
            if let Some(avatar) = &group.avatar_override {
                validate_avatar(avatar)?;
            }
            let members = unique(group.contact_ids.iter().map(String::as_str))?;
            let fields = unique(group.visible_fields.iter().map(String::as_str))?;
            if !members.is_subset(&ids) || !fields.is_subset(&own_fields) {
                return Err(invalid());
            }
        }
        let mut overrides = HashSet::new();
        for item in &self.overrides {
            if !ids.contains(item.contact_id.as_str())
                || !own_fields.contains(item.field_id.as_str())
                || !overrides.insert((&item.contact_id, &item.field_id))
            {
                return Err(invalid());
            }
        }
        Ok(contacts)
    }
}
