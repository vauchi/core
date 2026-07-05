// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tags engine — manage the owner-private tag vocabulary (ADR-051).
//!
//! Lists every tag with its member count and offers a per-row delete that
//! routes through an engine-owned `InlineConfirm` (mirrors the
//! contact-detail delete: a static `delete_tag` confirm id plus a
//! `pending_delete` state, so the BFS reachability walker dedupes the
//! confirm state by `screen_id`). Tag *creation* lives on ContactDetail
//! (the add-tag field), not here — this screen is purely management.
//!
//! The actual `Vauchi::delete_tag` call needs storage access, so the
//! `confirm_delete_tag` action is resolved by the AppEngine intercept,
//! which reads [`TagsEngine::pending_delete_id`] and then applies
//! [`TagsEngine::confirm_delete`].

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Summary of a tag for the management list.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TagSummary {
    pub id: String,
    pub name: String,
    pub member_count: usize,
}

/// Engine for the tag-management list.
#[derive(Clone, Debug)]
pub struct TagsEngine {
    tags: Vec<TagSummary>,
    /// Id of the tag pending a delete confirmation, if any.
    pending_delete: Option<String>,
    locale: Locale,
}

impl TagsEngine {
    pub fn new(tags: Vec<TagSummary>) -> Self {
        Self {
            tags,
            pending_delete: None,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-15).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Id of the tag awaiting delete confirmation — read by the AppEngine
    /// intercept so the `Vauchi::delete_tag` call knows its target.
    pub fn pending_delete_id(&self) -> Option<&str> {
        self.pending_delete.as_deref()
    }

    /// Apply a confirmed delete: drop the row and clear the pending state.
    /// Called by the AppEngine intercept after `Vauchi::delete_tag` succeeds.
    pub fn confirm_delete(&mut self) {
        if let Some(id) = self.pending_delete.take() {
            self.tags.retain(|t| t.id != id);
        }
    }

    fn pending_name(&self) -> Option<&str> {
        let id = self.pending_delete.as_deref()?;
        self.tags
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.name.as_str())
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        let items: Vec<Item> = self
            .tags
            .iter()
            .map(|t| {
                let detail = if t.member_count == 1 {
                    self.t("tags_list.member_count_singular")
                } else {
                    get_string_with_args(
                        self.locale,
                        "tags_list.member_count_plural",
                        &[("count", &t.member_count.to_string())],
                    )
                };
                Item {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    subtitle: Some(detail),
                    avatar_initials: String::new(),
                    status: None,
                    actions: vec![
                        ListItemAction {
                            id: "promote".into(),
                            label: self.t("tags_list.promote_button"),
                            kind: ListItemActionKind::Custom,
                            destructive: false,
                        },
                        ListItemAction {
                            id: "request_delete".into(),
                            label: self.t("action.delete"),
                            kind: ListItemActionKind::Custom,
                            destructive: false,
                        },
                    ],
                    a11y: None,
                }
            })
            .collect();

        components.push(Component::List {
            id: "tags".into(),
            items,
            searchable: false,
            total_count: 0,
            offset: 0,
            window: 0,
        });

        if let Some(name) = self.pending_name() {
            components.push(Component::InlineConfirm {
                id: "delete_tag".into(),
                warning: get_string_with_args(
                    self.locale,
                    "tags_list.delete_warning",
                    &[("name", name)],
                ),
                confirm_text: self.t("action.delete"),
                cancel_text: self.t("action.cancel"),
                destructive: true,
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "tags".into(),
            title: self.t("more.tags"),
            subtitle: None,
            components,
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for TagsEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::Tags {
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
            // Per-row delete affordance → arm the confirmation.
            UserAction::ListItemAction {
                component_id,
                item_id,
                action_id,
            } if component_id == "tags" && action_id == "request_delete" => {
                self.pending_delete = Some(item_id);
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Cancel the armed confirmation.
            UserAction::ActionPressed { action_id } if action_id == "cancel_delete_tag" => {
                self.pending_delete = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            // `confirm_delete_tag` needs `Vauchi` and is resolved by the
            // AppEngine intercept; here it falls through to a no-op re-render.
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
