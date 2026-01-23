//! Demo Contact - A simulated contact for solo users
//!
//! Provides a "Vauchi Tips" demo contact that demonstrates
//! the update flow for users who haven't exchanged with anyone yet.
//!
//! Feature file: features/demo_contact.feature

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Demo contact display name
pub const DEMO_CONTACT_NAME: &str = "Vauchi Tips";

/// Demo contact ID (fixed)
pub const DEMO_CONTACT_ID: &str = "demo-vauchi-tips";

/// Interval between demo updates (in seconds) - 2 hours
pub const DEMO_UPDATE_INTERVAL_SECS: u64 = 2 * 60 * 60;

/// Demo tip content that rotates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoTip {
    /// Unique ID for this tip
    pub id: String,
    /// Category of the tip
    pub category: DemoTipCategory,
    /// Short title
    pub title: String,
    /// Detailed content
    pub content: String,
}

/// Categories of demo tips
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DemoTipCategory {
    GettingStarted,
    Privacy,
    Updates,
    Recovery,
    Contacts,
    Features,
}

impl DemoTipCategory {
    /// Get all categories
    pub fn all() -> &'static [DemoTipCategory] {
        &[
            DemoTipCategory::GettingStarted,
            DemoTipCategory::Privacy,
            DemoTipCategory::Updates,
            DemoTipCategory::Recovery,
            DemoTipCategory::Contacts,
            DemoTipCategory::Features,
        ]
    }
}

/// Get all demo tips
pub fn get_demo_tips() -> Vec<DemoTip> {
    vec![
        DemoTip {
            id: "tip-share".to_string(),
            category: DemoTipCategory::GettingStarted,
            title: "Share Your Card".to_string(),
            content: "Tap the Exchange tab and show your QR code to share your contact card. The other person scans it, and you're connected!".to_string(),
        },
        DemoTip {
            id: "tip-privacy".to_string(),
            category: DemoTipCategory::Privacy,
            title: "Your Privacy Matters".to_string(),
            content: "Your data never touches our servers. Everything is encrypted end-to-end, and only people you've exchanged with can see your info.".to_string(),
        },
        DemoTip {
            id: "tip-updates".to_string(),
            category: DemoTipCategory::Updates,
            title: "Automatic Updates".to_string(),
            content: "When you update your card, everyone who has it sees the change automatically. No need to send it again!".to_string(),
        },
        DemoTip {
            id: "tip-recovery".to_string(),
            category: DemoTipCategory::Recovery,
            title: "Social Recovery".to_string(),
            content: "Lost your phone? Your contacts can vouch for you. Meet 3+ trusted contacts in person, and they'll help restore your identity.".to_string(),
        },
        DemoTip {
            id: "tip-visibility".to_string(),
            category: DemoTipCategory::Contacts,
            title: "Control Who Sees What".to_string(),
            content: "Use visibility labels to control what different groups see. Family might see your home address, while colleagues only see work info.".to_string(),
        },
        DemoTip {
            id: "tip-qr".to_string(),
            category: DemoTipCategory::GettingStarted,
            title: "In-Person Exchange".to_string(),
            content: "Vauchi requires you to be physically present to exchange. This prevents spam and ensures you only connect with real people you've met.".to_string(),
        },
        DemoTip {
            id: "tip-edit".to_string(),
            category: DemoTipCategory::Updates,
            title: "Edit Anytime".to_string(),
            content: "Changed your phone number? Just update it in your card. Everyone who has your card will see the new number automatically.".to_string(),
        },
        DemoTip {
            id: "tip-multi-device".to_string(),
            category: DemoTipCategory::Features,
            title: "Multiple Devices".to_string(),
            content: "Use Vauchi on your phone and tablet. Link devices in Settings and your data stays in sync across all of them.".to_string(),
        },
    ]
}

/// State of the demo contact
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DemoContactState {
    /// Whether the demo contact is active
    pub is_active: bool,
    /// Whether it was manually dismissed
    pub was_dismissed: bool,
    /// Whether it was auto-removed after first real exchange
    pub auto_removed: bool,
    /// Current tip index (which tip is being shown)
    pub current_tip_index: usize,
    /// Timestamp of last update (Unix epoch seconds)
    pub last_update_timestamp: u64,
    /// History of shown tip IDs
    pub shown_tip_ids: Vec<String>,
    /// Number of updates sent
    pub update_count: u32,
}

impl DemoContactState {
    /// Create new demo contact state (active)
    pub fn new_active() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        Self {
            is_active: true,
            was_dismissed: false,
            auto_removed: false,
            current_tip_index: 0,
            last_update_timestamp: now,
            shown_tip_ids: vec!["tip-share".to_string()],
            update_count: 0,
        }
    }

    /// Check if a demo update is due
    pub fn is_update_due(&self) -> bool {
        if !self.is_active {
            return false;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        now >= self.last_update_timestamp + DEMO_UPDATE_INTERVAL_SECS
    }

    /// Get the current tip
    pub fn current_tip(&self) -> Option<DemoTip> {
        let tips = get_demo_tips();
        if tips.is_empty() {
            return None;
        }
        let index = self.current_tip_index % tips.len();
        Some(tips[index].clone())
    }

    /// Get the next tip and advance the index
    pub fn advance_to_next_tip(&mut self) -> Option<DemoTip> {
        let tips = get_demo_tips();
        if tips.is_empty() {
            return None;
        }

        self.current_tip_index = (self.current_tip_index + 1) % tips.len();
        let tip = tips[self.current_tip_index].clone();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        self.last_update_timestamp = now;
        self.update_count += 1;

        if !self.shown_tip_ids.contains(&tip.id) {
            self.shown_tip_ids.push(tip.id.clone());
        }

        Some(tip)
    }

    /// Dismiss the demo contact
    pub fn dismiss(&mut self) {
        self.is_active = false;
        self.was_dismissed = true;
    }

    /// Auto-remove after first real exchange
    pub fn auto_remove(&mut self) {
        self.is_active = false;
        self.auto_removed = true;
    }

    /// Restore the demo contact
    pub fn restore(&mut self) {
        self.is_active = true;
        self.was_dismissed = false;
        self.auto_removed = false;
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Generate a demo contact card
pub fn generate_demo_contact_card(tip: &DemoTip) -> DemoContactCard {
    DemoContactCard {
        id: DEMO_CONTACT_ID.to_string(),
        display_name: DEMO_CONTACT_NAME.to_string(),
        is_demo: true,
        tip_title: tip.title.clone(),
        tip_content: tip.content.clone(),
        tip_category: format!("{:?}", tip.category),
    }
}

/// Demo contact card representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoContactCard {
    /// Contact ID
    pub id: String,
    /// Display name
    pub display_name: String,
    /// Flag indicating this is a demo
    pub is_demo: bool,
    /// Current tip title
    pub tip_title: String,
    /// Current tip content
    pub tip_content: String,
    /// Tip category
    pub tip_category: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demo_tips_not_empty() {
        let tips = get_demo_tips();
        assert!(!tips.is_empty());
        assert!(tips.len() >= 5);
    }

    #[test]
    fn test_demo_tip_categories() {
        let tips = get_demo_tips();
        let categories: std::collections::HashSet<_> = tips.iter().map(|t| t.category).collect();

        // Should have multiple categories
        assert!(categories.len() >= 3);
    }

    #[test]
    fn test_demo_state_default() {
        let state = DemoContactState::default();
        assert!(!state.is_active);
        assert!(!state.was_dismissed);
        assert!(!state.auto_removed);
    }

    #[test]
    fn test_demo_state_new_active() {
        let state = DemoContactState::new_active();
        assert!(state.is_active);
        assert!(!state.was_dismissed);
        assert!(state.last_update_timestamp > 0);
    }

    #[test]
    fn test_demo_state_current_tip() {
        let state = DemoContactState::new_active();
        let tip = state.current_tip();
        assert!(tip.is_some());
    }

    #[test]
    fn test_demo_state_advance_tip() {
        let mut state = DemoContactState::new_active();
        let initial_index = state.current_tip_index;

        let tip = state.advance_to_next_tip();
        assert!(tip.is_some());
        assert_ne!(state.current_tip_index, initial_index);
        assert_eq!(state.update_count, 1);
    }

    #[test]
    fn test_demo_state_dismiss() {
        let mut state = DemoContactState::new_active();
        state.dismiss();

        assert!(!state.is_active);
        assert!(state.was_dismissed);
    }

    #[test]
    fn test_demo_state_auto_remove() {
        let mut state = DemoContactState::new_active();
        state.auto_remove();

        assert!(!state.is_active);
        assert!(state.auto_removed);
    }

    #[test]
    fn test_demo_state_restore() {
        let mut state = DemoContactState::new_active();
        state.dismiss();
        state.restore();

        assert!(state.is_active);
        assert!(!state.was_dismissed);
    }

    #[test]
    fn test_demo_state_serialization() {
        let mut state = DemoContactState::new_active();
        state.advance_to_next_tip();

        let json = state.to_json().unwrap();
        let restored = DemoContactState::from_json(&json).unwrap();

        assert_eq!(state.is_active, restored.is_active);
        assert_eq!(state.current_tip_index, restored.current_tip_index);
        assert_eq!(state.update_count, restored.update_count);
    }

    #[test]
    fn test_generate_demo_card() {
        let tips = get_demo_tips();
        let card = generate_demo_contact_card(&tips[0]);

        assert_eq!(card.id, DEMO_CONTACT_ID);
        assert_eq!(card.display_name, DEMO_CONTACT_NAME);
        assert!(card.is_demo);
        assert!(!card.tip_title.is_empty());
        assert!(!card.tip_content.is_empty());
    }

    #[test]
    fn test_update_due_initial() {
        let state = DemoContactState::new_active();
        // Just created, update not due yet
        assert!(!state.is_update_due());
    }

    #[test]
    fn test_update_not_due_when_inactive() {
        let mut state = DemoContactState::new_active();
        state.dismiss();
        assert!(!state.is_update_due());
    }

    #[test]
    fn test_tip_rotation_wraps() {
        let mut state = DemoContactState::new_active();
        let tip_count = get_demo_tips().len();

        // Advance through all tips and verify it wraps
        for _ in 0..tip_count + 2 {
            state.advance_to_next_tip();
        }

        assert!(state.current_tip_index < tip_count);
    }
}
