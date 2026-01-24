//! In-App Help System
//!
//! Provides FAQ content and help resources for the app.
//! Content is bundled for offline access.
//!
//! Feature file: features/in_app_help.feature (pending)

use serde::{Deserialize, Serialize};

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

/// Get all bundled FAQ items
pub fn get_faqs() -> Vec<FaqItem> {
    vec![
        // Recovery
        FaqItem {
            id: "faq-phone-lost".to_string(),
            category: HelpCategory::Recovery,
            question: "What happens if I lose my phone?".to_string(),
            answer: "Your identity can be recovered through Social Recovery. If you've set up recovery contacts, meet 3 or more of them in person. They can vouch for you, and your identity will be restored.\n\nTo set up recovery: Go to Settings > Recovery > Add Recovery Contacts.".to_string(),
            related: vec!["faq-recovery-setup".to_string()],
        },
        FaqItem {
            id: "faq-recovery-setup".to_string(),
            category: HelpCategory::Recovery,
            question: "How do I set up recovery contacts?".to_string(),
            answer: "Recovery contacts are people who can vouch for your identity if you lose access:\n\n1. Go to Settings > Recovery\n2. Tap 'Add Recovery Contact'\n3. Meet your trusted contact in person\n4. They'll scan your QR code to become a recovery contact\n\nWe recommend adding 5-7 people you trust and see regularly.".to_string(),
            related: vec!["faq-phone-lost".to_string()],
        },

        // Privacy
        FaqItem {
            id: "faq-tracking".to_string(),
            category: HelpCategory::Privacy,
            question: "Can someone track me through Vauchi?".to_string(),
            answer: "No. Vauchi is designed with privacy first:\n\n- No location data is ever collected or shared\n- Your card contains only what you choose to add\n- All data is end-to-end encrypted\n- The relay server can't read your content\n- We have no analytics or tracking\n\nYour contacts only see the fields you've shared with them.".to_string(),
            related: vec!["faq-data-storage".to_string(), "faq-encryption".to_string()],
        },
        FaqItem {
            id: "faq-data-storage".to_string(),
            category: HelpCategory::Privacy,
            question: "What data is stored where?".to_string(),
            answer: "Your data lives in three places:\n\n**On your device:**\n- Your identity (private keys)\n- Your contact card\n- Your contacts' cards\n- All settings\n\n**On the relay server (encrypted):**\n- Messages waiting to be delivered\n- Never readable by us\n\n**Nowhere else.**\n\nWe never have access to your unencrypted data.".to_string(),
            related: vec!["faq-encryption".to_string()],
        },
        FaqItem {
            id: "faq-encryption".to_string(),
            category: HelpCategory::Privacy,
            question: "How is my data protected?".to_string(),
            answer: "Vauchi uses multiple layers of encryption:\n\n**End-to-end encryption:**\nYour data is encrypted on your device before it leaves. Only you and your intended recipients can read it.\n\n**Double Ratchet protocol:**\nEach message uses a unique key. Even if one key is compromised, other messages remain secure.\n\n**Local storage encryption:**\nData on your device is encrypted with your device key.\n\nWe use the same cryptographic standards as Signal.".to_string(),
            related: vec!["faq-tracking".to_string()],
        },

        // Contacts
        FaqItem {
            id: "faq-remove-contact".to_string(),
            category: HelpCategory::Contacts,
            question: "How do I remove a contact?".to_string(),
            answer: "To remove a contact:\n\n1. Go to your Contacts list\n2. Tap on the contact\n3. Tap the menu (three dots)\n4. Select 'Remove Contact'\n\nNote: They will still have your card, but won't receive your future updates. Consider asking them to remove you too.".to_string(),
            related: vec!["faq-block-contact".to_string()],
        },
        FaqItem {
            id: "faq-block-contact".to_string(),
            category: HelpCategory::Contacts,
            question: "Can I block someone?".to_string(),
            answer: "Yes. Blocking a contact:\n\n1. Removes them from your contact list\n2. Stops sending them your updates\n3. Ignores any updates from them\n\nTo block: Tap the contact > Menu > Block Contact\n\nThey won't be notified that they're blocked.".to_string(),
            related: vec!["faq-remove-contact".to_string()],
        },

        // Updates
        FaqItem {
            id: "faq-how-updates-work".to_string(),
            category: HelpCategory::Updates,
            question: "How do updates reach my contacts?".to_string(),
            answer: "When you edit your card:\n\n1. The change is encrypted on your device\n2. Sent to the relay server (still encrypted)\n3. Delivered to your contacts' devices\n4. Decrypted only on their devices\n\nUpdates are typically delivered within seconds. If a contact is offline, updates wait on the relay and are delivered when they reconnect.".to_string(),
            related: vec!["faq-offline-updates".to_string()],
        },
        FaqItem {
            id: "faq-offline-updates".to_string(),
            category: HelpCategory::Updates,
            question: "What happens when I'm offline?".to_string(),
            answer: "Vauchi handles offline gracefully:\n\n**When you go offline:**\n- Your edits are saved locally\n- They'll be sent when you reconnect\n\n**When contacts are offline:**\n- Updates queue on the relay\n- Delivered when they reconnect\n- No data loss\n\nThe relay stores encrypted messages for up to 30 days.".to_string(),
            related: vec!["faq-how-updates-work".to_string()],
        },

        // Getting Started
        FaqItem {
            id: "faq-first-contact".to_string(),
            category: HelpCategory::GettingStarted,
            question: "How do I add my first contact?".to_string(),
            answer: "To add a contact, you need to meet in person:\n\n1. Tap the Exchange tab\n2. Show your QR code to the other person\n3. They scan it with their Vauchi app\n4. You scan their QR code\n5. Done! You're connected.\n\nThis in-person requirement prevents spam and ensures you only connect with people you've actually met.".to_string(),
            related: vec!["faq-why-in-person".to_string()],
        },
        FaqItem {
            id: "faq-why-in-person".to_string(),
            category: HelpCategory::GettingStarted,
            question: "Why do I need to meet in person?".to_string(),
            answer: "The in-person QR exchange is a core privacy feature:\n\n- **No spam**: You can't be added by strangers\n- **Verified identity**: You know who you're connecting with\n- **No social graph**: We can't see who you know\n- **Physical trust**: Cryptographic trust starts with physical presence\n\nThis is similar to how Signal verifies contacts in person.".to_string(),
            related: vec!["faq-first-contact".to_string()],
        },

        // Features
        FaqItem {
            id: "faq-visibility-labels".to_string(),
            category: HelpCategory::Features,
            question: "What are visibility labels?".to_string(),
            answer: "Visibility labels let you control who sees what:\n\n1. Create labels like 'Family', 'Work', 'Friends'\n2. Assign contacts to labels\n3. Mark fields as visible to specific labels\n\nExample: Your home address shows to 'Family' but not 'Work'.\n\nTo set up: Go to your card > tap a field > Set Visibility.".to_string(),
            related: vec!["faq-visibility-default".to_string()],
        },
        FaqItem {
            id: "faq-visibility-default".to_string(),
            category: HelpCategory::Features,
            question: "What's the default visibility?".to_string(),
            answer: "By default, all fields are visible to all contacts.\n\nTo restrict a field:\n1. Edit your card\n2. Tap the field\n3. Tap 'Visibility'\n4. Choose which labels can see it\n\nYou can also set per-contact overrides for fine-grained control.".to_string(),
            related: vec!["faq-visibility-labels".to_string()],
        },
        FaqItem {
            id: "faq-multiple-devices".to_string(),
            category: HelpCategory::Features,
            question: "Can I use Vauchi on multiple devices?".to_string(),
            answer: "Yes! Link your devices in Settings:\n\n1. On your new device, install Vauchi\n2. Choose 'Link to Existing Account'\n3. On your original device, go to Settings > Devices > Link Device\n4. Scan the QR code on your new device\n\nYour contacts, cards, and settings will sync across devices.".to_string(),
            related: vec![],
        },
    ]
}

/// Get FAQs for a specific category
pub fn get_faqs_by_category(category: HelpCategory) -> Vec<FaqItem> {
    get_faqs()
        .into_iter()
        .filter(|faq| faq.category == category)
        .collect()
}

/// Get a specific FAQ by ID
pub fn get_faq_by_id(id: &str) -> Option<FaqItem> {
    get_faqs().into_iter().find(|faq| faq.id == id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_categories_exist() {
        let categories = HelpCategory::all();
        assert_eq!(categories.len(), 6);
    }

    #[test]
    fn test_faqs_not_empty() {
        let faqs = get_faqs();
        assert!(!faqs.is_empty());
        assert!(faqs.len() >= 10, "Should have at least 10 FAQs");
    }

    #[test]
    fn test_faqs_cover_all_categories() {
        let faqs = get_faqs();
        for category in HelpCategory::all() {
            let count = faqs.iter().filter(|f| f.category == *category).count();
            assert!(count > 0, "Category {:?} should have FAQs", category);
        }
    }

    #[test]
    fn test_faq_content_not_empty() {
        for faq in get_faqs() {
            assert!(!faq.id.is_empty(), "FAQ should have ID");
            assert!(!faq.question.is_empty(), "FAQ should have question");
            assert!(!faq.answer.is_empty(), "FAQ should have answer");
        }
    }

    #[test]
    fn test_get_faqs_by_category() {
        let privacy_faqs = get_faqs_by_category(HelpCategory::Privacy);
        assert!(!privacy_faqs.is_empty());
        for faq in &privacy_faqs {
            assert_eq!(faq.category, HelpCategory::Privacy);
        }
    }

    #[test]
    fn test_get_faq_by_id() {
        let faq = get_faq_by_id("faq-phone-lost");
        assert!(faq.is_some());
        assert!(faq.unwrap().question.contains("lose my phone"));
    }

    #[test]
    fn test_get_faq_by_id_not_found() {
        let faq = get_faq_by_id("nonexistent");
        assert!(faq.is_none());
    }

    #[test]
    fn test_search_faqs() {
        let results = search_faqs("encrypt");
        assert!(!results.is_empty());

        let results = search_faqs("xyznonexistent123");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_faqs_case_insensitive() {
        let results_lower = search_faqs("privacy");
        let results_upper = search_faqs("PRIVACY");
        assert_eq!(results_lower.len(), results_upper.len());
    }

    #[test]
    fn test_related_faqs_exist() {
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
}
