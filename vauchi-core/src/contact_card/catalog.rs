// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Field Type Catalog
//!
//! Provides a unified, categorized view of all available field types
//! for the Add Field picker UI. Merges base field types with social
//! networks from `SocialNetworkRegistry`.

use crate::social::SocialNetworkRegistry;

/// Category grouping for field types in the picker UI.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldCategory {
    /// Phone, Email, Website
    Contact,
    /// Social network entries from the registry
    Social,
    /// Address, Birthday
    Personal,
    /// User-defined custom field
    Custom,
}

impl FieldCategory {
    /// Returns the display name for this category.
    pub fn display_name(&self) -> &'static str {
        match self {
            FieldCategory::Contact => "Contact",
            FieldCategory::Social => "Social",
            FieldCategory::Personal => "Personal",
            FieldCategory::Custom => "Custom",
        }
    }

    /// Returns all categories in display order.
    pub fn all() -> &'static [FieldCategory] {
        &[
            FieldCategory::Contact,
            FieldCategory::Social,
            FieldCategory::Personal,
            FieldCategory::Custom,
        ]
    }
}

/// A single entry in the field type catalog.
#[derive(Clone, Debug)]
pub struct CatalogEntry {
    /// Unique key for this entry (e.g., "phone", "email", "social:github").
    pub key: String,
    /// Human-readable display name (e.g., "Phone", "GitHub").
    pub display_name: String,
    /// Category this entry belongs to.
    pub category: FieldCategory,
    /// Optional icon identifier for UI rendering.
    pub icon: Option<String>,
}

/// Unified catalog of all available field types for the Add Field picker.
///
/// Combines base `FieldType` variants with social networks from
/// `SocialNetworkRegistry` into a single browsable list.
pub struct FieldTypeCatalog {
    entries: Vec<CatalogEntry>,
}

impl FieldTypeCatalog {
    /// Builds the catalog from a social network registry.
    pub fn new(registry: &SocialNetworkRegistry) -> Self {
        let mut entries = Vec::new();

        // Contact category
        entries.push(CatalogEntry {
            key: "phone".to_string(),
            display_name: "Phone".to_string(),
            category: FieldCategory::Contact,
            icon: None,
        });
        entries.push(CatalogEntry {
            key: "email".to_string(),
            display_name: "Email".to_string(),
            category: FieldCategory::Contact,
            icon: None,
        });
        entries.push(CatalogEntry {
            key: "website".to_string(),
            display_name: "Website".to_string(),
            category: FieldCategory::Contact,
            icon: None,
        });

        // Social category (from registry, sorted by display name)
        for network in registry.all() {
            entries.push(CatalogEntry {
                key: format!("social:{}", network.id()),
                display_name: network.display_name().to_string(),
                category: FieldCategory::Social,
                icon: network.icon().map(|s| s.to_string()),
            });
        }

        // Personal category
        entries.push(CatalogEntry {
            key: "address".to_string(),
            display_name: "Address".to_string(),
            category: FieldCategory::Personal,
            icon: None,
        });
        entries.push(CatalogEntry {
            key: "birthday".to_string(),
            display_name: "Birthday".to_string(),
            category: FieldCategory::Personal,
            icon: None,
        });

        // Custom category
        entries.push(CatalogEntry {
            key: "custom".to_string(),
            display_name: "Custom".to_string(),
            category: FieldCategory::Custom,
            icon: None,
        });

        FieldTypeCatalog { entries }
    }

    /// Returns all entries in the catalog.
    pub fn all(&self) -> &[CatalogEntry] {
        &self.entries
    }

    /// Returns entries filtered by category.
    pub fn by_category(&self, category: &FieldCategory) -> Vec<&CatalogEntry> {
        self.entries
            .iter()
            .filter(|e| &e.category == category)
            .collect()
    }

    /// Searches entries by display name (case-insensitive partial match).
    pub fn search(&self, query: &str) -> Vec<&CatalogEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| e.display_name.to_lowercase().contains(&query_lower))
            .collect()
    }

    /// Looks up an entry by key.
    pub fn get(&self, key: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    /// Returns the total number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
