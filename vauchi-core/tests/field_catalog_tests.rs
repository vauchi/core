// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for FieldTypeCatalog
//!
//! Verifies the unified field type catalog merges base types with social
//! networks correctly and supports category browsing and search.
//!
//! Traces to: features/contacts_management.feature @contacts @view

use vauchi_core::contact_card::{FieldCategory, FieldTypeCatalog};
use vauchi_core::social::SocialNetworkRegistry;

fn default_catalog() -> FieldTypeCatalog {
    let registry = SocialNetworkRegistry::with_defaults();
    FieldTypeCatalog::new(&registry)
}

// @scenario: contacts_management :: Catalog includes base field types
#[test]
fn test_catalog_has_base_types() {
    let catalog = default_catalog();

    assert!(catalog.get("phone").is_some(), "phone must exist");
    assert!(catalog.get("email").is_some(), "email must exist");
    assert!(catalog.get("website").is_some(), "website must exist");
    assert!(catalog.get("address").is_some(), "address must exist");
    assert!(catalog.get("birthday").is_some(), "birthday must exist");
    assert!(catalog.get("custom").is_some(), "custom must exist");
}

// @scenario: contacts_management :: Catalog includes social networks
#[test]
fn test_catalog_has_social_networks() {
    let catalog = default_catalog();

    let github = catalog.get("social:github");
    assert!(github.is_some(), "GitHub should be in catalog");
    assert_eq!(github.unwrap().display_name, "GitHub");
    assert_eq!(github.unwrap().category, FieldCategory::Social);
}

// @scenario: contacts_management :: Catalog categories are correct
#[test]
fn test_catalog_category_assignment() {
    let catalog = default_catalog();

    assert_eq!(
        catalog.get("phone").unwrap().category,
        FieldCategory::Contact
    );
    assert_eq!(
        catalog.get("email").unwrap().category,
        FieldCategory::Contact
    );
    assert_eq!(
        catalog.get("website").unwrap().category,
        FieldCategory::Contact
    );
    assert_eq!(
        catalog.get("address").unwrap().category,
        FieldCategory::Personal
    );
    assert_eq!(
        catalog.get("birthday").unwrap().category,
        FieldCategory::Personal
    );
    assert_eq!(
        catalog.get("custom").unwrap().category,
        FieldCategory::Custom
    );
}

// @scenario: contacts_management :: Catalog by_category filters correctly
#[test]
fn test_catalog_by_category() {
    let catalog = default_catalog();

    let contact = catalog.by_category(&FieldCategory::Contact);
    assert_eq!(contact.len(), 3, "Contact: Phone, Email, Website");

    let personal = catalog.by_category(&FieldCategory::Personal);
    assert_eq!(personal.len(), 2, "Personal: Address, Birthday");

    let custom = catalog.by_category(&FieldCategory::Custom);
    assert_eq!(custom.len(), 1, "Custom: one entry");

    let social = catalog.by_category(&FieldCategory::Social);
    assert!(
        social.len() >= 30,
        "Should have many social networks from registry"
    );
}

// @scenario: contacts_management :: Catalog search finds matching entries
#[test]
fn test_catalog_search() {
    let catalog = default_catalog();

    let results = catalog.search("git");
    assert!(
        results.iter().any(|e| e.key == "social:github"),
        "Search for 'git' should find GitHub"
    );

    let results = catalog.search("phone");
    assert!(
        results.iter().any(|e| e.key == "phone"),
        "Search for 'phone' should find Phone"
    );

    let results = catalog.search("ZZZZNONEXISTENT");
    assert!(results.is_empty(), "Nonsense query should return nothing");
}

// @scenario: contacts_management :: Catalog search is case-insensitive
#[test]
fn test_catalog_search_case_insensitive() {
    let catalog = default_catalog();

    let lower = catalog.search("github");
    let upper = catalog.search("GITHUB");
    let mixed = catalog.search("GitHub");

    assert_eq!(lower.len(), upper.len());
    assert_eq!(lower.len(), mixed.len());
    assert!(!lower.is_empty());
}

// @scenario: contacts_management :: Catalog total count includes all entries
#[test]
fn test_catalog_len() {
    let catalog = default_catalog();
    // 3 contact + N social + 2 personal + 1 custom
    assert!(
        catalog.len() >= 6 + 30,
        "Catalog should have base types + social networks"
    );
    assert!(!catalog.is_empty());
}

// @scenario: contacts_management :: Empty registry produces minimal catalog
#[test]
fn test_catalog_empty_registry() {
    let registry = SocialNetworkRegistry::new();
    let catalog = FieldTypeCatalog::new(&registry);

    assert_eq!(
        catalog.len(),
        6,
        "Without social networks: 3 contact + 2 personal + 1 custom"
    );
    assert!(catalog.by_category(&FieldCategory::Social).is_empty());
}

// @scenario: contacts_management :: Category display names
#[test]
fn test_category_display_names() {
    assert_eq!(FieldCategory::Contact.display_name(), "Contact");
    assert_eq!(FieldCategory::Social.display_name(), "Social");
    assert_eq!(FieldCategory::Personal.display_name(), "Personal");
    assert_eq!(FieldCategory::Custom.display_name(), "Custom");
}

// @scenario: contacts_management :: All categories enumerated
#[test]
fn test_all_categories() {
    let all = FieldCategory::all();
    assert_eq!(all.len(), 4);
    assert_eq!(all[0], FieldCategory::Contact);
    assert_eq!(all[1], FieldCategory::Social);
    assert_eq!(all[2], FieldCategory::Personal);
    assert_eq!(all[3], FieldCategory::Custom);
}

// @scenario: contacts_management :: AppEngine exposes field type catalog
#[test]
fn test_app_engine_field_type_catalog() {
    use vauchi_app::ui::AppEngine;
    use vauchi_core::api::Vauchi;

    let vauchi = Vauchi::in_memory().unwrap();
    let engine = AppEngine::new(vauchi);
    let catalog = engine.field_type_catalog();

    // Should have base types + social networks
    assert!(
        catalog.len() >= 36,
        "Catalog should have at least 36 entries (6 base + 30 social)"
    );
    catalog.get("phone").expect("expected Some");
    catalog.get("email").expect("expected Some");
    assert!(!catalog.by_category(&FieldCategory::Social).is_empty());
}
