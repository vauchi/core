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

use crate::types::{AhaMomentTracker, AhaMomentType};

impl AhaMomentType {
    /// Get the user-facing title for this moment (English).
    pub fn title(&self) -> &'static str {
        match self {
            AhaMomentType::CardCreationComplete => "Your card is ready",
            AhaMomentType::FirstEdit => "Nice edit!",
            AhaMomentType::FirstContactAdded => "First contact added!",
            AhaMomentType::FirstUpdateReceived => "You received an update!",
            AhaMomentType::FirstOutboundDelivered => "Update delivered!",
            AhaMomentType::FirstFieldEdit => "Field updated!",
            AhaMomentType::ThreeContactsReached => "Growing network!",
            AhaMomentType::DeviceLinked => "Device linked!",
        }
    }

    /// Get the user-facing message for this moment (English).
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
            AhaMomentType::FirstFieldEdit => {
                "Your contacts will see this change the next time the app syncs."
            }
            AhaMomentType::ThreeContactsReached => "Three contacts! Your network is taking shape.",
            AhaMomentType::DeviceLinked => {
                "Your contacts are now synced across devices. Changes appear everywhere."
            }
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
            AhaMomentType::FirstFieldEdit => true, // ripple animation
            AhaMomentType::ThreeContactsReached => true,
            AhaMomentType::DeviceLinked => true,
        }
    }
}

// Localized methods (title_localized, message_localized) moved to vauchi-app.
// English title() and message() methods remain above.

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

impl AhaMomentTracker {
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
}

// INLINE_TEST_REQUIRED: tests access private AhaMoment internals and localized title/message methods
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moment_type_all() {
        let all = AhaMomentType::all();
        assert_eq!(all.len(), 8);
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
        assert_eq!(tracker.total_count(), 8);
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

        let moment = tracker.try_trigger(AhaMomentType::CardCreationComplete);
        assert!(moment.is_some(), "expected Some value");
        assert_eq!(
            moment.unwrap().moment_type,
            AhaMomentType::CardCreationComplete
        );

        let moment = tracker.try_trigger(AhaMomentType::CardCreationComplete);
        assert!(moment.is_none());
    }

    #[test]
    fn test_tracker_try_trigger_with_context() {
        let mut tracker = AhaMomentTracker::new();

        let moment =
            tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Alice".to_string());

        assert!(moment.is_some(), "expected Some value");
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

    // Localized tests moved to vauchi-app (they depend on i18n).
}
