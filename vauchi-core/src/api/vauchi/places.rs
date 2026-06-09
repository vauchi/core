// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Place + per-contact exchange-location API (ADR-051 contact annotations).
//!
//! Core-owned (ADR-021): the named-place vocabulary, proximity suggestion, and
//! a contact's recorded exchange location all live here. Frontends call this;
//! they never compute distances or own the vocabulary.
//!
//! Capture-at-exchange wiring (emit `Command::LocationRequest`, consume
//! `Event::LocationResult`) is app-layer orchestration landing in Phase 4 — at
//! the core layer, `set_exchange_location` is the entry point.

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;
use crate::contact::place::{ExchangeLocation, Place};

impl Vauchi {
    // === Named-place vocabulary ===

    /// Lists all named places, oldest first.
    pub fn list_places(&self) -> VauchiResult<Vec<Place>> {
        Ok(self.storage.places().list_places()?)
    }

    /// Creates a named place at the given coordinates. Rejects an empty name.
    pub fn create_named_place(
        &self,
        name: &str,
        latitude: f64,
        longitude: f64,
    ) -> VauchiResult<Place> {
        let name = name.trim();
        if name.is_empty() {
            return Err(VauchiError::InvalidState(
                "Place name cannot be empty".into(),
            ));
        }
        Ok(self
            .storage
            .places()
            .create_place(name, latitude, longitude)?)
    }

    /// Deletes a named place. Returns `true` if it existed. Contacts that
    /// referenced it keep their raw coordinates; only the name link dangles.
    pub fn delete_place(&self, place_id: &str) -> VauchiResult<bool> {
        Ok(self.storage.places().delete_place(place_id)?)
    }

    /// Suggests the nearest named place within the proximity radius of the
    /// given coordinates, or `None` — the proximity-autocomplete read used when
    /// recording a new exchange location (T2.4). Named-place match only.
    pub fn suggest_place_near(&self, latitude: f64, longitude: f64) -> VauchiResult<Option<Place>> {
        Ok(self.storage.places().find_place_near(latitude, longitude)?)
    }

    /// Returns the named place whose name matches (trimmed, case-insensitive),
    /// or `None`. Names are encrypted, so this scans the decrypted vocabulary.
    pub fn find_place_by_name(&self, name: &str) -> VauchiResult<Option<Place>> {
        let needle = name.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(None);
        }
        Ok(self
            .storage
            .places()
            .list_places()?
            .into_iter()
            .find(|p| p.name.to_lowercase() == needle))
    }

    // === Per-contact exchange location ===

    /// Records the coordinates a contact was met at (unnamed). Validates the
    /// contact exists. Overwrites any existing location.
    pub fn set_exchange_location(
        &self,
        contact_id: &str,
        latitude: f64,
        longitude: f64,
    ) -> VauchiResult<()> {
        if self.storage.contacts().load_contact(contact_id)?.is_none() {
            return Err(VauchiError::ContactNotFound(contact_id.to_string()));
        }
        let loc = ExchangeLocation {
            latitude,
            longitude,
            place_id: None,
        };
        self.storage.save_exchange_location(contact_id, &loc)?;
        Ok(())
    }

    /// Returns a contact's recorded exchange location, or `None`.
    pub fn exchange_location(&self, contact_id: &str) -> VauchiResult<Option<ExchangeLocation>> {
        Ok(self.storage.load_exchange_location(contact_id)?)
    }

    /// Clears a contact's exchange location.
    pub fn clear_exchange_location(&self, contact_id: &str) -> VauchiResult<()> {
        Ok(self.storage.delete_exchange_location(contact_id)?)
    }

    /// Names a contact's exchange location (retroactive naming,
    /// autocomplete-or-create): reuses the named place matching `name`
    /// (trimmed, case-insensitive) or creates one at the contact's recorded
    /// coordinates, then links the contact's location to it. Returns the place.
    ///
    /// Errors if the contact has no recorded location yet, or the name is empty.
    pub fn name_exchange_place(&self, contact_id: &str, name: &str) -> VauchiResult<Place> {
        let name = name.trim();
        if name.is_empty() {
            return Err(VauchiError::InvalidState(
                "Place name cannot be empty".into(),
            ));
        }
        let loc = self
            .storage
            .load_exchange_location(contact_id)?
            .ok_or_else(|| {
                VauchiError::InvalidState(format!("Contact {contact_id} has no exchange location"))
            })?;

        let place = match self.find_place_by_name(name)? {
            Some(existing) => existing,
            None => self
                .storage
                .places()
                .create_place(name, loc.latitude, loc.longitude)?,
        };

        let linked = ExchangeLocation {
            latitude: loc.latitude,
            longitude: loc.longitude,
            place_id: Some(place.id.clone()),
        };
        self.storage.save_exchange_location(contact_id, &linked)?;
        Ok(place)
    }
}
