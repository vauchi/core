// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact detail engine — view a single contact with a perspective
//! choice between "their info I can see" and "my info they can see".

use std::collections::HashMap;

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
// `footer_action_id` is re-exported from `crate::ui` under the renamed
// alias `contact_detail_footer_action_id`, so the glob above does not bind
// the bare name the call site uses — import it directly. The other
// predicates (`verify_button_visible`, `show_verified_badge`,
// `show_recovery_trusted_indicator`) come in via the glob unchanged.
use crate::ui::contact_detail_rules::{
    ContactPlace, ContactTag, footer_action_id, place_components, tag_components,
};
use vauchi_core::contact::trust::TrustLevel;

/// Which perspective the user is viewing.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum ContactViewMode {
    /// Their shared fields (default — what they share with me).
    TheirInfo,
    /// My fields as visible to this contact (what I share with them).
    MyInfoForThem,
}

/// Data needed to show "my info they can see".
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SharedInfoView {
    /// The display name this contact sees (override or default).
    pub shared_display_name: String,
    /// My fields with visibility state for this contact.
    pub my_fields: Vec<Field>,
    /// Group names that grant this contact visibility to my fields.
    pub visible_groups: Vec<String>,
}

/// Summary of card update delivery status for a contact.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeliverySummary {
    pub total: usize,
    pub delivered: usize,
    pub pending: usize,
    pub failed: usize,
}

/// Read-only engine that displays a single contact's details with a
/// perspective toggle.
#[derive(Clone, Debug)]
pub struct ContactDetailEngine {
    contact: Item,
    fields: Vec<Field>,
    shared_info: Option<SharedInfoView>,
    view_mode: ContactViewMode,
    /// Private note about this contact (never shared). Stored as plain UTF-8.
    personal_note: String,
    /// Per-field private notes (never shared). Keyed by field_id, plain UTF-8.
    field_notes: HashMap<String, String>,
    /// Component currently in edit mode. This presentation transition is
    /// core-owned so every renderer receives the same declarative state.
    editing_component_id: Option<String>,
    /// Core-owned draft for `editing_component_id`. Frontends report raw text;
    /// Save commits this value and Cancel discards it.
    editing_value: Option<String>,
    /// Computed trust level display string (read-only).
    trust_level: String,
    /// Trust level enum used to gate `verify_fingerprint` action via
    /// `verify_button_visible(is_verified, trust_level_enum)`. Defaults to
    /// `Standard` so legacy callers that only pass the display string get
    /// the same visibility behaviour as before this field existed.
    trust_level_enum: TrustLevel,
    /// Exchange reciprocity status display string (read-only).
    reciprocity_status: String,
    /// Whether the user has manually verified this contact's fingerprint
    /// — gates the "Verify" affordance per `verify_button_visible`.
    is_verified: bool,
    /// Hex / formatted fingerprint string for the InfoPanel (G6 added
    /// 2026-04-28 to close the Pair 3 ContactDetail engine gap).
    fingerprint: String,
    /// Whether this contact is configured as a recovery trustee.
    /// Drives both the `Recovery Trusted` indicator and the
    /// `recovery_permissions` SettingsGroup toggle.
    is_recovery_trusted: bool,
    /// Whether this contact is trusted for simplified contact proposals (user-editable).
    proposal_trusted: bool,
    /// Whether this contact is hidden from the main contact list.
    is_hidden: bool,
    /// Whether this is an imported (non-crypto) contact vs. exchanged.
    is_imported: bool,
    /// Whether this contact is ignored (ADR-072: silent, reversible).
    is_ignored: bool,
    /// Card update delivery status for this contact (J1 MVP).
    delivery_summary: Option<DeliverySummary>,
    /// Whether the user has pressed "Delete" and the InlineConfirm is showing.
    pending_delete: bool,
    /// Avatar image bytes (WebP) for the ImageCircle component.
    avatar_data: Option<Vec<u8>>,
    /// Owner-private tags applied to this contact (ADR-051), rendered as a
    /// removable list. Loaded by the screen factory via
    /// `Vauchi::tags_for_contact`.
    tags: Vec<ContactTag>,
    /// Transient in-progress add-tag query (never persisted).
    tag_input: String,
    /// Transient autocomplete suggestions for `tag_input`, computed by the
    /// AppEngine intercept via `Vauchi::tag_name_suggestions`.
    tag_suggestions: Vec<String>,
    /// The contact's recorded exchange place (ADR-051), or `None` when no
    /// location was captured. Loaded by the screen factory.
    exchange_place: Option<ContactPlace>,
    /// Transient in-progress place-name query (never persisted).
    place_input: String,
    /// Transient name suggestions for `place_input`, from the named-place
    /// vocabulary, computed by the AppEngine intercept.
    place_suggestions: Vec<String>,
    locale: Locale,
}

#[path = "contact_detail_builders.rs"]
mod builders;

impl ContactDetailEngine {
    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Toggles hidden state in-memory. Callers must persist via Vauchi.
    pub fn toggle_hidden(&mut self) {
        self.is_hidden = !self.is_hidden;
    }

    /// Returns the current hidden state.
    pub fn is_hidden(&self) -> bool {
        self.is_hidden
    }

    /// Toggles ignored state in-memory. Callers must persist via Vauchi.
    pub fn toggle_ignored(&mut self) {
        self.is_ignored = !self.is_ignored;
    }

    /// Returns the current ignored state.
    pub fn is_ignored(&self) -> bool {
        self.is_ignored
    }

    /// Returns the current proposal_trusted flag.
    pub fn proposal_trusted(&self) -> bool {
        self.proposal_trusted
    }

    /// Toggles proposal_trusted in-memory. Callers must persist via Vauchi.
    pub fn toggle_proposal_trusted(&mut self) {
        self.proposal_trusted = !self.proposal_trusted;
    }

    /// Returns the current view mode.
    pub fn view_mode(&self) -> &ContactViewMode {
        &self.view_mode
    }

    fn build_screen(&self) -> ScreenModel {
        let mut components = Vec::new();

        // Perspective choice — only shown when shared info is available
        if self.shared_info.is_some() {
            components.push(self.perspective_choice());
        }

        match self.view_mode {
            ContactViewMode::TheirInfo => components.extend(self.their_info_components()),
            ContactViewMode::MyInfoForThem => components.extend(self.my_info_components()),
        }

        // InlineConfirm for irrevocable delete (imported contacts only)
        if self.pending_delete {
            components.push(self.delete_confirm());
        }

        let title = match self.view_mode {
            ContactViewMode::TheirInfo => self.contact.name.clone(),
            ContactViewMode::MyInfoForThem => get_string_with_args(
                self.locale,
                "contact_detail.shared_with_title",
                &[("name", &self.contact.name)],
            ),
        };

        ScreenModel {
            screen_id: "contact_detail".into(),
            title,
            subtitle: self.contact.subtitle.clone(),
            components,
            contextual_actions: self.build_actions(),
            progress: None,
            ..Default::default()
        }
    }

    fn perspective_choice(&self) -> Component {
        let selected = match self.view_mode {
            ContactViewMode::TheirInfo => "their_info",
            ContactViewMode::MyInfoForThem => "my_info_for_them",
        };
        Component::Dropdown {
            id: "view_mode".into(),
            label: self.t("contact_detail.perspective_label"),
            selected: Some(selected.into()),
            options: vec![
                DropdownOption {
                    id: "their_info".into(),
                    label: self.t("contact_detail.their_info_label"),
                },
                DropdownOption {
                    id: "my_info_for_them".into(),
                    label: self.t("contact_detail.my_info_for_them_label"),
                },
            ],
            a11y: Some(A11y {
                label: Some(self.t("contact_detail.perspective_options_a11y")),
                hint: None,
                role: None,
            }),
        }
    }

    fn their_info_components(&self) -> Vec<Component> {
        let mut components = Vec::new();
        // Avatar preview at top
        components.push(Component::ImageCircle {
            id: "avatar".into(),
            image_data: self.avatar_data.clone(),
            initials: self.contact.initials.clone(),
            bg_color: None,
            brightness: 0.0,
            editable: false,
            edit_action_id: None,
            a11y: Some(A11y {
                label: Some(get_string_with_args(
                    self.locale,
                    "contact_detail.avatar_a11y",
                    &[("name", &self.contact.name)],
                )),
                hint: None,
                role: Some(AccessibilityRole::Image),
            }),
        });
        components.push(self.contact_info_panel());
        components.extend(self.their_fields_components());
        // Private note about the contact — only visible to me, never shared
        components.push(Component::EditableText {
            id: "personal_note".into(),
            label: self.t("contact_detail.private_note_label"),
            value: if self.editing_component_id.as_deref() == Some("personal_note") {
                self.editing_value
                    .clone()
                    .unwrap_or_else(|| self.personal_note.clone())
            } else {
                self.personal_note.clone()
            },
            edit_text: self.t("action.edit"),
            save_text: self.t("action.save"),
            cancel_text: self.t("action.cancel"),
            edit_action_id: "edit_personal_note".into(),
            save_action_id: "save_personal_note".into(),
            cancel_action_id: "cancel_personal_note".into(),
            editing: self.editing_component_id.as_deref() == Some("personal_note"),
            validation_error: None,
            a11y: Some(A11y {
                label: Some(self.t("contact_detail.personal_note_a11y")),
                hint: Some(self.t("contact_detail.double_tap_to_edit_hint")),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        });
        // Owner-private tags (ADR-051): removable list + add input + suggestions
        components.extend(tag_components(
            &self.tags,
            &self.tag_input,
            &self.tag_suggestions,
            self.locale,
        ));
        // Exchange place (ADR-051): label + name input + suggestions + clear.
        components.extend(place_components(
            &self.exchange_place,
            &self.place_input,
            &self.place_suggestions,
            self.locale,
        ));
        // Trust & permissions group (local-only, never shared with the contact)
        components.push(Component::SettingsGroup {
            id: "trust_permissions".into(),
            label: self.t("contact_detail.trust_permissions_label"),
            items: vec![SettingsItem {
                id: "proposal_trusted".into(),
                subtitle: None,
                label: self.t("contact_detail.can_propose_contacts_label"),
                kind: SettingsItemKind::Toggle {
                    enabled: self.proposal_trusted,
                },
                a11y: Some(A11y {
                    label: Some(self.t("contact_detail.can_propose_contacts_a11y")),
                    hint: Some(self.t("contact_detail.can_propose_contacts_hint")),
                    role: None,
                }),
                info_key: None,
            }],
        });
        // Recovery permissions group — gate the recovery-trustee toggle
        // (Pair 3 ContactDetail engine extension, 2026-04-28).
        components.push(Component::SettingsGroup {
            id: "recovery_permissions".into(),
            label: self.t("contact_detail.recovery_label"),
            items: vec![SettingsItem {
                id: "recovery_trusted".into(),
                subtitle: None,
                label: self.t("contact_detail.trust_for_recovery_label"),
                kind: SettingsItemKind::Toggle {
                    enabled: self.is_recovery_trusted,
                },
                a11y: Some(A11y {
                    label: Some(self.t("contact_detail.trust_for_recovery_a11y")),
                    hint: Some(self.t("contact_detail.trust_for_recovery_hint")),
                    role: None,
                }),
                info_key: None,
            }],
        });
        if let Some(panel) = self.delivery_status_panel() {
            components.push(panel);
        }
        components
    }

    fn contact_info_panel(&self) -> Component {
        // Status rows only: the avatar already shows the initials and the
        // raw fingerprint is shown by Verify Fingerprint, where it is
        // compared rather than read.
        let mut contact_info_items = Vec::new();
        if !self.trust_level.is_empty() {
            contact_info_items.push(InfoItem {
                icon: Some("checkmark.shield".into()),
                title: self.t("contact_detail.trust_label"),
                detail: self.trust_level.clone(),
            });
        }
        if show_verified_badge(self.is_verified) {
            contact_info_items.push(InfoItem {
                icon: Some("checkmark.seal".into()),
                title: self.t("contacts.verified"),
                detail: self.t("generic.yes"),
            });
        }
        if show_recovery_trusted_indicator(self.is_recovery_trusted) {
            contact_info_items.push(InfoItem {
                icon: Some("shield".into()),
                title: self.t("contact_detail.recovery_trusted_label"),
                detail: self.t("generic.yes"),
            });
        }
        if !self.reciprocity_status.is_empty() {
            contact_info_items.push(InfoItem {
                icon: Some("arrow.left.arrow.right".into()),
                title: self.t("contact_detail.exchange_status_label"),
                detail: self.reciprocity_status.clone(),
            });
        }
        Component::InfoPanel {
            id: "contact_info".into(),
            icon: None,
            title: self.contact.name.clone(),
            items: contact_info_items,
            a11y: Some(A11y {
                label: Some(self.contact.name.clone()),
                hint: None,
                role: Some(AccessibilityRole::Heading),
            }),
        }
    }

    fn their_fields_components(&self) -> Vec<Component> {
        let mut components = Vec::new();
        // Their fields — read-only, no visibility column.
        // Each field is followed by an inline-editable private note.
        let contact_fields_a11y = self.t("fields.a11y_contact_fields");
        for field in &self.fields {
            components.push(Component::FieldList {
                id: format!("field_{}", field.id),
                title: contact_fields_a11y.clone(),
                fields: vec![field.clone()],
                visibility_mode: VisibilityMode::ReadOnly,
                available_scopes: vec![],
                a11y: Some(A11y {
                    label: Some(contact_fields_a11y.clone()),
                    hint: None,
                    role: None,
                }),
            });
            let note_value = self.field_notes.get(&field.id).cloned().unwrap_or_default();
            let component_id = format!("field_note:{}", field.id);
            components.push(Component::EditableText {
                id: component_id.clone(),
                label: self.t("contact_detail.private_field_note_label"),
                value: if self.editing_component_id.as_deref() == Some(component_id.as_str()) {
                    self.editing_value.clone().unwrap_or(note_value)
                } else {
                    note_value
                },
                edit_text: self.t("action.edit"),
                save_text: self.t("action.save"),
                cancel_text: self.t("action.cancel"),
                edit_action_id: format!("edit_field_note:{}", field.id),
                save_action_id: format!("save_field_note:{}", field.id),
                cancel_action_id: format!("cancel_field_note:{}", field.id),
                editing: self.editing_component_id.as_deref() == Some(component_id.as_str()),
                validation_error: None,
                a11y: Some(A11y {
                    label: Some(self.t("contact_detail.private_field_note_a11y")),
                    hint: Some(self.t("contact_detail.double_tap_to_edit_hint")),
                    role: Some(AccessibilityRole::TextField),
                }),
                info_key: None,
            });
        }
        components
    }

    fn delivery_status_panel(&self) -> Option<Component> {
        let summary = self.delivery_summary.as_ref()?;
        let mut items = Vec::new();
        if summary.failed == 0 && summary.pending == 0 {
            items.push(InfoItem {
                icon: None,
                title: self.t("contacts.status"),
                detail: self.t("contact_detail.all_delivered_detail"),
            });
        } else {
            items.push(InfoItem {
                icon: None,
                title: self.t("delivery.status_delivered"),
                detail: summary.delivered.to_string(),
            });
            if summary.pending > 0 {
                items.push(InfoItem {
                    icon: None,
                    title: self.t("delivery.pending_tab"),
                    detail: summary.pending.to_string(),
                });
            }
            if summary.failed > 0 {
                items.push(InfoItem {
                    icon: None,
                    title: self.t("delivery.status_failed"),
                    detail: summary.failed.to_string(),
                });
            }
        }
        Some(Component::InfoPanel {
            id: "delivery_status".into(),
            icon: None,
            title: self.t("contact_detail.update_delivery_title"),
            items,
            a11y: Some(A11y {
                label: Some(self.t("contact_detail.update_delivery_title")),
                hint: None,
                role: Some(AccessibilityRole::Heading),
            }),
        })
    }

    fn my_info_components(&self) -> Vec<Component> {
        let mut components = Vec::new();
        if let Some(ref shared) = self.shared_info {
            components.push(Component::InfoPanel {
                id: "shared_name_info".into(),
                icon: None,
                title: self.t("contact_detail.they_see_me_as_title"),
                items: vec![InfoItem {
                    icon: None,
                    title: self.t("settings.display_name"),
                    detail: shared.shared_display_name.clone(),
                }],
                a11y: Some(A11y {
                    label: Some(self.t("contact_detail.they_see_me_as_title")),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            });
            // My fields — show which groups grant visibility
            components.push(Component::FieldList {
                id: "my_fields".into(),
                title: self.t("fields.a11y_contact_fields"),
                fields: shared.my_fields.clone(),
                visibility_mode: VisibilityMode::PerGroup,
                available_scopes: shared.visible_groups.clone(),
                a11y: Some(A11y {
                    label: Some(self.t("fields.a11y_contact_fields")),
                    hint: Some(self.t("contact_detail.manage_group_visibility_hint")),
                    role: None,
                }),
            });
        }
        components
    }

    fn delete_confirm(&self) -> Component {
        Component::InlineConfirm {
            id: "delete_contact".into(),
            warning: get_string_with_args(
                self.locale,
                "contact_detail.delete_confirm_warning",
                &[("name", &self.contact.name)],
            ),
            confirm_text: self.t("action.delete"),
            cancel_text: self.t("action.cancel"),
            confirm_action_id: "confirm_delete_contact".into(),
            cancel_action_id: "cancel_delete_contact".into(),
            destructive: true,
            a11y: Some(A11y {
                label: Some(self.t("contact_detail.confirm_deletion_a11y")),
                hint: Some(self.t("contact_detail.confirm_deletion_hint")),
                role: Some(AccessibilityRole::Alert),
            }),
        }
    }

    fn build_actions(&self) -> Vec<ScreenAction> {
        let mut actions: Vec<ScreenAction> = Vec::new();
        let edit_label = self.t("action.edit");
        actions.push(ScreenAction {
            id: "edit".into(),
            label: edit_label.clone(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: Some(A11y::labeled(edit_label)),
        });
        if verify_button_visible(self.is_verified, self.trust_level_enum) {
            let verify_label = self.t("contact_detail.verify_fingerprint_button");
            actions.push(ScreenAction {
                id: "verify_fingerprint".into(),
                label: verify_label.clone(),
                style: ActionStyle::Serious,
                enabled: true,
                a11y: Some(A11y::labeled(verify_label)),
            });
        }
        let toggle_hidden_label = if self.is_hidden {
            self.t("contact_detail.unhide_button")
        } else {
            self.t("contact_detail.hide_button")
        };
        actions.push(ScreenAction {
            id: "toggle_hidden".into(),
            label: toggle_hidden_label.clone(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y::labeled(toggle_hidden_label)),
        });
        let (ignore_action_id, ignore_label) = if self.is_ignored {
            (
                "unignore_contact",
                self.t("contact_detail.unignore_contact_button"),
            )
        } else {
            (
                "ignore_contact",
                self.t("contact_detail.ignore_contact_button"),
            )
        };
        actions.push(ScreenAction {
            id: ignore_action_id.into(),
            label: ignore_label.clone(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y::labeled(ignore_label)),
        });
        let footer_label = if self.is_imported {
            self.t("contact_detail.delete_contact_button")
        } else {
            self.t("contact_detail.archive_contact_button")
        };
        actions.push(ScreenAction {
            id: footer_action_id(self.is_imported).into(),
            label: footer_label.clone(),
            style: if self.is_imported {
                ActionStyle::Destructive
            } else {
                ActionStyle::Secondary
            },
            enabled: true,
            a11y: Some(A11y::labeled(footer_label)),
        });
        // Back is the frontend's job now: every frontend renders a
        // core-driven back affordance from `can_go_back` (2026-06-05-
        // core-driven-back-chrome). Dropping the footer "Back" leaves
        // Edit (primary) + a small secondary/destructive set.
        actions
    }
}

impl WorkflowEngine for ContactDetailEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::ContactDetail {
            is_hidden: self.is_hidden(),
        })
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        use crate::ui::ContactDetailUpdate as U;
        let crate::ui::EngineUpdate::ContactDetail(update) = update else {
            return false;
        };
        match update {
            U::ToggleProposalTrusted => self.toggle_proposal_trusted(),
            U::ToggleRecoveryTrusted => self.toggle_recovery_trusted(),
            U::ToggleHidden => self.toggle_hidden(),
            U::ToggleIgnored => self.toggle_ignored(),
            U::TagQuery { query, suggestions } => self.set_tag_query(query, suggestions),
            U::TagAdded(tag) => self.add_tag_row(tag),
            U::TagRemoved(tag_id) => self.remove_tag_row(&tag_id),
            U::PlaceQuery { query, suggestions } => self.set_place_query(query, suggestions),
            U::PlaceNamed(name) => self.set_place_named(name),
            U::ClearExchangePlace => self.clear_exchange_place(),
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            // Perspective choice (one exclusive Choice node, ADR-066)
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == "view_mode" => {
                self.view_mode = match item_id.as_str() {
                    "their_info" => ContactViewMode::TheirInfo,
                    "my_info_for_them" => ContactViewMode::MyInfoForThem,
                    _ => return ActionResult::UpdateScreen(self.build_screen()),
                };
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Proposal trust toggle (local state; AppEngine intercept persists to storage)
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "trust_permissions" && item_id == "proposal_trusted" => {
                self.proposal_trusted = !self.proposal_trusted;
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Recovery trust toggle (local state; AppEngine intercept persists)
            UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } if component_id == "recovery_permissions" && item_id == "recovery_trusted" => {
                self.is_recovery_trusted = !self.is_recovery_trusted;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "edit" => {
                ActionResult::EditContact {
                    contact_id: self.contact.id.clone(),
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "verify_fingerprint" => {
                ActionResult::VerifyFingerprint {
                    contact_id: self.contact.id.clone(),
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "edit_personal_note" => {
                self.editing_component_id = Some("personal_note".into());
                self.editing_value = Some(self.personal_note.clone());
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if self.editing_component_id.as_deref() == Some(component_id.as_str()) => {
                self.editing_value = Some(value);
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "save_personal_note" => {
                if self.editing_component_id.as_deref() == Some("personal_note")
                    && let Some(value) = self.editing_value.take()
                {
                    self.personal_note = value;
                }
                self.editing_component_id = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "cancel_personal_note" => {
                self.editing_component_id = None;
                self.editing_value = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id }
                if action_id.starts_with("edit_field_note:") =>
            {
                let field_id = action_id.trim_start_matches("edit_field_note:");
                if self.fields.iter().any(|field| field.id == field_id) {
                    let component_id = format!("field_note:{field_id}");
                    self.editing_component_id = Some(component_id);
                    self.editing_value =
                        Some(self.field_notes.get(field_id).cloned().unwrap_or_default());
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id }
                if action_id.starts_with("save_field_note:") =>
            {
                let field_id = action_id.trim_start_matches("save_field_note:");
                let component_id = format!("field_note:{field_id}");
                if self.editing_component_id.as_deref() == Some(component_id.as_str())
                    && let Some(value) = self.editing_value.take()
                {
                    self.field_notes.insert(field_id.into(), value);
                }
                self.editing_component_id = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id }
                if action_id.starts_with("cancel_field_note:") =>
            {
                self.editing_component_id = None;
                self.editing_value = None;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "delete_contact" => {
                self.pending_delete = true;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "confirm_delete_contact" => {
                self.pending_delete = false;
                ActionResult::Complete
            }
            UserAction::ActionPressed { action_id } if action_id == "cancel_delete_contact" => {
                self.pending_delete = false;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "archive_contact" => {
                ActionResult::ShowToast {
                    message: self.t("contacts.toast_archived"),
                    undo_action_id: Some(format!("undo_archive_contact:{}", self.contact.id)),
                    undo_label: Some(self.t("action.undo")),
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

#[path = "contact_detail_not_found.rs"]
mod not_found;
pub use not_found::ContactNotFoundEngine;

// INLINE_TEST_REQUIRED: tests access private ContactViewMode and
// ContactDetailEngine internals. Extracted to contact_detail_tests.rs
// to keep this file under the 1000-line src hard limit (M3 S5-12).
#[cfg(test)]
#[path = "contact_detail_tests.rs"]
mod tests;
