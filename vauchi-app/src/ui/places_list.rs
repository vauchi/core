// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Places engine — manage the named-place vocabulary (ADR-051).
//!
//! Lists every named place with a per-row delete that routes through an
//! engine-owned `InlineConfirm` (static `delete_place` confirm id +
//! `pending_delete` state, so the BFS reachability walker dedupes the
//! confirm state by `screen_id`). Place *creation* happens at exchange time
//! (naming a recorded location), not here — this screen is management only.
//!
//! The actual `Vauchi::delete_place` call needs storage, so the
//! `confirm_delete_place` action is resolved by the AppEngine intercept,
//! which reads [`PlacesEngine::pending_delete_id`] then applies
//! [`PlacesEngine::confirm_delete`].

use crate::ui::*;

/// Summary of a named place for the management list.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaceSummary {
    pub id: String,
    pub name: String,
}

/// Engine for the place-management list.
#[derive(Clone, Debug)]
pub struct PlacesEngine {
    places: Vec<PlaceSummary>,
    pending_delete: Option<String>,
}

impl PlacesEngine {
    pub fn new(places: Vec<PlaceSummary>) -> Self {
        Self {
            places,
            pending_delete: None,
        }
    }

    /// Id of the place awaiting delete confirmation — read by the AppEngine
    /// intercept so `Vauchi::delete_place` knows its target.
    pub fn pending_delete_id(&self) -> Option<&str> {
        self.pending_delete.as_deref()
    }

    /// Apply a confirmed delete: drop the row and clear the pending state.
    pub fn confirm_delete(&mut self) {
        if let Some(id) = self.pending_delete.take() {
            self.places.retain(|p| p.id != id);
        }
    }

    fn pending_name(&self) -> Option<&str> {
        let id = self.pending_delete.as_deref()?;
        self.places
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.as_str())
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        let items: Vec<Item> = self
            .places
            .iter()
            .map(|p| Item {
                id: p.id.clone(),
                name: p.name.clone(),
                subtitle: None,
                avatar_initials: String::new(),
                status: None,
                actions: vec![ListItemAction {
                    id: "request_delete".into(),
                    label: "Delete".into(),
                    kind: ListItemActionKind::Custom,
                    destructive: false,
                }],
                a11y: None,
            })
            .collect();

        components.push(Component::List {
            id: "places".into(),
            items,
            searchable: false,
            total_count: 0,
            offset: 0,
            window: 0,
        });

        if let Some(name) = self.pending_name() {
            components.push(Component::InlineConfirm {
                id: "delete_place".into(),
                warning: format!(
                    "Delete the place \"{name}\"? Contacts met there keep their \
                     coordinates; only the name is removed."
                ),
                confirm_text: "Delete".into(),
                cancel_text: "Cancel".into(),
                destructive: true,
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "places".into(),
            title: "Places".into(),
            subtitle: None,
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for PlacesEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::Places {
            pending_delete_id: self.pending_delete_id().map(str::to_string),
        })
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        match update {
            crate::ui::EngineUpdate::ConfirmPendingDelete => {
                self.confirm_delete();
                true
            }
            _ => false,
        }
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ListItemAction {
                component_id,
                item_id,
                action_id,
            } if component_id == "places" && action_id == "request_delete" => {
                self.pending_delete = Some(item_id);
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "cancel_delete_place" => {
                self.pending_delete = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            // `confirm_delete_place` needs `Vauchi` and is resolved by the
            // AppEngine intercept; here it falls through to a no-op re-render.
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
