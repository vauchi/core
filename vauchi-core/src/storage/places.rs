// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Place storage forwarders + per-contact exchange locations.
//!
//! Named-place CRUD now lives in [`PlaceStore`](super::PlaceStore); the methods
//! below forward to it while call sites migrate (Phase 2 of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`). Per-contact *exchange*
//! locations are a column on the `contacts` row, so they stay here pending the
//! ContactStore extraction.

use crate::contact::place::{ExchangeLocation, Place};

use super::{Storage, StorageError};

impl Storage {
    /// Forwards to [`PlaceStore::create_place`].
    pub fn create_place(
        &self,
        name: &str,
        latitude: f64,
        longitude: f64,
    ) -> Result<Place, StorageError> {
        self.places().create_place(name, latitude, longitude)
    }

    /// Forwards to [`PlaceStore::save_place`].
    pub fn save_place(&self, place: &Place) -> Result<(), StorageError> {
        self.places().save_place(place)
    }

    /// Forwards to [`PlaceStore::get_place`].
    pub fn get_place(&self, id: &str) -> Result<Option<Place>, StorageError> {
        self.places().get_place(id)
    }

    /// Forwards to [`PlaceStore::list_places`].
    pub fn list_places(&self) -> Result<Vec<Place>, StorageError> {
        self.places().list_places()
    }

    /// Forwards to [`PlaceStore::delete_place`].
    pub fn delete_place(&self, id: &str) -> Result<bool, StorageError> {
        self.places().delete_place(id)
    }

    /// Forwards to [`PlaceStore::find_place_near`].
    pub fn find_place_near(
        &self,
        latitude: f64,
        longitude: f64,
    ) -> Result<Option<Place>, StorageError> {
        self.places().find_place_near(latitude, longitude)
    }

    /// Saves a contact's exchange location, encrypted at rest with the storage
    /// key (ADR-051). Overwrites any existing value.
    pub fn save_exchange_location(
        &self,
        contact_id: &str,
        location: &ExchangeLocation,
    ) -> Result<(), StorageError> {
        let json =
            serde_json::to_vec(location).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let encrypted = crate::crypto::encrypt(&self.encryption_key, &json)
            .map_err(|e| StorageError::Encryption(format!("Encrypt exchange location: {e}")))?;
        let rows = self.conn.execute(
            "UPDATE contacts SET exchange_location_encrypted = ?1 WHERE id = ?2",
            rusqlite::params![encrypted, contact_id],
        )?;
        if rows == 0 {
            return Err(StorageError::NotFound("Contact not found".to_string()));
        }
        Ok(())
    }

    /// Loads a contact's exchange location, or `None` if unset.
    pub fn load_exchange_location(
        &self,
        contact_id: &str,
    ) -> Result<Option<ExchangeLocation>, StorageError> {
        let result = self.conn.query_row(
            "SELECT exchange_location_encrypted FROM contacts WHERE id = ?1",
            rusqlite::params![contact_id],
            |row| row.get::<_, Option<Vec<u8>>>(0),
        );
        match result {
            Ok(Some(encrypted)) => {
                let json =
                    crate::crypto::decrypt(&self.encryption_key, &encrypted).map_err(|e| {
                        StorageError::Encryption(format!("Decrypt exchange location: {e}"))
                    })?;
                let loc: ExchangeLocation = serde_json::from_slice(&json)
                    .map_err(|e| StorageError::InvalidData(e.to_string()))?;
                Ok(Some(loc))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                Err(StorageError::NotFound("Contact not found".to_string()))
            }
            Err(e) => Err(StorageError::from(e)),
        }
    }

    /// Lists every contact that has a recorded exchange location, as
    /// `(contact_id, location)` pairs. Used to gather per-contact locations for
    /// device sync.
    pub fn list_exchange_locations(&self) -> Result<Vec<(String, ExchangeLocation)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, exchange_location_encrypted FROM contacts
                 WHERE exchange_location_encrypted IS NOT NULL",
            )
            .map_err(StorageError::from)?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(StorageError::from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;
        rows.into_iter()
            .map(|(id, enc)| {
                let json = crate::crypto::decrypt(&self.encryption_key, &enc).map_err(|e| {
                    StorageError::Encryption(format!("Decrypt exchange location: {e}"))
                })?;
                let loc: ExchangeLocation = serde_json::from_slice(&json)
                    .map_err(|e| StorageError::InvalidData(e.to_string()))?;
                Ok((id, loc))
            })
            .collect()
    }

    /// Clears a contact's exchange location.
    pub fn delete_exchange_location(&self, contact_id: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE contacts SET exchange_location_encrypted = NULL WHERE id = ?1",
            rusqlite::params![contact_id],
        )?;
        Ok(())
    }
}
