// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage CRUD for named places (owner-private `name → coords` vocabulary).
//!
//! Places live in the `places` table (migration v50). Name AND coordinates are
//! owner-private, so the whole [`PlaceData`] payload is encrypted at rest with
//! the storage key (`data_encrypted` BLOB) — ADR-051. Proximity lookups load
//! and decrypt the (small) vocabulary and compute distances in Rust.

use crate::contact::place::{PLACE_MATCH_RADIUS_M, Place, PlaceData};

use super::{Storage, StorageError};

impl Storage {
    /// Creates and persists a new named place at the given coordinates.
    pub fn create_place(
        &self,
        name: &str,
        latitude: f64,
        longitude: f64,
    ) -> Result<Place, StorageError> {
        let place = Place::new(name, latitude, longitude, self.clock().unix_seconds());
        self.write_place(&place)?;
        Ok(place)
    }

    /// Inserts or replaces a place with a caller-supplied id (used by device
    /// sync to preserve ids across devices).
    pub fn save_place(&self, place: &Place) -> Result<(), StorageError> {
        self.write_place(place)
    }

    /// Returns the place with the given id, or `None`.
    pub fn get_place(&self, id: &str) -> Result<Option<Place>, StorageError> {
        let result = self.conn.query_row(
            "SELECT id, data_encrypted, created_at FROM places WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            },
        );
        match result {
            Ok(row) => Ok(Some(self.row_to_place(row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::from(e)),
        }
    }

    /// Lists all named places, oldest first.
    pub fn list_places(&self) -> Result<Vec<Place>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data_encrypted, created_at FROM places ORDER BY created_at ASC")
            .map_err(StorageError::from)?;
        let rows: Vec<(String, Vec<u8>, u64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            })
            .map_err(StorageError::from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;
        rows.into_iter().map(|row| self.row_to_place(row)).collect()
    }

    /// Deletes a place by id. Returns `true` if it existed.
    pub fn delete_place(&self, id: &str) -> Result<bool, StorageError> {
        let rows = self
            .conn
            .execute("DELETE FROM places WHERE id = ?1", rusqlite::params![id])
            .map_err(StorageError::from)?;
        Ok(rows > 0)
    }

    /// Returns the nearest named place within [`PLACE_MATCH_RADIUS_M`] of the
    /// given coordinates, or `None` — the basis for proximity autocomplete
    /// (T2.4). Matching is by named place only (no fuzzy unnamed coords).
    pub fn find_place_near(
        &self,
        latitude: f64,
        longitude: f64,
    ) -> Result<Option<Place>, StorageError> {
        let mut best: Option<(f64, Place)> = None;
        for place in self.list_places()? {
            let d = place.distance_m(latitude, longitude);
            if d <= PLACE_MATCH_RADIUS_M && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, place));
            }
        }
        Ok(best.map(|(_, p)| p))
    }

    /// Encrypts and writes a place row (insert-or-replace).
    fn write_place(&self, place: &Place) -> Result<(), StorageError> {
        let data = PlaceData {
            name: place.name.clone(),
            latitude: place.latitude,
            longitude: place.longitude,
        };
        let json =
            serde_json::to_vec(&data).map_err(|e| StorageError::Serialization(e.to_string()))?;
        let data_encrypted = crate::crypto::encrypt(&self.encryption_key, &json)
            .map_err(|e| StorageError::Encryption(format!("Encrypt place: {e}")))?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO places (id, data_encrypted, created_at)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![place.id, data_encrypted, place.created_at],
            )
            .map_err(StorageError::from)?;
        Ok(())
    }

    /// Decrypts a raw `(id, data_encrypted, created_at)` row into a `Place`.
    fn row_to_place(
        &self,
        (id, data_encrypted, created_at): (String, Vec<u8>, u64),
    ) -> Result<Place, StorageError> {
        let json = crate::crypto::decrypt(&self.encryption_key, &data_encrypted)
            .map_err(|e| StorageError::Encryption(format!("Decrypt place: {e}")))?;
        let data: PlaceData =
            serde_json::from_slice(&json).map_err(|e| StorageError::InvalidData(e.to_string()))?;
        Ok(Place {
            id,
            name: data.name,
            latitude: data.latitude,
            longitude: data.longitude,
            created_at,
        })
    }
}
