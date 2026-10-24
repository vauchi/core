// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Contact-specific vCard adapters and compatible card data exports.
//!
//! The data model lives in `crate::card` so contacts do not depend on the
//! adapters that serialize contact lifecycle metadata.

pub mod vcard;
pub mod vcard_import;

pub use crate::card::catalog;
pub use crate::card::{
    CURRENT_CARD_SCHEMA_VERSION, CatalogEntry, ContactAction, ContactCard, ContactCardError,
    ContactField, FieldCategory, FieldType, FieldTypeCatalog, MAX_AVATAR_SIZE, MAX_BIO_LENGTH,
    MAX_CARD_SIZE_BYTES, MAX_DISPLAY_NAME_LENGTH, MAX_FIELDS, MapProvider, ValidationError,
    is_allowed_scheme, is_blocked_scheme, is_safe_url, is_valid_email, is_valid_phone,
    is_valid_relay_url, map_uri, normalize_avatar, validate_social_username, validate_value,
};
#[cfg(feature = "testing")]
pub use crate::card::{field, uri};
