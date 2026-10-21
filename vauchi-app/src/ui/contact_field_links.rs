// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The links a contact's field opens outside the app (vauchi/private#709).

#[cfg(feature = "network-rustls")]
use vauchi_core::contact_card::{ContactField, FieldType, MapProvider, is_safe_url, map_uri};
#[cfg(feature = "network-rustls")]
use vauchi_core::exchange::capability::Platform;

#[cfg(feature = "network-rustls")]
use crate::i18n::{Locale, get_string};

/// One way to open a field; a field's first link opens on a tap of its row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLink {
    pub action_id: String,
    pub label: String,
    pub url: String,
}

/// The links `field` offers on `platform`, empty when it has nothing to open.
///
/// An address opens in the system's maps app and is also offered to the maps
/// the user may prefer: Google Maps where it is the platform's own, and the
/// two OpenStreetMap-based ones everywhere.
#[cfg(feature = "network-rustls")]
pub fn field_links(field: &ContactField, platform: &Platform, locale: Locale) -> Vec<FieldLink> {
    let link = |action_id: &str, label_key: &str, url: Option<String>| {
        url.filter(|url| is_safe_url(url)).map(|url| FieldLink {
            action_id: action_id.into(),
            label: get_string(locale, &format!("contact_detail.field_action.{label_key}")),
            url,
        })
    };
    let effective_type = match field.field_type() {
        FieldType::Custom => field.detect_value_type(),
        field_type => Some(field_type),
    };
    let label_key = match effective_type {
        Some(FieldType::Address) => {
            let system = match platform {
                Platform::Ios => MapProvider::AppleMaps,
                _ => MapProvider::System,
            };
            let mut providers = vec![("open", "show_on_map", system)];
            if *platform == Platform::Android {
                providers.push((
                    "google_maps",
                    "open_in_google_maps",
                    MapProvider::GoogleMaps,
                ));
            }
            providers.push((
                "openstreetmap",
                "open_in_openstreetmap",
                MapProvider::OpenStreetMap,
            ));
            providers.push((
                "organic_maps",
                "open_in_organic_maps",
                MapProvider::OrganicMaps,
            ));
            return providers
                .into_iter()
                .filter_map(|(id, key, provider)| link(id, key, map_uri(field.value(), provider)))
                .collect();
        }
        Some(FieldType::Phone) => "call",
        Some(FieldType::Email) => "send_email",
        Some(FieldType::Website) => "open_website",
        Some(FieldType::Social) => "open_profile",
        _ => return Vec::new(),
    };
    link(
        "open",
        label_key,
        field.to_uri(&crate::social::active_registry()),
    )
    .into_iter()
    .collect()
}
