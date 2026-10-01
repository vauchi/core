// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Places engine — manage the named-place vocabulary (ADR-051).
//!
//! Lists every named place with a per-row delete that routes through an
//! engine-owned `InlineConfirm` (static `delete_place` confirm id +
//! `pending_delete` state, so the BFS reachability walker dedupes the
//! confirm state by `screen_id`). Exchange locations nobody has named yet
//! are offered here too, one row per spot, opening the form that names
//! them (#467).
//!
//! The actual `Vauchi::delete_place` call needs storage, so the
//! `confirm_delete_place` action is resolved by the AppEngine intercept,
//! which reads [`PlacesEngine::pending_delete_id`] then applies
//! [`PlacesEngine::confirm_delete`].

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Summary of a named place for the management list.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlaceSummary {
    pub id: String,
    pub name: String,
    /// Contacts whose exchange location is linked to this place.
    pub met_count: usize,
}

/// A spot where exchanges were recorded but nobody named the place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnnamedPlace {
    /// The contact whose location anchors the spot; names it on selection.
    pub anchor_contact_id: String,
    /// Display names of the contacts met there, anchor first.
    pub met: Vec<String>,
}

/// Engine for the place-management list.
#[derive(Clone, Debug)]
pub struct PlacesEngine {
    places: Vec<PlaceSummary>,
    unnamed: Vec<UnnamedPlace>,
    pending_delete: Option<String>,
    locale: Locale,
}

impl PlacesEngine {
    pub fn new(places: Vec<PlaceSummary>) -> Self {
        Self {
            places,
            unnamed: Vec::new(),
            pending_delete: None,
            locale: Locale::English,
        }
    }

    /// Set the spots nobody has named yet.
    pub fn with_unnamed(mut self, unnamed: Vec<UnnamedPlace>) -> Self {
        self.unnamed = unnamed;
        self
    }

    fn met_count(&self, n: usize) -> String {
        match n {
            0 => self.t("places_list.met_count_none"),
            1 => self.t("places_list.met_count_one"),
            _ => get_string_with_args(
                self.locale,
                "places_list.met_count_many",
                &[("count", &n.to_string())],
            ),
        }
    }

    /// Who you met there, so each unnamed row reads differently.
    fn unnamed_detail(&self, spot: &UnnamedPlace) -> String {
        match spot.met.as_slice() {
            [] => self.t("places_list.unnamed_title"),
            [name] => get_string_with_args(
                self.locale,
                "places_list.unnamed_detail_one",
                &[("name", name)],
            ),
            [first, second] => get_string_with_args(
                self.locale,
                "places_list.unnamed_detail_two",
                &[("first", first), ("second", second)],
            ),
            [first, rest @ ..] => get_string_with_args(
                self.locale,
                "places_list.unnamed_detail_many",
                &[("first", first), ("count", &rest.len().to_string())],
            ),
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-14).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
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
        let mut components = vec![Component::Text {
            id: "places_intro".into(),
            content: self.t("places_list.intro"),
            style: TextStyle::Body,
            a11y: None,
        }];
        if self.places.is_empty() && self.unnamed.is_empty() {
            components.push(Component::Text {
                id: "places_empty".into(),
                content: self.t("places_list.empty"),
                style: TextStyle::Body,
                a11y: None,
            });
        }

        if !self.places.is_empty() {
            components.push(Component::List {
                id: "places".into(),
                items: self
                    .places
                    .iter()
                    .map(|p| Item {
                        id: p.id.clone(),
                        name: p.name.clone(),
                        subtitle: Some(self.met_count(p.met_count)),
                        initials: String::new(),
                        status: None,
                        actions: vec![ListItemAction {
                            id: "request_delete".into(),
                            label: self.t("action.delete"),
                            kind: ListItemActionKind::Custom,
                            destructive: true,
                        }],
                        a11y: None,
                    })
                    .collect(),
                searchable: false,
                total_count: 0,
                offset: 0,
                window: 0,
            });
        }

        if !self.unnamed.is_empty() {
            components.push(Component::List {
                id: "unnamed_places".into(),
                items: self
                    .unnamed
                    .iter()
                    .map(|spot| Item {
                        id: spot.anchor_contact_id.clone(),
                        name: self.t("places_list.unnamed_title"),
                        subtitle: Some(self.unnamed_detail(spot)),
                        initials: String::new(),
                        status: None,
                        actions: vec![],
                        a11y: None,
                    })
                    .collect(),
                searchable: false,
                total_count: 0,
                offset: 0,
                window: 0,
            });
        }

        if let Some(name) = self.pending_name() {
            components.push(Component::InlineConfirm {
                id: "delete_place".into(),
                warning: get_string_with_args(
                    self.locale,
                    "places_list.delete_warning",
                    &[("name", name)],
                ),
                confirm_text: self.t("action.delete"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: "confirm_delete_place".into(),
                cancel_action_id: "cancel_delete_place".into(),
                destructive: true,
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "places".into(),
            title: self.t("places_list.title"),
            subtitle: None,
            components,
            contextual_actions: vec![],
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
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == "unnamed_places"
                && self.unnamed.iter().any(|s| s.anchor_contact_id == item_id) =>
            {
                ActionResult::ShowFormDialog {
                    dialog_type: "name_place".into(),
                    context_id: Some(item_id),
                }
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
