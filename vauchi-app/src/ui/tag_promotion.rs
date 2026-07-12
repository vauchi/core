// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tag → group promotion engine (ADR-051 contact annotations).
//!
//! Reviews the `GroupDraft` produced by `Vauchi::begin_tag_promotion`: the
//! proposed group name, how many contacts will join, and which own-card
//! fields the new group will see (an editable `ToggleList`, mirroring the
//! group field-visibility review). Confirming calls
//! `Vauchi::confirm_tag_promotion` (replace semantics: it creates the group
//! and consumes the tag) — resolved by the AppEngine intercept, which reads
//! [`TagPromotionEngine::tag_id`] + [`TagPromotionEngine::selected_field_ids`].

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// One own-card field in the promotion review, with whether the new group
/// will be able to see it. Shape mirrors the group field-visibility row.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromotionField {
    pub field_id: String,
    pub label: String,
    pub value: String,
    pub selected: bool,
}

/// Component id of the field-review toggle list.
pub const PROMOTION_FIELDS_COMPONENT_ID: &str = "promotion_fields";
/// Action id of the confirm button.
pub const CONFIRM_PROMOTION_ACTION_ID: &str = "confirm_promotion";

/// Engine for the tag→group promotion review screen.
#[derive(Clone, Debug)]
pub struct TagPromotionEngine {
    tag_id: String,
    name: String,
    member_count: usize,
    fields: Vec<PromotionField>,
    locale: Locale,
}

impl TagPromotionEngine {
    pub fn new(
        tag_id: String,
        name: String,
        member_count: usize,
        fields: Vec<PromotionField>,
    ) -> Self {
        Self {
            tag_id,
            name,
            member_count,
            fields,
            locale: Locale::English,
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

    /// The tag being promoted — read by the AppEngine intercept to call
    /// `Vauchi::confirm_tag_promotion`.
    pub fn tag_id(&self) -> &str {
        &self.tag_id
    }

    /// Field ids the owner has left selected — the reviewed `visible_fields`
    /// passed to `confirm_tag_promotion`.
    pub fn selected_field_ids(&self) -> Vec<String> {
        self.fields
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.field_id.clone())
            .collect()
    }

    /// Flip the selection of one field (in-memory; no storage).
    pub fn toggle_field(&mut self, field_id: &str) {
        if let Some(f) = self.fields.iter_mut().find(|f| f.field_id == field_id) {
            f.selected = !f.selected;
        }
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components: Vec<Component> = Vec::new();

        let members = if self.member_count == 1 {
            self.t("tag_promotion.members_singular")
        } else {
            get_string_with_args(
                self.locale,
                "tag_promotion.members_plural",
                &[("count", &self.member_count.to_string())],
            )
        };
        components.push(Component::InfoPanel {
            id: "promotion_info".into(),
            icon: Some("people".into()),
            title: get_string_with_args(
                self.locale,
                "tag_promotion.promote_title",
                &[("name", &self.name)],
            ),
            items: vec![InfoItem {
                icon: Some("people".into()),
                title: self.t("group_detail.members_label"),
                detail: members,
            }],
            a11y: Some(A11y {
                label: Some(get_string_with_args(
                    self.locale,
                    "tag_promotion.promote_a11y",
                    &[("name", &self.name)],
                )),
                hint: None,
                role: Some(AccessibilityRole::Heading),
            }),
        });

        if !self.fields.is_empty() {
            components.push(Component::ToggleList {
                id: PROMOTION_FIELDS_COMPONENT_ID.into(),
                label: self.t("tag_promotion.fields_label"),
                items: self
                    .fields
                    .iter()
                    .map(|f| ToggleItem {
                        id: f.field_id.clone(),
                        label: f.label.clone(),
                        selected: f.selected,
                        subtitle: Some(f.value.clone()),
                        a11y: Some(A11y {
                            label: Some(get_string_with_args(
                                self.locale,
                                "group_detail.visibility_for_a11y",
                                &[("label", &f.label)],
                            )),
                            hint: Some(if f.selected {
                                self.t("tag_promotion.visible_to_new_group_hint")
                            } else {
                                self.t("tag_promotion.hidden_from_new_group_hint")
                            }),
                            role: None,
                        }),
                        info_key: None,
                    })
                    .collect(),
                a11y: Some(A11y {
                    label: Some(self.t("tag_promotion.field_visibility_a11y")),
                    hint: Some(self.t("tag_promotion.field_visibility_hint")),
                    role: None,
                }),
            });
        }

        ScreenModel {
            screen_id: "tag_promotion".into(),
            title: self.t("tag_promotion.title"),
            subtitle: None,
            components,
            actions: vec![ScreenAction {
                id: CONFIRM_PROMOTION_ACTION_ID.into(),
                label: self.t("tag_promotion.create_group_button"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for TagPromotionEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::TagPromotion {
            selected_field_ids: self.selected_field_ids(),
        })
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == PROMOTION_FIELDS_COMPONENT_ID => {
                self.toggle_field(&item_id);
                ActionResult::UpdateScreen(self.build_screen())
            }
            // `confirm_promotion` needs `Vauchi` and is resolved by the
            // AppEngine intercept; here it falls through to a no-op re-render.
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
