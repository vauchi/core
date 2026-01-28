// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Aha Moments - User experience milestones
//!
//! Tracks user progress through key moments that demonstrate Vauchi's value.
//! These "aha moments" help users understand the app before finding a second user.
//!
//! Feature file: features/aha_moments.feature

use serde::{Deserialize, Serialize};

/// Types of aha moments that can be triggered
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AhaMomentType {
    /// Shown when card creation completes
    CardCreationComplete,
    /// Shown on first edit (before having contacts)
    FirstEdit,
    /// Shown when first contact is added
    FirstContactAdded,
    /// Shown when receiving first update from a contact
    FirstUpdateReceived,
    /// Shown when first outbound update is delivered
    FirstOutboundDelivered,
}

impl AhaMomentType {
    /// Get the user-facing title for this moment
    pub fn title(&self) -> &'static str {
        match self {
            AhaMomentType::CardCreationComplete => "Your card is ready",
            AhaMomentType::FirstEdit => "Nice edit!",
            AhaMomentType::FirstContactAdded => "First contact added!",
            AhaMomentType::FirstUpdateReceived => "You received an update!",
            AhaMomentType::FirstOutboundDelivered => "Update delivered!",
        }
    }

    /// Get the user-facing message for this moment
    pub fn message(&self) -> &'static str {
        match self {
            AhaMomentType::CardCreationComplete => {
                "Anyone who scans your QR code will always have your latest info."
            }
            AhaMomentType::FirstEdit => {
                "If anyone had your card, they'd see this change instantly."
            }
            AhaMomentType::FirstContactAdded => {
                "When they update their card, you'll see the change automatically."
            }
            AhaMomentType::FirstUpdateReceived => {
                "This is the magic - they updated, you see it instantly."
            }
            AhaMomentType::FirstOutboundDelivered => "Your contacts now have your latest info.",
        }
    }

    /// Whether this moment should show an animation
    pub fn has_animation(&self) -> bool {
        match self {
            AhaMomentType::CardCreationComplete => true,
            AhaMomentType::FirstEdit => true, // ripple animation
            AhaMomentType::FirstContactAdded => true,
            AhaMomentType::FirstUpdateReceived => true,
            AhaMomentType::FirstOutboundDelivered => false,
        }
    }

    /// Get all aha moment types in order
    pub fn all() -> &'static [AhaMomentType] {
        &[
            AhaMomentType::CardCreationComplete,
            AhaMomentType::FirstEdit,
            AhaMomentType::FirstContactAdded,
            AhaMomentType::FirstUpdateReceived,
            AhaMomentType::FirstOutboundDelivered,
        ]
    }
}

/// An aha moment event to display to the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AhaMoment {
    /// The type of moment
    pub moment_type: AhaMomentType,
    /// Optional context (e.g., contact name)
    pub context: Option<String>,
}

impl AhaMoment {
    /// Create a new aha moment
    pub fn new(moment_type: AhaMomentType) -> Self {
        Self {
            moment_type,
            context: None,
        }
    }

    /// Create an aha moment with context
    pub fn with_context(moment_type: AhaMomentType, context: String) -> Self {
        Self {
            moment_type,
            context: Some(context),
        }
    }

    /// Get the title for display
    pub fn title(&self) -> &str {
        self.moment_type.title()
    }

    /// Get the message for display, potentially customized with context
    pub fn message(&self) -> String {
        match (&self.moment_type, &self.context) {
            (AhaMomentType::FirstContactAdded, Some(name)) => {
                format!(
                    "You now have {}'s card. When they update it, you'll see the change automatically.",
                    name
                )
            }
            (AhaMomentType::FirstUpdateReceived, Some(name)) => {
                format!("{} updated their card and you see it instantly!", name)
            }
            (AhaMomentType::FirstOutboundDelivered, Some(count)) => {
                format!("Your update was delivered to {} contacts.", count)
            }
            _ => self.moment_type.message().to_string(),
        }
    }

    /// Whether to show animation for this moment
    pub fn has_animation(&self) -> bool {
        self.moment_type.has_animation()
    }
}

/// Tracks which aha moments have been seen
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AhaMomentTracker {
    /// Set of seen moment types
    seen: std::collections::HashSet<AhaMomentType>,
}

impl AhaMomentTracker {
    /// Create a new tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a moment type has been seen
    pub fn has_seen(&self, moment_type: AhaMomentType) -> bool {
        self.seen.contains(&moment_type)
    }

    /// Mark a moment as seen
    pub fn mark_seen(&mut self, moment_type: AhaMomentType) {
        self.seen.insert(moment_type);
    }

    /// Check if a moment should be triggered (not yet seen)
    pub fn should_trigger(&self, moment_type: AhaMomentType) -> bool {
        !self.has_seen(moment_type)
    }

    /// Try to trigger a moment, returning it if not yet seen
    pub fn try_trigger(&mut self, moment_type: AhaMomentType) -> Option<AhaMoment> {
        if self.should_trigger(moment_type) {
            self.mark_seen(moment_type);
            Some(AhaMoment::new(moment_type))
        } else {
            None
        }
    }

    /// Try to trigger a moment with context
    pub fn try_trigger_with_context(
        &mut self,
        moment_type: AhaMomentType,
        context: String,
    ) -> Option<AhaMoment> {
        if self.should_trigger(moment_type) {
            self.mark_seen(moment_type);
            Some(AhaMoment::with_context(moment_type, context))
        } else {
            None
        }
    }

    /// Get count of seen moments
    pub fn seen_count(&self) -> usize {
        self.seen.len()
    }

    /// Get count of total possible moments
    pub fn total_count(&self) -> usize {
        AhaMomentType::all().len()
    }

    /// Reset all seen moments (for testing/debugging)
    pub fn reset(&mut self) {
        self.seen.clear();
    }

    /// Serialize to JSON for storage
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moment_type_all() {
        let all = AhaMomentType::all();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_moment_titles_not_empty() {
        for moment in AhaMomentType::all() {
            assert!(!moment.title().is_empty());
            assert!(!moment.message().is_empty());
        }
    }

    #[test]
    fn test_tracker_initial_state() {
        let tracker = AhaMomentTracker::new();
        assert_eq!(tracker.seen_count(), 0);
        assert_eq!(tracker.total_count(), 5);
    }

    #[test]
    fn test_tracker_should_trigger() {
        let tracker = AhaMomentTracker::new();
        assert!(tracker.should_trigger(AhaMomentType::CardCreationComplete));
        assert!(tracker.should_trigger(AhaMomentType::FirstEdit));
    }

    #[test]
    fn test_tracker_mark_seen() {
        let mut tracker = AhaMomentTracker::new();

        tracker.mark_seen(AhaMomentType::CardCreationComplete);

        assert!(tracker.has_seen(AhaMomentType::CardCreationComplete));
        assert!(!tracker.should_trigger(AhaMomentType::CardCreationComplete));
        assert!(tracker.should_trigger(AhaMomentType::FirstEdit));
    }

    #[test]
    fn test_tracker_try_trigger() {
        let mut tracker = AhaMomentTracker::new();

        // First trigger should succeed
        let moment = tracker.try_trigger(AhaMomentType::CardCreationComplete);
        assert!(moment.is_some());
        assert_eq!(
            moment.unwrap().moment_type,
            AhaMomentType::CardCreationComplete
        );

        // Second trigger should fail
        let moment = tracker.try_trigger(AhaMomentType::CardCreationComplete);
        assert!(moment.is_none());
    }

    #[test]
    fn test_tracker_try_trigger_with_context() {
        let mut tracker = AhaMomentTracker::new();

        let moment =
            tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Alice".to_string());

        assert!(moment.is_some());
        let m = moment.unwrap();
        assert_eq!(m.moment_type, AhaMomentType::FirstContactAdded);
        assert!(m.message().contains("Alice"));
    }

    #[test]
    fn test_moment_message_with_context() {
        let moment =
            AhaMoment::with_context(AhaMomentType::FirstOutboundDelivered, "5".to_string());

        assert!(moment.message().contains("5 contacts"));
    }

    #[test]
    fn test_tracker_serialization() {
        let mut tracker = AhaMomentTracker::new();
        tracker.mark_seen(AhaMomentType::CardCreationComplete);
        tracker.mark_seen(AhaMomentType::FirstEdit);

        let json = tracker.to_json().unwrap();
        let restored = AhaMomentTracker::from_json(&json).unwrap();

        assert!(restored.has_seen(AhaMomentType::CardCreationComplete));
        assert!(restored.has_seen(AhaMomentType::FirstEdit));
        assert!(!restored.has_seen(AhaMomentType::FirstContactAdded));
    }

    #[test]
    fn test_tracker_reset() {
        let mut tracker = AhaMomentTracker::new();
        tracker.mark_seen(AhaMomentType::CardCreationComplete);
        assert_eq!(tracker.seen_count(), 1);

        tracker.reset();
        assert_eq!(tracker.seen_count(), 0);
        assert!(tracker.should_trigger(AhaMomentType::CardCreationComplete));
    }
}
