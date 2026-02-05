// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! In-App Help System
//!
//! Provides FAQ content and help resources for the app.
//! Content is loaded from i18n locale files for localization.
//!
//! Feature file: features/in_app_help.feature (pending)

use serde::{Deserialize, Serialize};

use crate::i18n::{get_string, Locale};

/// Categories of help content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HelpCategory {
    /// Getting started and basic usage
    GettingStarted,
    /// Privacy and security questions
    Privacy,
    /// Account and device recovery
    Recovery,
    /// Contact management
    Contacts,
    /// Update synchronization
    Updates,
    /// General features
    Features,
}

impl HelpCategory {
    /// Get all help categories
    pub fn all() -> &'static [HelpCategory] {
        &[
            HelpCategory::GettingStarted,
            HelpCategory::Privacy,
            HelpCategory::Recovery,
            HelpCategory::Contacts,
            HelpCategory::Updates,
            HelpCategory::Features,
        ]
    }

    /// Get display name for this category
    pub fn display_name(&self) -> &'static str {
        match self {
            HelpCategory::GettingStarted => "Getting Started",
            HelpCategory::Privacy => "Privacy & Security",
            HelpCategory::Recovery => "Recovery",
            HelpCategory::Contacts => "Contacts",
            HelpCategory::Updates => "Updates",
            HelpCategory::Features => "Features",
        }
    }
}

/// A frequently asked question with answer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaqItem {
    /// Unique identifier
    pub id: String,
    /// Category this FAQ belongs to
    pub category: HelpCategory,
    /// The question
    pub question: String,
    /// The answer (may contain markdown)
    pub answer: String,
    /// Related FAQ IDs (for "see also")
    pub related: Vec<String>,
}

/// FAQ definition with i18n key prefix and metadata.
struct FaqDef {
    id: &'static str,
    i18n_key: &'static str,
    category: HelpCategory,
    related: &'static [&'static str],
}

/// All FAQ definitions — content comes from i18n keys.
const FAQ_DEFS: &[FaqDef] = &[
    FaqDef {
        id: "faq-phone-lost",
        i18n_key: "faq.phone_lost",
        category: HelpCategory::Recovery,
        related: &["faq-recovery-setup"],
    },
    FaqDef {
        id: "faq-recovery-setup",
        i18n_key: "faq.recovery_setup",
        category: HelpCategory::Recovery,
        related: &["faq-phone-lost"],
    },
    FaqDef {
        id: "faq-tracking",
        i18n_key: "faq.tracking",
        category: HelpCategory::Privacy,
        related: &["faq-data-storage", "faq-encryption"],
    },
    FaqDef {
        id: "faq-data-storage",
        i18n_key: "faq.data_storage",
        category: HelpCategory::Privacy,
        related: &["faq-encryption"],
    },
    FaqDef {
        id: "faq-encryption",
        i18n_key: "faq.encryption",
        category: HelpCategory::Privacy,
        related: &["faq-tracking"],
    },
    FaqDef {
        id: "faq-remove-contact",
        i18n_key: "faq.remove_contact",
        category: HelpCategory::Contacts,
        related: &["faq-block-contact"],
    },
    FaqDef {
        id: "faq-block-contact",
        i18n_key: "faq.block_contact",
        category: HelpCategory::Contacts,
        related: &["faq-remove-contact"],
    },
    FaqDef {
        id: "faq-how-updates-work",
        i18n_key: "faq.how_updates_work",
        category: HelpCategory::Updates,
        related: &["faq-offline-updates"],
    },
    FaqDef {
        id: "faq-offline-updates",
        i18n_key: "faq.offline_updates",
        category: HelpCategory::Updates,
        related: &["faq-how-updates-work"],
    },
    FaqDef {
        id: "faq-first-contact",
        i18n_key: "faq.first_contact",
        category: HelpCategory::GettingStarted,
        related: &["faq-why-in-person"],
    },
    FaqDef {
        id: "faq-why-in-person",
        i18n_key: "faq.why_in_person",
        category: HelpCategory::GettingStarted,
        related: &["faq-first-contact"],
    },
    FaqDef {
        id: "faq-visibility-labels",
        i18n_key: "faq.visibility_labels",
        category: HelpCategory::Features,
        related: &["faq-visibility-default"],
    },
    FaqDef {
        id: "faq-visibility-default",
        i18n_key: "faq.visibility_default",
        category: HelpCategory::Features,
        related: &["faq-visibility-labels"],
    },
    FaqDef {
        id: "faq-multiple-devices",
        i18n_key: "faq.multiple_devices",
        category: HelpCategory::Features,
        related: &[],
    },
];

/// Build a FaqItem from a definition and locale.
fn build_faq(def: &FaqDef, locale: Locale) -> FaqItem {
    let question_key = format!("{}.question", def.i18n_key);
    let answer_key = format!("{}.answer", def.i18n_key);
    FaqItem {
        id: def.id.to_string(),
        category: def.category,
        question: get_string(locale, &question_key),
        answer: get_string(locale, &answer_key),
        related: def.related.iter().map(|s| s.to_string()).collect(),
    }
}

/// Get all bundled FAQ items in the specified locale.
pub fn get_faqs_localized(locale: Locale) -> Vec<FaqItem> {
    FAQ_DEFS.iter().map(|def| build_faq(def, locale)).collect()
}

/// Get all bundled FAQ items (English).
pub fn get_faqs() -> Vec<FaqItem> {
    get_faqs_localized(Locale::English)
}

/// Get FAQs for a specific category
pub fn get_faqs_by_category(category: HelpCategory) -> Vec<FaqItem> {
    get_faqs()
        .into_iter()
        .filter(|faq| faq.category == category)
        .collect()
}

/// Get FAQs for a specific category in a locale.
pub fn get_faqs_by_category_localized(category: HelpCategory, locale: Locale) -> Vec<FaqItem> {
    get_faqs_localized(locale)
        .into_iter()
        .filter(|faq| faq.category == category)
        .collect()
}

/// Get a specific FAQ by ID
pub fn get_faq_by_id(id: &str) -> Option<FaqItem> {
    get_faqs().into_iter().find(|faq| faq.id == id)
}

/// Get a specific FAQ by ID in a locale.
pub fn get_faq_by_id_localized(id: &str, locale: Locale) -> Option<FaqItem> {
    get_faqs_localized(locale)
        .into_iter()
        .find(|faq| faq.id == id)
}

/// Search FAQs by keyword (searches question and answer)
pub fn search_faqs(query: &str) -> Vec<FaqItem> {
    let query_lower = query.to_lowercase();
    get_faqs()
        .into_iter()
        .filter(|faq| {
            faq.question.to_lowercase().contains(&query_lower)
                || faq.answer.to_lowercase().contains(&query_lower)
        })
        .collect()
}

/// Search FAQs by keyword in a locale.
pub fn search_faqs_localized(query: &str, locale: Locale) -> Vec<FaqItem> {
    let query_lower = query.to_lowercase();
    get_faqs_localized(locale)
        .into_iter()
        .filter(|faq| {
            faq.question.to_lowercase().contains(&query_lower)
                || faq.answer.to_lowercase().contains(&query_lower)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_init() {
        if !crate::i18n::is_initialized() {
            let locales_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales");
            let _ = crate::i18n::init(&locales_dir);
        }
    }

    #[test]
    fn test_all_categories_exist() {
        let categories = HelpCategory::all();
        assert_eq!(categories.len(), 6);
    }

    #[test]
    fn test_faqs_not_empty() {
        ensure_init();
        let faqs = get_faqs();
        assert!(!faqs.is_empty());
        assert!(faqs.len() >= 10, "Should have at least 10 FAQs");
    }

    #[test]
    fn test_faqs_cover_all_categories() {
        ensure_init();
        let faqs = get_faqs();
        for category in HelpCategory::all() {
            let count = faqs.iter().filter(|f| f.category == *category).count();
            assert!(count > 0, "Category {:?} should have FAQs", category);
        }
    }

    #[test]
    fn test_faq_content_not_empty() {
        ensure_init();
        for faq in get_faqs() {
            assert!(!faq.id.is_empty(), "FAQ should have ID");
            assert!(!faq.question.is_empty(), "FAQ should have question");
            assert!(!faq.answer.is_empty(), "FAQ should have answer");
        }
    }

    #[test]
    fn test_get_faqs_by_category() {
        ensure_init();
        let privacy_faqs = get_faqs_by_category(HelpCategory::Privacy);
        assert!(!privacy_faqs.is_empty());
        for faq in &privacy_faqs {
            assert_eq!(faq.category, HelpCategory::Privacy);
        }
    }

    #[test]
    fn test_get_faq_by_id() {
        ensure_init();
        let faq = get_faq_by_id("faq-phone-lost");
        assert!(faq.is_some());
        assert!(faq.unwrap().question.contains("lose my phone"));
    }

    #[test]
    fn test_get_faq_by_id_not_found() {
        ensure_init();
        let faq = get_faq_by_id("nonexistent");
        assert!(faq.is_none());
    }

    #[test]
    fn test_search_faqs() {
        ensure_init();
        let results = search_faqs("encrypt");
        assert!(!results.is_empty());

        let results = search_faqs("xyznonexistent123");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_faqs_case_insensitive() {
        ensure_init();
        let results_lower = search_faqs("privacy");
        let results_upper = search_faqs("PRIVACY");
        assert_eq!(results_lower.len(), results_upper.len());
    }

    #[test]
    fn test_related_faqs_exist() {
        ensure_init();
        let faqs = get_faqs();
        for faq in &faqs {
            for related_id in &faq.related {
                let related = faqs.iter().find(|f| &f.id == related_id);
                assert!(
                    related.is_some(),
                    "Related FAQ {} not found for {}",
                    related_id,
                    faq.id
                );
            }
        }
    }

    #[test]
    fn test_localized_faqs_german() {
        ensure_init();
        let faqs = get_faqs_localized(Locale::German);
        assert_eq!(faqs.len(), get_faqs().len());
        let phone_lost = faqs.iter().find(|f| f.id == "faq-phone-lost").unwrap();
        assert!(phone_lost.question.contains("Telefon"));
    }

    #[test]
    fn test_localized_faqs_french() {
        ensure_init();
        let faqs = get_faqs_localized(Locale::French);
        let phone_lost = faqs.iter().find(|f| f.id == "faq-phone-lost").unwrap();
        assert!(phone_lost.question.contains("telephone"));
    }

    #[test]
    fn test_localized_faqs_spanish() {
        ensure_init();
        let faqs = get_faqs_localized(Locale::Spanish);
        let phone_lost = faqs.iter().find(|f| f.id == "faq-phone-lost").unwrap();
        assert!(phone_lost.question.contains("telefono"));
    }

    #[test]
    fn test_localized_search() {
        ensure_init();
        // Search in German
        let results = search_faqs_localized("Verschluesselung", Locale::German);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_localized_faq_by_id() {
        ensure_init();
        let faq = get_faq_by_id_localized("faq-phone-lost", Locale::German);
        assert!(faq.is_some());
        assert!(faq.unwrap().question.contains("Telefon"));
    }
}
