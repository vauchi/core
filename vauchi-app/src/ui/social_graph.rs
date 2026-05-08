// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social graph engine — contacts grouped by trust level (ADR-034).
//!
//! Note (2026-05-08, ADR-040 follow-up): "social graph" here means a
//! trust-level-grouped view of the user's OWN contacts — not the
//! community-scoring system ADR-040 retired. No cross-user
//! relationships are stored, no validator records, no Principle 1
//! violation. The naming is a tripwire post ADR-040; the substance
//! is compliant. Renaming the file would touch all engine
//! registrations — left as future maintenance work, this comment
//! documents the distinction inline so a future reader greps once
//! and stops.
//!
//! Renders a network summary (totals, trusted %, cautions, group count)
//! plus per-trust-level contact lists. The user can filter to a single
//! trust level via a `ToggleList` chip row. Tapping a contact emits
//! `OpenContact` so the AppEngine routes to ContactDetail.
//!
//! Trust levels mirror `vauchi_core::contact::TrustLevel` (ADR-034):
//! `Cautious < Standard < High < Verified`. Cautious contacts surface
//! first because they need re-verification — surfacing them lets the
//! user see attention-needed contacts at the top of the list.

use crate::ui::*;

/// Display-friendly trust level — mirrors `vauchi_core::contact::TrustLevel`
/// but lives in the UI layer so engines aren't coupled to the core
/// crypto enum's variant order.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum SocialTrustLevel {
    /// Recovered identity — re-verify before trusting again (highest UI priority).
    Cautious,
    /// Default level for any new contact.
    Standard,
    /// Verified via proximity (NFC, BLE, etc.).
    High,
    /// Verified in person via fingerprint comparison.
    Verified,
}

impl SocialTrustLevel {
    /// Display order: cautious first (needs attention), then ascending trust.
    pub fn display_order() -> [Self; 4] {
        [Self::Cautious, Self::Standard, Self::High, Self::Verified]
    }

    /// Human-readable label for the section header / filter chip.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Cautious => "Needs Re-verification",
            Self::Standard => "Not Verified",
            Self::High => "High Trust",
            Self::Verified => "Verified",
        }
    }

    /// Stable string id used in `ToggleItem.id` + back in the
    /// `ItemToggled` event so the engine can map back to a level.
    pub fn id(self) -> &'static str {
        match self {
            Self::Cautious => "cautious",
            Self::Standard => "standard",
            Self::High => "high",
            Self::Verified => "verified",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        match id {
            "cautious" => Some(Self::Cautious),
            "standard" => Some(Self::Standard),
            "high" => Some(Self::High),
            "verified" => Some(Self::Verified),
            _ => None,
        }
    }
}

/// A contact along with its trust level — pre-classified by the
/// AppEngine when constructing the SocialGraphEngine so the engine
/// doesn't need access to `Vauchi`/`MobileContact` types.
#[derive(Clone, Debug)]
pub struct SocialContactEntry {
    pub contact: Item,
    pub trust_level: SocialTrustLevel,
}

/// Engine that displays the social graph: a contact network grouped
/// by trust level, with filter chips and a summary header.
#[derive(Clone, Debug)]
pub struct SocialGraphEngine {
    contacts: Vec<SocialContactEntry>,
    /// Number of visibility groups owned by the user — surfaced in the
    /// summary panel (no other engine state depends on it).
    group_count: usize,
    /// Optional filter — None means "show all trust levels".
    filter: Option<SocialTrustLevel>,
}

impl SocialGraphEngine {
    pub fn new(contacts: Vec<SocialContactEntry>, group_count: usize) -> Self {
        Self {
            contacts,
            group_count,
            filter: None,
        }
    }

    /// Returns the active trust-level filter (None = all).
    pub fn filter(&self) -> Option<SocialTrustLevel> {
        self.filter
    }

    fn total_count(&self) -> usize {
        self.contacts.len()
    }

    fn count_at(&self, level: SocialTrustLevel) -> usize {
        self.contacts
            .iter()
            .filter(|e| e.trust_level == level)
            .count()
    }

    fn verified_count(&self) -> usize {
        self.contacts
            .iter()
            .filter(|e| {
                matches!(
                    e.trust_level,
                    SocialTrustLevel::Verified | SocialTrustLevel::High
                )
            })
            .count()
    }

    fn verified_percent(&self) -> u8 {
        let total = self.total_count();
        if total == 0 {
            0
        } else {
            ((self.verified_count() * 100) / total) as u8
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Network summary panel — totals, trusted %, cautions, groups.
        components.push(Component::InfoPanel {
            id: "network_summary".into(),
            icon: Some("network".into()),
            title: "Your Network".into(),
            items: vec![
                InfoItem {
                    icon: Some("contacts".into()),
                    title: "Contacts".into(),
                    detail: self.total_count().to_string(),
                },
                InfoItem {
                    icon: Some("verified".into()),
                    title: "Trusted".into(),
                    detail: format!("{}%", self.verified_percent()),
                },
                InfoItem {
                    icon: Some("warning".into()),
                    title: "Need re-verify".into(),
                    detail: self.count_at(SocialTrustLevel::Cautious).to_string(),
                },
                InfoItem {
                    icon: Some("groups".into()),
                    title: "Groups".into(),
                    detail: self.group_count.to_string(),
                },
            ],
            a11y: Some(A11y {
                label: Some(format!(
                    "Network summary: {} contacts, {} percent trusted, {} need re-verification, {} groups",
                    self.total_count(),
                    self.verified_percent(),
                    self.count_at(SocialTrustLevel::Cautious),
                    self.group_count,
                )),
                hint: None,
                role: None,
            }),
        });

        // Filter chip row — radio-style ToggleList. "All" + one chip
        // per non-empty trust level (always shown if currently selected
        // even when count is zero, so the user can clear the filter).
        let mut filter_items = vec![ToggleItem {
            id: "all".into(),
            label: format!("All ({})", self.total_count()),
            selected: self.filter.is_none(),
            subtitle: None,
            a11y: None,
            info_key: None,
        }];
        for level in SocialTrustLevel::display_order() {
            let count = self.count_at(level);
            if count == 0 && self.filter != Some(level) {
                continue;
            }
            filter_items.push(ToggleItem {
                id: level.id().into(),
                label: format!("{} ({})", level.display_name(), count),
                selected: self.filter == Some(level),
                subtitle: None,
                a11y: None,
                info_key: None,
            });
        }
        components.push(Component::ToggleList {
            id: "trust_filter".into(),
            label: "Filter".into(),
            items: filter_items,
            a11y: None,
        });

        // Per-trust-level contact lists. When a filter is active,
        // only the matching section is shown. Cautious surfaces first
        // (needs-attention); ascending trust afterwards.
        for level in SocialTrustLevel::display_order() {
            if let Some(active) = self.filter
                && active != level
            {
                continue;
            }
            let level_contacts: Vec<Item> = self
                .contacts
                .iter()
                .filter(|e| e.trust_level == level)
                .map(|e| e.contact.clone())
                .collect();
            if level_contacts.is_empty() {
                continue;
            }
            components.push(Component::Text {
                id: format!("section_{}", level.id()),
                content: format!("{} ({})", level.display_name(), level_contacts.len()),
                style: TextStyle::Subtitle,
            });
            components.push(Component::List {
                id: format!("contacts_{}", level.id()),
                items: level_contacts,
                searchable: false,
            });
        }

        // Empty state — only when no contacts at all (filter-induced
        // empty falls under the per-level skips above and is fine).
        if self.contacts.is_empty() {
            components.push(Component::StatusIndicator {
                id: "empty".into(),
                icon: Some("contacts".into()),
                title: "No contacts yet".into(),
                detail: Some("Exchange with someone to start building your network.".into()),
                status: Status::InProgress,
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "social_graph".into(),
            title: "Contact Network".into(),
            subtitle: None,
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for SocialGraphEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            // Filter chip tapped → toggle the selected level (re-tap clears).
            UserAction::ItemToggled {
                ref component_id,
                ref item_id,
            } if component_id == "trust_filter" => {
                if item_id == "all" {
                    self.filter = None;
                } else if let Some(level) = SocialTrustLevel::from_id(item_id) {
                    // Re-tap on the active filter chip clears the filter
                    // (matches iOS behaviour: chip is a toggle).
                    self.filter = if self.filter == Some(level) {
                        None
                    } else {
                        Some(level)
                    };
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Contact tapped in any per-level list → open detail.
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id.starts_with("contacts_") => ActionResult::OpenContact {
                contact_id: item_id.clone(),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

// INLINE_TEST_REQUIRED: tests assert the engine's screen-shape across
// filter state transitions and validate per-trust-level grouping —
// can't be observed from outside the engine module.
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, level: SocialTrustLevel) -> SocialContactEntry {
        SocialContactEntry {
            contact: Item {
                id: id.into(),
                name: format!("Name {id}"),
                avatar_initials: id.chars().take(2).collect::<String>().to_uppercase(),
                ..Default::default()
            },
            trust_level: level,
        }
    }

    fn engine_with_mix() -> SocialGraphEngine {
        SocialGraphEngine::new(
            vec![
                entry("a", SocialTrustLevel::Verified),
                entry("b", SocialTrustLevel::Standard),
                entry("c", SocialTrustLevel::High),
                entry("d", SocialTrustLevel::Cautious),
                entry("e", SocialTrustLevel::Verified),
            ],
            2,
        )
    }

    fn find_section_text<'a>(screen: &'a ScreenModel, level_id: &str) -> Option<&'a str> {
        screen.components.iter().find_map(|c| match c {
            Component::Text { id, content, .. } if id == &format!("section_{level_id}") => {
                Some(content.as_str())
            }
            _ => None,
        })
    }

    fn list_contact_ids<'a>(screen: &'a ScreenModel, level_id: &str) -> Option<Vec<&'a str>> {
        screen.components.iter().find_map(|c| match c {
            Component::List { id, items, .. } if id == &format!("contacts_{level_id}") => {
                Some(items.iter().map(|c| c.id.as_str()).collect())
            }
            _ => None,
        })
    }

    // @internal
    #[test]
    fn empty_engine_renders_empty_state() {
        let engine = SocialGraphEngine::new(vec![], 0);
        let screen = engine.build_screen();

        assert_eq!(screen.screen_id, "social_graph");
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::StatusIndicator { id, .. } if id == "empty"
            )),
            "empty state must include the empty StatusIndicator"
        );
    }

    // @internal
    #[test]
    fn summary_panel_aggregates_counts() {
        let engine = engine_with_mix();
        let screen = engine.build_screen();

        let summary = screen.components.iter().find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == "network_summary" => Some(items),
            _ => None,
        });
        let items = summary.expect("network_summary panel must exist");

        let detail = |title: &str| {
            items
                .iter()
                .find(|i| i.title == title)
                .map(|i| i.detail.as_str())
                .unwrap_or("")
        };
        assert_eq!(detail("Contacts"), "5");
        // Verified (2) + High (1) = 3 trusted out of 5 → 60%
        assert_eq!(detail("Trusted"), "60%");
        assert_eq!(detail("Need re-verify"), "1");
        assert_eq!(detail("Groups"), "2");
    }

    // @internal
    #[test]
    fn sections_appear_in_display_order() {
        let engine = engine_with_mix();
        let screen = engine.build_screen();

        // Section header IDs follow display order: cautious → standard → high → verified.
        let section_ids: Vec<String> = screen
            .components
            .iter()
            .filter_map(|c| match c {
                Component::Text { id, .. } if id.starts_with("section_") => Some(id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            section_ids,
            vec![
                "section_cautious",
                "section_standard",
                "section_high",
                "section_verified",
            ]
        );
    }

    // @internal
    #[test]
    fn each_section_lists_only_matching_contacts() {
        let engine = engine_with_mix();
        let screen = engine.build_screen();

        let cautious = list_contact_ids(&screen, "cautious").unwrap();
        assert_eq!(cautious, vec!["d"]);
        let standard = list_contact_ids(&screen, "standard").unwrap();
        assert_eq!(standard, vec!["b"]);
        let high = list_contact_ids(&screen, "high").unwrap();
        assert_eq!(high, vec!["c"]);
        let verified = list_contact_ids(&screen, "verified").unwrap();
        assert_eq!(verified, vec!["a", "e"]);
    }

    // @internal
    #[test]
    fn empty_levels_skip_section_header() {
        let engine = SocialGraphEngine::new(vec![entry("a", SocialTrustLevel::Verified)], 0);
        let screen = engine.build_screen();
        assert!(find_section_text(&screen, "cautious").is_none());
        assert!(find_section_text(&screen, "standard").is_none());
        assert!(find_section_text(&screen, "high").is_none());
        assert!(find_section_text(&screen, "verified").is_some());
    }

    // @internal
    #[test]
    fn filter_chip_tap_narrows_to_single_section() {
        let mut engine = engine_with_mix();
        let result = engine.handle_action(UserAction::ItemToggled {
            component_id: "trust_filter".into(),
            item_id: "verified".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
        assert_eq!(engine.filter(), Some(SocialTrustLevel::Verified));

        let screen = engine.build_screen();
        // Other sections must not appear.
        assert!(find_section_text(&screen, "cautious").is_none());
        assert!(find_section_text(&screen, "standard").is_none());
        assert!(find_section_text(&screen, "high").is_none());
        // Verified section still present with both members.
        let verified = list_contact_ids(&screen, "verified").unwrap();
        assert_eq!(verified, vec!["a", "e"]);
    }

    // @internal
    #[test]
    fn re_tap_active_filter_clears_to_all() {
        let mut engine = engine_with_mix();
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "trust_filter".into(),
            item_id: "high".into(),
        });
        assert_eq!(engine.filter(), Some(SocialTrustLevel::High));

        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "trust_filter".into(),
            item_id: "high".into(),
        });
        assert_eq!(engine.filter(), None);
    }

    // @internal
    #[test]
    fn all_chip_clears_active_filter() {
        let mut engine = engine_with_mix();
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "trust_filter".into(),
            item_id: "high".into(),
        });
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "trust_filter".into(),
            item_id: "all".into(),
        });
        assert_eq!(engine.filter(), None);
    }

    // @internal
    #[test]
    fn filter_chips_show_only_non_empty_levels_when_no_filter() {
        // Only Verified contacts present — Cautious/Standard/High chips
        // should not appear.
        let engine = SocialGraphEngine::new(vec![entry("a", SocialTrustLevel::Verified)], 0);
        let screen = engine.build_screen();
        let chips = screen.components.iter().find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "trust_filter" => Some(items),
            _ => None,
        });
        let chip_ids: Vec<&str> = chips.unwrap().iter().map(|i| i.id.as_str()).collect();
        assert_eq!(chip_ids, vec!["all", "verified"]);
    }

    // @internal
    #[test]
    fn active_filter_chip_remains_visible_when_count_is_zero() {
        let mut engine = SocialGraphEngine::new(vec![entry("a", SocialTrustLevel::Verified)], 0);
        // Force-select the empty Cautious filter.
        let _ = engine.handle_action(UserAction::ItemToggled {
            component_id: "trust_filter".into(),
            item_id: "cautious".into(),
        });
        let screen = engine.build_screen();
        let chips = screen.components.iter().find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "trust_filter" => Some(items),
            _ => None,
        });
        let chip_ids: Vec<&str> = chips.unwrap().iter().map(|i| i.id.as_str()).collect();
        // Cautious chip stays visible so the user can re-tap it to clear.
        assert!(chip_ids.contains(&"cautious"));
    }

    // @internal
    #[test]
    fn tapping_contact_emits_open_contact() {
        let mut engine = engine_with_mix();
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "contacts_verified".into(),
            item_id: "a".into(),
        });
        match result {
            ActionResult::OpenContact { contact_id } => assert_eq!(contact_id, "a"),
            other => panic!("expected OpenContact, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn unrelated_action_refreshes_screen() {
        let mut engine = engine_with_mix();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "noop".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }
}
