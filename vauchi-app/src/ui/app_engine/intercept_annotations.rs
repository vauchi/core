// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact-detail annotation intercepts (ADR-051): personal/field notes,
//! tags, exchange place, faceted search, and tag→group promotion. Split
//! out of `intercept.rs` to keep that dispatcher under the file-size limit.
//! These are `impl AppEngine` methods, dispatched from `dispatch.rs`
//! (contact-screen intercepts) and `mod.rs` (`handle_action`).

use super::AppEngine;
use super::AppScreen;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::contact_detail_rules::ContactTag;
use crate::ui::{Component, ScreenModel};
use vauchi_core::SearchFacets;

fn editable_text_value(screen: &ScreenModel, component_id: &str) -> Option<String> {
    screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::EditableText { id, value, .. } if id == component_id => Some(value.clone()),
            _ => None,
        })
}

impl AppEngine {
    /// Persist the core-owned personal-note draft only when Save is pressed.
    /// TextChanged first updates the live ContactDetail engine; Cancel therefore
    /// discards the draft without touching storage.
    pub(super) fn persist_personal_note_save(&mut self, contact_id: &str, action: &UserAction) {
        if matches!(
            action,
            UserAction::ActionPressed { action_id } if action_id == "save_personal_note"
        ) && let Some(value) =
            editable_text_value(&self.engine.current_screen(), "personal_note")
        {
            // Encryption handled at the storage layer (save_personal_notes encrypts
            // with the storage encryption key). Legacy plaintext rows are self-healed
            // on next load+save cycle. See: problems/2026-03-27-notes-encryption-gap.
            if let Err(e) = self
                .vauchi
                .save_personal_notes(contact_id, value.as_bytes())
            {
                let _ = e; // Silently ignore — UI already shows the field unchanged
            }
        }
    }

    /// Persist the core-owned per-field draft only when its Save is pressed.
    pub(super) fn persist_field_note_save(&mut self, contact_id: &str, action: &UserAction) {
        if let UserAction::ActionPressed { action_id } = action
            && let Some(field_id) = action_id.strip_prefix("save_field_note:")
            && let Some(value) = editable_text_value(
                &self.engine.current_screen(),
                &format!("field_note:{field_id}"),
            )
        {
            if let Err(e) =
                self.vauchi
                    .save_contact_field_note(contact_id, field_id, value.as_bytes())
            {
                let _ = e;
            }
        }
    }

    /// Intercept add-tag typing, tag commit, and tag removal on the
    /// ContactDetail screen (ADR-051 contact annotations).
    ///
    /// - `TextChanged { "add_tag" }` → recompute autocomplete suggestions
    ///   via `Vauchi::tag_name_suggestions` and stash them on the engine as
    ///   transient state. No storage write and no `invalidate_screen`: the
    ///   in-progress query must survive the re-render.
    /// - `ActionPressed { "add_tag:<name>" }` → `add_tag_to_contact`
    ///   (autocomplete-or-create; core dedups by name), then optimistically
    ///   add the returned tag to the in-memory engine and clear the query.
    /// - `ActionPressed { "remove_tag:<id>" }` → `remove_tag_from_contact`,
    ///   then optimistically drop the row from the in-memory engine.
    ///
    /// Persistence-then-optimistic-render mirrors the hide/trust toggles.
    /// `invalidate_screen` is deliberately NOT called here: it now rebuilds
    /// the live current engine, which would wipe the in-progress query —
    /// so the fresh tag row is applied in memory instead. Storage errors
    /// are swallowed (best-effort) — the in-memory edit is only applied
    /// when the corresponding `Vauchi` call succeeded, so the engine stays
    /// consistent with storage on the next genuine reload.
    pub(super) fn intercept_tag_action(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "add_tag" => {
                let suggestions = self.vauchi.tag_name_suggestions(value).unwrap_or_default();
                self.engine
                    .apply_update(crate::ui::EngineUpdate::ContactDetail(
                        crate::ui::ContactDetailUpdate::TagQuery {
                            query: value.clone(),
                            suggestions,
                        },
                    ))
                    .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            UserAction::ActionPressed { action_id } if action_id.starts_with("add_tag:") => {
                let name = action_id.strip_prefix("add_tag:").unwrap_or_default();
                let added = self.vauchi.add_tag_to_contact(contact_id, name).ok();
                if let Some(tag) = added
                    && !self
                        .engine
                        .apply_update(crate::ui::EngineUpdate::ContactDetail(
                            crate::ui::ContactDetailUpdate::TagAdded(ContactTag {
                                id: tag.id,
                                name: tag.name,
                            }),
                        ))
                {
                    return None;
                }
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            UserAction::ActionPressed { action_id } if action_id.starts_with("remove_tag:") => {
                let tag_id = action_id.strip_prefix("remove_tag:").unwrap_or_default();
                let removed = self
                    .vauchi
                    .remove_tag_from_contact(tag_id, contact_id)
                    .is_ok();
                if removed
                    && !self
                        .engine
                        .apply_update(crate::ui::EngineUpdate::ContactDetail(
                            crate::ui::ContactDetailUpdate::TagRemoved(tag_id.to_string()),
                        ))
                {
                    return None;
                }
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            _ => None,
        }
    }

    /// Intercept the tag-delete confirmation on the Tags management screen
    /// (ADR-051). Reads the armed tag id from the engine, deletes it via
    /// `Vauchi::delete_tag`, then applies the optimistic row drop. The
    /// `cancel_delete_tag` action needs no storage, so the engine handles it
    /// directly and it is not intercepted here.
    pub(super) fn intercept_tag_delete(&mut self, action: &UserAction) -> Option<ActionResult> {
        if self.screen != AppScreen::Tags {
            return None;
        }
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if action_id != "confirm_delete_tag" {
            return None;
        }
        let tag_id = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Tags { pending_delete_id }) => pending_delete_id?,
            _ => return None,
        };
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.vauchi.delete_tag(&tag_id);
        self.engine
            .apply_update(crate::ui::EngineUpdate::ConfirmPendingDelete)
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept the place-delete confirmation on the Places management
    /// screen (ADR-051). Reads the armed place id from the engine, deletes it
    /// via `Vauchi::delete_place`, then applies the optimistic row drop.
    pub(super) fn intercept_place_delete(&mut self, action: &UserAction) -> Option<ActionResult> {
        if self.screen != AppScreen::Places {
            return None;
        }
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if action_id != "confirm_delete_place" {
            return None;
        }
        let place_id = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Places { pending_delete_id }) => pending_delete_id?,
            _ => return None,
        };
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.vauchi.delete_place(&place_id);
        self.engine
            .apply_update(crate::ui::EngineUpdate::ConfirmPendingDelete)
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept the tag→group promotion flow (ADR-051):
    ///
    /// - On the Tags screen, a per-row `promote` ListItemAction opens the
    ///   promotion review (`AppScreen::TagPromotion`), whose factory builds
    ///   the `GroupDraft` via `Vauchi::begin_tag_promotion`.
    /// - On the review screen, `confirm_promotion` reads the reviewed field
    ///   selection off the engine and calls `Vauchi::confirm_tag_promotion`
    ///   (replace: creates the group, consumes the tag), then lands on the
    ///   Groups list. A failed promotion stays on the review screen.
    pub(super) fn intercept_tag_promotion(&mut self, action: &UserAction) -> Option<ActionResult> {
        if self.screen == AppScreen::Tags
            && let UserAction::ListItemAction {
                component_id,
                item_id,
                action_id,
            } = action
            && component_id == "tags"
            && action_id == "promote"
        {
            let screen = self.navigate_to(AppScreen::TagPromotion {
                tag_id: item_id.clone(),
            });
            return Some(ActionResult::NavigateTo(screen));
        }

        if let AppScreen::TagPromotion { tag_id } = &self.screen {
            let tag_id = tag_id.clone();
            let UserAction::ActionPressed { action_id } = action else {
                return None;
            };
            if action_id != "confirm_promotion" {
                return None;
            }
            let fields = match self.engine.engine_output() {
                Some(crate::ui::EngineOutput::TagPromotion { selected_field_ids }) => {
                    selected_field_ids
                }
                other => {
                    tracing::warn!(?other, "promotion without TagPromotion output");
                    Vec::new()
                }
            };
            return Some(match self.vauchi.confirm_tag_promotion(&tag_id, fields) {
                Ok(_group_id) => {
                    // The tag is consumed (replace semantics); drop the cached
                    // Tags engine so a later visit rebuilds from storage.
                    self.invalidate_screen(&AppScreen::Tags);
                    let screen = self.navigate_to(AppScreen::Groups);
                    ActionResult::NavigateTo(screen)
                }
                Err(_) => ActionResult::UpdateScreen(self.engine.current_screen()),
            });
        }

        None
    }

    /// Faceted contact search (ADR-051): on the Contacts screen, route the
    /// search query + facet opt-ins through the canonical core API
    /// (`Vauchi::search_contacts_faceted`) instead of the engine's plain
    /// in-memory name match.
    ///
    /// - Toggling a `search_facets` item flips the opt-in and recomputes.
    /// - A `SearchChanged` is only intercepted while faceting is active;
    ///   otherwise it falls through so the engine keeps its (unchanged)
    ///   in-memory name search.
    pub(super) fn intercept_contact_facets(&mut self, action: &UserAction) -> Option<ActionResult> {
        if self.screen != AppScreen::Contacts {
            return None;
        }
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "search_facets" => {
                if !self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::ContactList(
                        crate::ui::ContactListUpdate::ToggleFacet(item_id.clone()),
                    ))
                {
                    return None;
                }
                let query = match self.engine.engine_output() {
                    Some(crate::ui::EngineOutput::ContactList { query, .. }) => query,
                    _ => String::new(),
                };
                self.apply_contact_facets(query);
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            UserAction::SearchChanged { query, .. } => {
                let faceting = matches!(
                    self.engine.engine_output(),
                    Some(crate::ui::EngineOutput::ContactList {
                        any_facet: true,
                        ..
                    })
                );
                if !faceting {
                    return None;
                }
                let _ = self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::ContactList(
                        crate::ui::ContactListUpdate::SearchQuery(query.clone()),
                    ));
                self.apply_contact_facets(query.clone());
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            _ => None,
        }
    }

    /// Recompute the faceted result set for the contact list's current facet
    /// flags + `query`, and push it onto the engine. No facet enabled clears
    /// faceted mode (the engine reverts to in-memory name search).
    fn apply_contact_facets(&mut self, query: String) {
        let Some(crate::ui::EngineOutput::ContactList {
            facets: (tags, comment, place),
            ..
        }) = self.engine.engine_output()
        else {
            return;
        };
        let ids = if tags || comment || place {
            let facets = SearchFacets {
                tags,
                comment,
                place,
                time_range: None,
            };
            self.vauchi
                .search_contacts_faceted(&query, &facets)
                .map(|cs| cs.iter().map(|c| c.id().to_string()).collect::<Vec<_>>())
                .ok()
        } else {
            None
        };
        let _ = self
            .engine
            .apply_update(crate::ui::EngineUpdate::ContactList(
                crate::ui::ContactListUpdate::FacetedIds(ids),
            ));
    }

    /// Intercept exchange-place naming/clearing on the ContactDetail screen
    /// (ADR-051). Mirrors the tag intercept: persist via `Vauchi`, then apply
    /// an optimistic in-memory update (the storage write succeeded).
    ///
    /// - `TextChanged { "name_place" }` → recompute named-place suggestions.
    /// - `ActionPressed { "name_place:<name>" }` → `name_exchange_place`.
    /// - `ActionPressed { "clear_exchange_place" }` → `clear_exchange_location`.
    pub(super) fn intercept_place_action(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "name_place" => {
                let suggestions = self.place_name_suggestions(value);
                self.engine
                    .apply_update(crate::ui::EngineUpdate::ContactDetail(
                        crate::ui::ContactDetailUpdate::PlaceQuery {
                            query: value.clone(),
                            suggestions,
                        },
                    ))
                    .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            UserAction::ActionPressed { action_id } if action_id.starts_with("name_place:") => {
                let name = action_id.strip_prefix("name_place:").unwrap_or_default();
                let named = self.vauchi.name_exchange_place(contact_id, name).is_ok();
                if named
                    && !self
                        .engine
                        .apply_update(crate::ui::EngineUpdate::ContactDetail(
                            crate::ui::ContactDetailUpdate::PlaceNamed(name.to_string()),
                        ))
                {
                    return None;
                }
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            UserAction::ActionPressed { action_id } if action_id == "clear_exchange_place" => {
                let cleared = self.vauchi.clear_exchange_location(contact_id).is_ok();
                if cleared
                    && !self
                        .engine
                        .apply_update(crate::ui::EngineUpdate::ContactDetail(
                            crate::ui::ContactDetailUpdate::ClearExchangePlace,
                        ))
                {
                    return None;
                }
                Some(ActionResult::UpdateScreen(self.engine.current_screen()))
            }
            _ => None,
        }
    }

    /// Named-place names matching `prefix` (trimmed, case-insensitive) — the
    /// autocomplete source for naming a contact's exchange place.
    fn place_name_suggestions(&self, prefix: &str) -> Vec<String> {
        let needle = prefix.trim().to_lowercase();
        self.vauchi
            .list_places()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle))
            .map(|p| p.name)
            .collect()
    }
}
