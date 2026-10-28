// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Demo Contact - A simulated contact for solo users
//!
//! Provides a "Vauchi Tips" demo contact that demonstrates
//! the update flow for users who haven't exchanged with anyone yet.
//!
//! Feature file: features/demo_contact.feature

use serde::{Deserialize, Serialize};

use crate::onboarding::DemoContactState;

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
    /// Locale key of the short title; the app layer translates it (ADR-038)
    pub title_key: String,
    /// Locale key of the detailed content
    pub content_key: String,
}

/// Categories of demo tips
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
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
            title_key: "demo_tip.share.title".to_string(),
            content_key: "demo_tip.share.content".to_string(),
        },
        DemoTip {
            id: "tip-privacy".to_string(),
            category: DemoTipCategory::Privacy,
            title_key: "demo_tip.privacy.title".to_string(),
            content_key: "demo_tip.privacy.content".to_string(),
        },
        DemoTip {
            id: "tip-updates".to_string(),
            category: DemoTipCategory::Updates,
            title_key: "demo_tip.updates.title".to_string(),
            content_key: "demo_tip.updates.content".to_string(),
        },
        DemoTip {
            id: "tip-recovery".to_string(),
            category: DemoTipCategory::Recovery,
            title_key: "demo_tip.recovery.title".to_string(),
            content_key: "demo_tip.recovery.content".to_string(),
        },
        DemoTip {
            id: "tip-visibility".to_string(),
            category: DemoTipCategory::Contacts,
            title_key: "demo_tip.visibility.title".to_string(),
            content_key: "demo_tip.visibility.content".to_string(),
        },
        DemoTip {
            id: "tip-qr".to_string(),
            category: DemoTipCategory::GettingStarted,
            title_key: "demo_tip.qr.title".to_string(),
            content_key: "demo_tip.qr.content".to_string(),
        },
        DemoTip {
            id: "tip-edit".to_string(),
            category: DemoTipCategory::Updates,
            title_key: "demo_tip.edit.title".to_string(),
            content_key: "demo_tip.edit.content".to_string(),
        },
        DemoTip {
            id: "tip-multi-device".to_string(),
            category: DemoTipCategory::Features,
            title_key: "demo_tip.multi_device.title".to_string(),
            content_key: "demo_tip.multi_device.content".to_string(),
        },
    ]
}

impl DemoContactState {
    /// Create new demo contact state (active).
    ///
    /// `now` is the Unix-epoch timestamp to stamp into
    /// `last_update_timestamp`. Vauchi passes
    /// `self.clock.unix_seconds()`; tests pass any fixed value.
    pub fn new_active(now: u64) -> Self {
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

    /// Check if a demo update is due.
    ///
    /// `now` is the Unix-epoch timestamp the caller compares
    /// against `last_update_timestamp + DEMO_UPDATE_INTERVAL_SECS`.
    pub fn is_update_due(&self, now: u64) -> bool {
        if !self.is_active {
            return false;
        }
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

    /// Get the next tip and advance the index.
    ///
    /// `now` is stamped into `last_update_timestamp`.
    pub fn advance_to_next_tip(&mut self, now: u64) -> Option<DemoTip> {
        let tips = get_demo_tips();
        if tips.is_empty() {
            return None;
        }

        self.current_tip_index = (self.current_tip_index + 1) % tips.len();
        let tip = tips[self.current_tip_index].clone();

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
        tip_title_key: tip.title_key.clone(),
        tip_content_key: tip.content_key.clone(),
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
    /// Locale key of the current tip's title
    pub tip_title_key: String,
    /// Locale key of the current tip's content
    pub tip_content_key: String,
    /// Tip category
    pub tip_category: String,
}

// INLINE_TEST_REQUIRED: Tests use DemoContactState internals and time-dependent state via super::*
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn test_demo_tips_count_and_first_id() {
        let tips = get_demo_tips();
        assert_eq!(tips.len(), 8, "tip catalog must have exactly 8 entries");
        assert_eq!(tips[0].id, "tip-share");
        assert_eq!(tips[0].category, DemoTipCategory::GettingStarted);
        assert_eq!(tips[7].id, "tip-multi-device");
        assert_eq!(tips[7].category, DemoTipCategory::Features);
    }

    // @internal
    #[test]
    fn test_demo_tip_categories_has_distinct_values() {
        let tips = get_demo_tips();
        let categories: std::collections::HashSet<_> = tips.iter().map(|t| t.category).collect();
        // Tips span exactly 6 distinct categories. Pinning the count kills
        // mutations that drop a tip or change categories.
        assert_eq!(categories.len(), 6);
        assert!(categories.contains(&DemoTipCategory::GettingStarted));
        assert!(categories.contains(&DemoTipCategory::Privacy));
        assert!(categories.contains(&DemoTipCategory::Updates));
        assert!(categories.contains(&DemoTipCategory::Recovery));
        assert!(categories.contains(&DemoTipCategory::Contacts));
        assert!(categories.contains(&DemoTipCategory::Features));
    }

    // @internal
    #[test]
    fn test_demo_tip_category_all_returns_six_in_order() {
        let all = DemoTipCategory::all();
        assert_eq!(
            all,
            &[
                DemoTipCategory::GettingStarted,
                DemoTipCategory::Privacy,
                DemoTipCategory::Updates,
                DemoTipCategory::Recovery,
                DemoTipCategory::Contacts,
                DemoTipCategory::Features,
            ]
        );
    }

    // @internal
    #[test]
    fn test_demo_state_default() {
        let state = DemoContactState::default();
        assert!(!state.is_active);
        assert!(!state.was_dismissed);
        assert!(!state.auto_removed);
        assert_eq!(state.current_tip_index, 0);
        assert_eq!(state.update_count, 0);
    }

    // @internal
    #[test]
    fn test_demo_state_new_active() {
        let state = DemoContactState::new_active(1_700_000_000);
        assert!(state.is_active);
        assert!(!state.was_dismissed);
        assert!(!state.auto_removed);
        assert_eq!(state.current_tip_index, 0);
        assert_eq!(state.update_count, 0);
        assert_eq!(state.shown_tip_ids, vec!["tip-share".to_string()]);
        // Caller-provided `now` is stamped into `last_update_timestamp`
        // verbatim. Previously this assertion read `> 0` and relied on
        // `SystemTime::now` being non-zero.
        assert_eq!(state.last_update_timestamp, 1_700_000_000);
    }

    // @internal
    #[test]
    fn test_demo_state_current_tip_returns_first_for_index_zero() {
        let state = DemoContactState::new_active(0);
        let tip = state.current_tip().expect("expected Some");
        assert_eq!(tip.id, "tip-share");
        assert_eq!(tip.category, DemoTipCategory::GettingStarted);
    }

    // @internal
    #[test]
    fn test_demo_state_current_tip_wraps_modulo_len() {
        let mut state = DemoContactState::new_active(0);
        let len = get_demo_tips().len();
        state.current_tip_index = len * 3 + 2;
        let tip = state.current_tip().expect("expected Some");
        assert_eq!(tip.id, get_demo_tips()[2].id);
    }

    // @internal
    #[test]
    fn test_demo_state_advance_tip_index_is_one_after_first_advance() {
        let mut state = DemoContactState::new_active(0);
        assert_eq!(state.current_tip_index, 0);

        let tip = state.advance_to_next_tip(0).expect("expected Some");
        assert_eq!(
            state.current_tip_index, 1,
            "first advance must land on index 1"
        );
        assert_eq!(tip.id, get_demo_tips()[1].id);
        assert_eq!(state.update_count, 1);
        assert!(state.shown_tip_ids.contains(&get_demo_tips()[1].id));
    }

    // @internal
    #[test]
    fn test_demo_state_advance_tip_wraps_at_end() {
        let mut state = DemoContactState::new_active(0);
        let len = get_demo_tips().len();
        state.current_tip_index = len - 1;

        state.advance_to_next_tip(0).expect("expected Some");
        assert_eq!(
            state.current_tip_index, 0,
            "advancing from last must wrap to 0"
        );
    }

    // @internal
    #[test]
    fn test_demo_state_advance_increments_update_count_each_call() {
        let mut state = DemoContactState::new_active(0);
        for expected in 1..=3u32 {
            state.advance_to_next_tip(0);
            assert_eq!(state.update_count, expected);
        }
    }

    // @internal
    #[test]
    fn test_demo_state_advance_does_not_duplicate_shown_ids() {
        let mut state = DemoContactState::new_active(0);
        let len = get_demo_tips().len();
        for _ in 0..(len * 2) {
            state.advance_to_next_tip(0);
        }
        let mut sorted = state.shown_tip_ids.clone();
        sorted.sort();
        let mut dedup = sorted.clone();
        dedup.dedup();
        assert_eq!(sorted, dedup, "shown_tip_ids must contain no duplicates");
        assert_eq!(sorted.len(), len);
    }

    // @internal
    #[test]
    fn test_demo_state_dismiss_sets_flags() {
        let mut state = DemoContactState::new_active(0);
        state.dismiss();
        assert!(!state.is_active);
        assert!(state.was_dismissed);
        assert!(!state.auto_removed, "dismiss must not set auto_removed");
    }

    // @internal
    #[test]
    fn test_demo_state_auto_remove_sets_flags() {
        let mut state = DemoContactState::new_active(0);
        state.auto_remove();
        assert!(!state.is_active);
        assert!(state.auto_removed);
        assert!(
            !state.was_dismissed,
            "auto_remove must not set was_dismissed"
        );
    }

    // @internal
    #[test]
    fn test_demo_state_restore_clears_both_flags() {
        let mut state = DemoContactState::new_active(0);
        state.dismiss();
        state.auto_remove();
        assert!(state.was_dismissed);
        assert!(state.auto_removed);

        state.restore();
        assert!(state.is_active);
        assert!(!state.was_dismissed);
        assert!(!state.auto_removed, "restore must clear auto_removed");
    }

    // @internal
    #[test]
    fn test_demo_state_serialization_roundtrip_preserves_all_fields() {
        let mut state = DemoContactState::new_active(0);
        state.advance_to_next_tip(0);
        state.advance_to_next_tip(0);
        state.was_dismissed = true; // exercise non-default values

        let json = state.to_json().unwrap();
        let restored = DemoContactState::from_json(&json).unwrap();

        assert_eq!(state.is_active, restored.is_active);
        assert_eq!(state.was_dismissed, restored.was_dismissed);
        assert_eq!(state.auto_removed, restored.auto_removed);
        assert_eq!(state.current_tip_index, restored.current_tip_index);
        assert_eq!(state.last_update_timestamp, restored.last_update_timestamp);
        assert_eq!(state.shown_tip_ids, restored.shown_tip_ids);
        assert_eq!(state.update_count, restored.update_count);
    }

    // @internal
    #[test]
    fn test_generate_demo_card_uses_constants_and_tip_fields() {
        let tip = DemoTip {
            id: "x".to_string(),
            category: DemoTipCategory::Privacy,
            title_key: "Title".to_string(),
            content_key: "Content".to_string(),
        };
        let card = generate_demo_contact_card(&tip);
        assert_eq!(card.id, DEMO_CONTACT_ID);
        assert_eq!(card.display_name, DEMO_CONTACT_NAME);
        assert!(card.is_demo);
        assert_eq!(card.tip_title_key, "Title");
        assert_eq!(card.tip_content_key, "Content");
        assert_eq!(card.tip_category, "Privacy");
    }

    // @internal
    #[test]
    fn test_update_due_just_created_is_false() {
        let state = DemoContactState::new_active(0);
        assert!(!state.is_update_due(0), "fresh state cannot be due");
    }

    // @internal
    #[test]
    fn test_update_not_due_when_inactive() {
        let mut state = DemoContactState::new_active(0);
        state.dismiss();
        assert!(!state.is_update_due(0));
    }

    // @internal
    #[test]
    fn test_update_due_when_interval_elapsed() {
        // Caller-controlled time: pretend `now` is exactly one
        // INTERVAL past the last update — the boundary should fire
        // (`>=`). Kills the `>` ↔ `>=` mutation and the `+ INTERVAL`
        // → `* INTERVAL` arithmetic mutation in one shot.
        let mut state = DemoContactState::new_active(0);
        state.last_update_timestamp = 1; // ~1970
        let now = 1 + DEMO_UPDATE_INTERVAL_SECS;
        assert!(
            state.is_update_due(now),
            "interval should have elapsed when now == last + INTERVAL"
        );
    }

    // @internal
    #[test]
    fn test_update_due_boundary_off_by_one() {
        // last_update_timestamp = now − INTERVAL: due (>= boundary).
        // last_update_timestamp = now − INTERVAL + 1: not due.
        // Kills the `>=` ↔ `>` mutation. With the `now` parameter we
        // can pin a deterministic value rather than reading the wall
        // clock — same coverage, no flakiness window.
        let now = 1_700_000_000_u64;
        let mut state = DemoContactState::new_active(0);
        state.last_update_timestamp = now - DEMO_UPDATE_INTERVAL_SECS;
        assert!(state.is_update_due(now));

        state.last_update_timestamp = now - DEMO_UPDATE_INTERVAL_SECS + 1;
        assert!(!state.is_update_due(now));
    }

    // @internal
    #[test]
    fn test_tip_rotation_wraps_to_specific_index() {
        let mut state = DemoContactState::new_active(0);
        let tip_count = get_demo_tips().len();
        for _ in 0..tip_count {
            state.advance_to_next_tip(0);
        }
        assert_eq!(state.current_tip_index, 0);
        for _ in 0..3 {
            state.advance_to_next_tip(0);
        }
        assert_eq!(state.current_tip_index, 3);
    }

    // @internal
    #[test]
    fn test_demo_update_interval_is_two_hours() {
        // Pin the constant so a `2 * 60 * 60` mutation (e.g. + → *)
        // is caught.
        assert_eq!(DEMO_UPDATE_INTERVAL_SECS, 7200);
    }
}
