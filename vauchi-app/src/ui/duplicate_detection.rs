// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duplicate detection engine — shows potential duplicate contact pairs.

use crate::ui::*;

/// A pair of potentially duplicate contacts.
#[derive(Clone, Debug)]
pub struct DuplicatePair {
    pub id1: String,
    pub name1: String,
    pub id2: String,
    pub name2: String,
    pub similarity: f64,
}

/// Engine that displays detected duplicate contacts.
#[derive(Clone, Debug)]
pub struct DuplicateDetectionEngine {
    pairs: Vec<DuplicatePair>,
}

impl DuplicateDetectionEngine {
    pub fn new(pairs: Vec<DuplicatePair>) -> Self {
        Self { pairs }
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
                            ActionListItem {
                                id: format!("pair_{i}"),
                                label: format!("{} <-> {}", pair.name1, pair.name2),
                                icon: None,
                                detail: Some(format!("{pct}% similar")),
                                a11y: None,
                                info_key: None,
                            }
                        })
                        .collect(),
                },
            ]
        };

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
                    enabled: !self.pairs.is_empty(),
                    a11y: None,
                },
                ScreenAction {
                    id: "dismiss".into(),
                    label: "Dismiss".into(),
                    style: ActionStyle::Secondary,
                    enabled: !self.pairs.is_empty(),
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for DuplicateDetectionEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                "merge" => ActionResult::Complete,
                "dismiss" => ActionResult::UpdateScreen(self.build_screen()),
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
