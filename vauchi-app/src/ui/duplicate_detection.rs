// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duplicate detection engine — shows potential duplicate contact pairs.
//!
//! The engine renders the list and tracks which pair the user has selected
//! (`ListItemSelected`). The outer [`AppEngine`] reads `selected_pair_index()`
//! when handling the "merge" action so the navigation lands on the selected
//! pair instead of always picking the first one.
//!
//! Cross-kind detection lives on each [`DuplicatePair`] (`is_cross_kind()`,
//! `imported_id()`); downstream code uses these to pick between
//! `merge_contacts` (same-kind) and `soft_delete_imported_contact`
//! (cross-kind) since core rejects cross-kind merges with `InvalidState`.

use crate::ui::*;

/// A pair of potentially duplicate contacts.
#[derive(Clone, Debug)]
pub struct DuplicatePair {
    pub id1: String,
    pub name1: String,
    /// True when contact 1 is an imported contact (no crypto exchange).
    pub is_imported_1: bool,
    pub id2: String,
    pub name2: String,
    /// True when contact 2 is an imported contact (no crypto exchange).
    pub is_imported_2: bool,
    pub similarity: f64,
}

impl DuplicatePair {
    /// True when one contact is imported and the other was exchanged.
    /// Cross-kind pairs cannot be merged (core rejects with `InvalidState`);
    /// the user should be offered to delete the imported side instead.
    pub fn is_cross_kind(&self) -> bool {
        self.is_imported_1 != self.is_imported_2
    }

    /// For cross-kind pairs, returns the id of the imported contact.
    /// For same-kind pairs, returns `None`.
    pub fn imported_id(&self) -> Option<&str> {
        if !self.is_cross_kind() {
            return None;
        }
        if self.is_imported_1 {
            Some(&self.id1)
        } else {
            Some(&self.id2)
        }
    }
}

/// Engine that displays detected duplicate contacts.
#[derive(Clone, Debug)]
pub struct DuplicateDetectionEngine {
    pairs: Vec<DuplicatePair>,
    /// Index into `pairs` of the user-selected pair, if any. Set on
    /// `ListItemSelected` for an item id of the form `pair_N`. The outer
    /// `AppEngine` reads this when handling the "merge" action so the
    /// navigation targets the selected pair rather than the first one.
    selected_pair_index: Option<usize>,
}

impl DuplicateDetectionEngine {
    pub fn new(pairs: Vec<DuplicatePair>) -> Self {
        Self {
            pairs,
            selected_pair_index: None,
        }
    }

    /// Returns the index of the user-selected pair (set via `ListItemSelected`).
    pub fn selected_pair_index(&self) -> Option<usize> {
        self.selected_pair_index
    }

    /// Returns a reference to the pairs (read-only).
    pub fn pairs(&self) -> &[DuplicatePair] {
        &self.pairs
    }

    fn build_screen(&self) -> ScreenModel {
        let components = if self.pairs.is_empty() {
            vec![Component::Text {
                id: "no_duplicates".into(),
                content: "No duplicate contacts detected.".into(),
                style: TextStyle::Body,
            }]
        } else {
            vec![
                Component::Text {
                    id: "header".into(),
                    content: format!("{} potential duplicate(s) found", self.pairs.len()),
                    style: TextStyle::Subtitle,
                },
                Component::ActionList {
                    id: "duplicate_pairs".into(),
                    items: self
                        .pairs
                        .iter()
                        .enumerate()
                        .map(|(i, pair)| {
                            let pct = (pair.similarity * 100.0) as u8;
                            let detail = if pair.is_cross_kind() {
                                format!("{pct}% similar — cross-kind")
                            } else {
                                format!("{pct}% similar")
                            };
                            ActionListItem {
                                id: format!("pair_{i}"),
                                label: format!("{} <-> {}", pair.name1, pair.name2),
                                icon: None,
                                detail: Some(detail),
                                a11y: None,
                                info_key: None,
                            }
                        })
                        .collect(),
                },
            ]
        };

        // Enable "merge" only once the user has selected a pair, so the
        // button always operates on a known target instead of silently
        // defaulting to pair 0.
        let merge_enabled = self.selected_pair_index.is_some();

        ScreenModel {
            screen_id: "duplicate_detection".into(),
            title: "Duplicate Detection".into(),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "merge".into(),
                    label: "Merge".into(),
                    style: ActionStyle::Primary,
                    enabled: merge_enabled,
                    a11y: None,
                },
                ScreenAction {
                    id: "dismiss".into(),
                    label: "Dismiss".into(),
                    style: ActionStyle::Secondary,
                    enabled: merge_enabled,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for DuplicateDetectionEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::DuplicateDetection {
            selected_pair_index: self.selected_pair_index(),
        })
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ListItemSelected {
                ref component_id,
                ref item_id,
            } if component_id == "duplicate_pairs" => {
                if let Some(idx_str) = item_id.strip_prefix("pair_")
                    && let Ok(idx) = idx_str.parse::<usize>()
                    && idx < self.pairs.len()
                {
                    self.selected_pair_index = Some(idx);
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { ref action_id } => match action_id.as_str() {
                // Engine signals Complete unconditionally; the outer
                // AppEngine intercept inspects vauchi.find_duplicates() +
                // selected_pair_index() to decide what to actually do
                // (navigate to ContactMerge, dismiss the pair, or surface
                // a no-op for empty/cross-kind cases).
                "merge" | "dismiss" => ActionResult::Complete,
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: tests assert engine state machine across selection +
// action transitions, which can't be done from outside the engine module.
#[cfg(test)]
mod tests {
    use super::*;

    fn pair(id1: &str, id2: &str, imp1: bool, imp2: bool, sim: f64) -> DuplicatePair {
        DuplicatePair {
            id1: id1.into(),
            name1: format!("Name {id1}"),
            is_imported_1: imp1,
            id2: id2.into(),
            name2: format!("Name {id2}"),
            is_imported_2: imp2,
            similarity: sim,
        }
    }

    // @internal
    #[test]
    fn empty_pairs_renders_empty_state() {
        let engine = DuplicateDetectionEngine::new(vec![]);
        let screen = engine.build_screen();

        assert_eq!(screen.screen_id, "duplicate_detection");
        assert_eq!(screen.components.len(), 1);
        assert!(matches!(
            &screen.components[0],
            Component::Text { id, .. } if id == "no_duplicates"
        ));
        // Both screen-level actions disabled with no pairs.
        assert!(screen.actions.iter().all(|a| !a.enabled));
    }

    // @internal
    #[test]
    fn pairs_render_as_action_list() {
        let pairs = vec![
            pair("a", "b", false, false, 0.92),
            pair("c", "d", true, true, 0.85),
        ];
        let engine = DuplicateDetectionEngine::new(pairs);
        let screen = engine.build_screen();

        // header + ActionList
        assert_eq!(screen.components.len(), 2);
        let Component::ActionList { items, .. } = &screen.components[1] else {
            panic!("expected ActionList, got {:?}", screen.components[1]);
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "pair_0");
        assert_eq!(items[1].id, "pair_1");
        assert_eq!(items[0].detail.as_deref(), Some("92% similar"));
        assert_eq!(items[1].detail.as_deref(), Some("85% similar"));
    }

    // @internal
    #[test]
    fn cross_kind_pair_detail_marks_cross_kind() {
        let pairs = vec![pair("a", "b", true, false, 0.91)];
        let engine = DuplicateDetectionEngine::new(pairs);
        let screen = engine.build_screen();

        let Component::ActionList { items, .. } = &screen.components[1] else {
            panic!("expected ActionList");
        };
        assert_eq!(items[0].detail.as_deref(), Some("91% similar — cross-kind"));
    }

    // @internal
    #[test]
    fn merge_disabled_until_pair_selected() {
        let pairs = vec![pair("a", "b", false, false, 0.9)];
        let mut engine = DuplicateDetectionEngine::new(pairs);

        // Merge starts disabled.
        let initial = engine.build_screen();
        let merge = initial.actions.iter().find(|a| a.id == "merge").unwrap();
        assert!(!merge.enabled);

        // After selecting a pair, merge becomes enabled.
        let result = engine.handle_action(UserAction::ListItemSelected {
            component_id: "duplicate_pairs".into(),
            item_id: "pair_0".into(),
        });
        let ActionResult::UpdateScreen(updated) = result else {
            panic!("expected UpdateScreen");
        };
        let merge = updated.actions.iter().find(|a| a.id == "merge").unwrap();
        assert!(merge.enabled);
        assert_eq!(engine.selected_pair_index(), Some(0));
    }

    // @internal
    #[test]
    fn list_item_selected_records_index() {
        let pairs = vec![
            pair("a", "b", false, false, 0.9),
            pair("c", "d", false, false, 0.8),
            pair("e", "f", false, false, 0.7),
        ];
        let mut engine = DuplicateDetectionEngine::new(pairs);

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "duplicate_pairs".into(),
            item_id: "pair_2".into(),
        });
        assert_eq!(engine.selected_pair_index(), Some(2));

        // Selecting another pair updates the index.
        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "duplicate_pairs".into(),
            item_id: "pair_0".into(),
        });
        assert_eq!(engine.selected_pair_index(), Some(0));
    }

    // @internal
    #[test]
    fn list_item_selected_ignores_unknown_component() {
        let pairs = vec![pair("a", "b", false, false, 0.9)];
        let mut engine = DuplicateDetectionEngine::new(pairs);

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "some_other_list".into(),
            item_id: "pair_0".into(),
        });
        assert_eq!(engine.selected_pair_index(), None);
    }

    // @internal
    #[test]
    fn list_item_selected_ignores_out_of_range_index() {
        let pairs = vec![pair("a", "b", false, false, 0.9)];
        let mut engine = DuplicateDetectionEngine::new(pairs);

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "duplicate_pairs".into(),
            item_id: "pair_99".into(),
        });
        assert_eq!(engine.selected_pair_index(), None);
    }

    // @internal
    #[test]
    fn merge_action_returns_complete() {
        let pairs = vec![pair("a", "b", false, false, 0.9)];
        let mut engine = DuplicateDetectionEngine::new(pairs);

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "duplicate_pairs".into(),
            item_id: "pair_0".into(),
        });

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "merge".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        // Selection persists so the outer handler can read it.
        assert_eq!(engine.selected_pair_index(), Some(0));
    }

    // @internal
    #[test]
    fn dismiss_action_returns_complete() {
        let pairs = vec![pair("a", "b", false, false, 0.9)];
        let mut engine = DuplicateDetectionEngine::new(pairs);

        let _ = engine.handle_action(UserAction::ListItemSelected {
            component_id: "duplicate_pairs".into(),
            item_id: "pair_0".into(),
        });

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "dismiss".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
    }

    // @internal
    #[test]
    fn is_cross_kind_detects_mixed_pair() {
        assert!(!pair("a", "b", false, false, 0.9).is_cross_kind());
        assert!(!pair("a", "b", true, true, 0.9).is_cross_kind());
        assert!(pair("a", "b", true, false, 0.9).is_cross_kind());
        assert!(pair("a", "b", false, true, 0.9).is_cross_kind());
    }

    // @internal
    #[test]
    fn imported_id_returns_imported_side_for_cross_kind() {
        assert_eq!(pair("a", "b", true, false, 0.9).imported_id(), Some("a"));
        assert_eq!(pair("a", "b", false, true, 0.9).imported_id(), Some("b"));
        // Same-kind pairs return None regardless of imported flag value.
        assert_eq!(pair("a", "b", false, false, 0.9).imported_id(), None);
        assert_eq!(pair("a", "b", true, true, 0.9).imported_id(), None);
    }
}
