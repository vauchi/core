// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Named places — an owner-private vocabulary of `(name → coordinates)` used to
//! recall *where* a contact was met (ADR-051 contact annotations).
//!
//! A `Place` pairs an owner-chosen name ("The Anchor Bar") with the coordinates
//! captured at an in-person exchange. Names AND coordinates are owner-private;
//! the whole record is encrypted at rest (see `storage/places.rs`). On a later
//! in-person exchange near a known place, the name is offered as a suggestion
//! (proximity autocomplete, T2.4) — matching is by named place only.

use serde::{Deserialize, Serialize};

/// Default proximity radius (metres) within which a new exchange location is
/// considered "the same place" as an existing named place (owner decision
/// 2026-06-07: full coords, ~100 m match).
pub const PLACE_MATCH_RADIUS_M: f64 = 100.0;

/// Mean Earth radius in metres (for the haversine great-circle distance).
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// An owner-named place: a reusable `(name → coordinates)` vocabulary entry.
///
/// The `data_encrypted` storage column holds [`PlaceData`] (name + coords); the
/// `id` and `created_at` are stored alongside in the clear for keying/ordering.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// UUID v4 identifier, stable across renames.
    pub id: String,
    /// Owner-chosen place name.
    pub name: String,
    /// Latitude in decimal degrees.
    pub latitude: f64,
    /// Longitude in decimal degrees.
    pub longitude: f64,
    /// Unix timestamp (seconds) when the place was first named.
    pub created_at: u64,
}

/// The encrypted-at-rest payload of a [`Place`] (everything owner-private).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceData {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// A contact's exchange location: the coordinates captured at an in-person
/// exchange ("where we met"), optionally linked to a named [`Place`] once the
/// owner names it (ADR-051). Stored encrypted per-contact; never shared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExchangeLocation {
    /// Latitude in decimal degrees.
    pub latitude: f64,
    /// Longitude in decimal degrees.
    pub longitude: f64,
    /// Linked named-place id, set when the owner names this location.
    #[serde(default)]
    pub place_id: Option<String>,
}

impl Place {
    /// Creates a new named place at the given coordinates.
    pub fn new(name: &str, latitude: f64, longitude: f64, now: u64) -> Self {
        Self {
            // TODO(PFC): ambient UUID in domain constructor — see 2026-07-06-core-pfc-violations C1
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            latitude,
            longitude,
            created_at: now,
        }
    }

    /// Great-circle distance in metres from this place to the given coordinates
    /// (haversine formula).
    pub fn distance_m(&self, latitude: f64, longitude: f64) -> f64 {
        haversine_m(self.latitude, self.longitude, latitude, longitude)
    }

    /// Whether the given coordinates fall within `radius_m` of this place.
    pub fn is_near(&self, latitude: f64, longitude: f64, radius_m: f64) -> bool {
        self.distance_m(latitude, longitude) <= radius_m
    }
}

/// Haversine great-circle distance in metres between two `(lat, lon)` points in
/// decimal degrees.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}
