// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact detail engine — view a single contact with a toggle between
//! "their info I can see" and "my info they can see".

use std::collections::HashMap;

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
    /// Card update delivery status for this contact (J1 MVP).
    delivery_summary: Option<DeliverySummary>,
    /// Whether the user has pressed "Delete" and the InlineConfirm is showing.
    pending_delete: bool,
    /// Avatar image bytes (WebP) for the AvatarPreview component.
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
}

impl ContactDetailEngine {
    /// Create with only their info (no shared info available).
    pub fn new(contact: Item, fields: Vec<Field>, personal_note: String) -> Self {
        Self {
            contact,
            fields,
            shared_info: None,
            view_mode: ContactViewMode::TheirInfo,
            personal_note,
            field_notes: HashMap::new(),
            trust_level: String::new(),
            trust_level_enum: TrustLevel::Standard,
            reciprocity_status: String::new(),
            is_verified: false,
            fingerprint: String::new(),
            is_recovery_trusted: false,
            proposal_trusted: false,
            is_hidden: false,
            is_imported: false,
            delivery_summary: None,
            pending_delete: false,
            avatar_data: None,
            tags: Vec::new(),
            tag_input: String::new(),
            tag_suggestions: Vec::new(),
            exchange_place: None,
            place_input: String::new(),
            place_suggestions: Vec::new(),
        }
    }

    /// Create with both perspectives available.
    pub fn with_shared_info(
        contact: Item,
        fields: Vec<Field>,
        shared_info: SharedInfoView,
        personal_note: String,
    ) -> Self {
        Self {
            contact,
            fields,
            shared_info: Some(shared_info),
            view_mode: ContactViewMode::TheirInfo,
            personal_note,
            field_notes: HashMap::new(),
            trust_level: String::new(),
            trust_level_enum: TrustLevel::Standard,
            reciprocity_status: String::new(),
            is_verified: false,
            fingerprint: String::new(),
            is_recovery_trusted: false,
            proposal_trusted: false,
            is_hidden: false,
            is_imported: false,
            delivery_summary: None,
            pending_delete: false,
            avatar_data: None,
            tags: Vec::new(),
            tag_input: String::new(),
            tag_suggestions: Vec::new(),
            exchange_place: None,
            place_input: String::new(),
            place_suggestions: Vec::new(),
        }
    }

    /// Attach delivery status summary for card updates to this contact.
    pub fn with_delivery_summary(mut self, summary: DeliverySummary) -> Self {
        self.delivery_summary = Some(summary);
        self
    }

    /// Attach per-field notes loaded from storage.
    pub fn with_field_notes(mut self, field_notes: HashMap<String, String>) -> Self {
        self.field_notes = field_notes;
        self
    }

    /// Attach the contact's owner-private tags (ADR-051), rendered as a
    /// removable list. Loaded by the screen factory via
    /// `Vauchi::tags_for_contact`.
    pub fn with_tags(mut self, tags: Vec<ContactTag>) -> Self {
        self.tags = tags;
        self
    }

    /// Set the in-progress add-tag query and its autocomplete suggestions.
    /// Called by the AppEngine intercept on each keystroke after computing
    /// suggestions via `Vauchi::tag_name_suggestions`; transient state that
    /// is never persisted and is cleared once a tag is committed.
    pub fn set_tag_query(&mut self, query: String, suggestions: Vec<String>) {
        self.tag_input = query;
        self.tag_suggestions = suggestions;
    }

    /// Attach the contact's recorded exchange place (ADR-051). Loaded by the
    /// screen factory from `Vauchi::exchange_location`.
    pub fn with_exchange_place(mut self, place: Option<ContactPlace>) -> Self {
        self.exchange_place = place;
        self
    }

    /// Set the in-progress place-name query and its suggestions (transient;
    /// the AppEngine intercept computes the suggestions from the named-place
    /// vocabulary on each keystroke).
    pub fn set_place_query(&mut self, query: String, suggestions: Vec<String>) {
        self.place_input = query;
        self.place_suggestions = suggestions;
    }

    /// Optimistically record that the exchange place was named, after a
    /// successful `Vauchi::name_exchange_place`, and clear the query.
    pub fn set_place_named(&mut self, name: String) {
        self.exchange_place = Some(ContactPlace { name: Some(name) });
        self.place_input.clear();
        self.place_suggestions.clear();
    }

    /// Optimistically clear the exchange place, after a successful
    /// `Vauchi::clear_exchange_location`.
    pub fn clear_exchange_place(&mut self) {
        self.exchange_place = None;
        self.place_input.clear();
        self.place_suggestions.clear();
    }

    /// Optimistically add a tag row after a successful
    /// `Vauchi::add_tag_to_contact`, and clear the in-progress query.
    /// Idempotent by id, so re-adding an existing tag (autocomplete-or-create
    /// returning the same tag) does not duplicate the row.
    pub fn add_tag_row(&mut self, tag: ContactTag) {
        if !self.tags.iter().any(|t| t.id == tag.id) {
            self.tags.push(tag);
        }
        self.tag_input.clear();
        self.tag_suggestions.clear();
    }

    /// Optimistically remove a tag row after a successful
    /// `Vauchi::remove_tag_from_contact`.
    pub fn remove_tag_row(&mut self, tag_id: &str) {
        self.tags.retain(|t| t.id != tag_id);
    }

    /// Attach trust data (trust level label and proposal_trusted flag).
    pub fn with_trust(mut self, trust_level: String, proposal_trusted: bool) -> Self {
        self.trust_level = trust_level;
        self.proposal_trusted = proposal_trusted;
        self
    }

    /// Attach reciprocity status string.
    pub fn with_reciprocity(mut self, status: String) -> Self {
        self.reciprocity_status = status;
        self
    }

    /// Attach hidden state.
    pub fn with_hidden(mut self, is_hidden: bool) -> Self {
        self.is_hidden = is_hidden;
        self
    }

    /// Attach imported flag (true for imported contacts, false for exchanged).
    pub fn with_imported(mut self, is_imported: bool) -> Self {
        self.is_imported = is_imported;
        self
    }

    /// Set the avatar image data for the AvatarPreview component.
    pub fn with_avatar_data(mut self, data: Option<Vec<u8>>) -> Self {
        self.avatar_data = data;
        self
    }

    /// Attach the verification flag + the canonical TrustLevel enum so
    /// `verify_button_visible` can gate the `verify_fingerprint` action.
    pub fn with_verification(mut self, is_verified: bool, trust_level_enum: TrustLevel) -> Self {
        self.is_verified = is_verified;
        self.trust_level_enum = trust_level_enum;
        self
    }

    /// Attach the fingerprint string for the contact_info InfoPanel.
    pub fn with_fingerprint(mut self, fingerprint: String) -> Self {
        self.fingerprint = fingerprint;
        self
    }

    /// Attach the recovery-trusted flag. Drives the Recovery Trusted
    /// indicator and the recovery_permissions SettingsGroup toggle.
    pub fn with_recovery_trusted(mut self, is_recovery_trusted: bool) -> Self {
        self.is_recovery_trusted = is_recovery_trusted;
        self
    }

    /// Returns whether this contact is imported (non-crypto).
    pub fn is_imported(&self) -> bool {
        self.is_imported
    }

    /// Flip the in-memory `is_recovery_trusted` flag — called by AppEngine
    /// intercept after a successful `vauchi.trust_contact_for_recovery` /
    /// `untrust_contact_for_recovery` call. Mirror of `toggle_proposal_trusted`.
    pub fn toggle_recovery_trusted(&mut self) {
        self.is_recovery_trusted = !self.is_recovery_trusted;
    }
}

impl ContactDetailEngine {
    /// Toggles hidden state in-memory. Callers must persist via Vauchi.
    pub fn toggle_hidden(&mut self) {
        self.is_hidden = !self.is_hidden;
    }

    /// Returns the current hidden state.
    pub fn is_hidden(&self) -> bool {
        self.is_hidden
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

        // Mode toggle — only shown when shared info is available
        if self.shared_info.is_some() {
            components.push(self.perspective_toggle());
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
            ContactViewMode::MyInfoForThem => {
                format!("Shared with {}", self.contact.name)
            }
        };

        ScreenModel {
            screen_id: "contact_detail".into(),
            title,
            subtitle: self.contact.subtitle.clone(),
            components,
            actions: self.build_actions(),
            progress: None,
            ..Default::default()
        }
    }

    fn perspective_toggle(&self) -> Component {
        let their_info_selected = self.view_mode == ContactViewMode::TheirInfo;
        let my_info_selected = self.view_mode == ContactViewMode::MyInfoForThem;
        Component::ToggleList {
            id: "view_mode".into(),
            label: "Perspective".into(),
            items: vec![
                ToggleItem {
                    id: "their_info".into(),
                    label: "Their Info".into(),
                    selected: their_info_selected,
                    subtitle: Some("What they share with me".into()),
                    a11y: Some(A11y {
                        label: Some(format!(
                            "Their Info, {}",
                            if their_info_selected {
                                "selected"
                            } else {
                                "not selected"
                            }
                        )),
                        hint: Some("Double tap to toggle".into()),
                        role: Some(AccessibilityRole::Toggle),
                    }),
                    info_key: None,
                },
                ToggleItem {
                    id: "my_info_for_them".into(),
                    label: "My Info for Them".into(),
                    selected: my_info_selected,
                    subtitle: Some("What I share with them".into()),
                    a11y: Some(A11y {
                        label: Some(format!(
                            "My Info for Them, {}",
                            if my_info_selected {
                                "selected"
                            } else {
                                "not selected"
                            }
                        )),
                        hint: Some("Double tap to toggle".into()),
                        role: Some(AccessibilityRole::Toggle),
                    }),
                    info_key: None,
                },
            ],
            a11y: Some(A11y {
                label: Some("Perspective options".into()),
                hint: Some("Select items to include".into()),
                role: None,
            }),
        }
    }

    fn their_info_components(&self) -> Vec<Component> {
        let mut components = Vec::new();
        // Avatar preview at top
        components.push(Component::AvatarPreview {
            id: "avatar".into(),
            image_data: self.avatar_data.clone(),
            initials: self.contact.avatar_initials.clone(),
            bg_color: None,
            brightness: 0.0,
            editable: false,
            a11y: Some(A11y {
                label: Some(format!("{}'s avatar", self.contact.name)),
                hint: None,
                role: Some(AccessibilityRole::Image),
            }),
        });
        components.push(self.contact_info_panel());
        components.extend(self.their_fields_components());
        // Private note about the contact — only visible to me, never shared
        components.push(Component::EditableText {
            id: "personal_note".into(),
            label: "Private note".into(),
            value: self.personal_note.clone(),
            editing: false,
            validation_error: None,
            a11y: Some(A11y {
                label: Some("Personal note, editable".into()),
                hint: Some("Double tap to edit".into()),
                role: Some(AccessibilityRole::TextField),
            }),
            info_key: None,
        });
        // Owner-private tags (ADR-051): removable list + add input + suggestions
        components.extend(tag_components(
            &self.tags,
            &self.tag_input,
            &self.tag_suggestions,
        ));
        // Exchange place (ADR-051): label + name input + suggestions + clear.
        components.extend(place_components(
            &self.exchange_place,
            &self.place_input,
            &self.place_suggestions,
        ));
        // Trust & permissions group (local-only, never shared with the contact)
        components.push(Component::SettingsGroup {
            id: "trust_permissions".into(),
            label: "Trust & Permissions".into(),
            items: vec![SettingsItem {
                id: "proposal_trusted".into(),
                label: "Can propose contacts".into(),
                kind: SettingsItemKind::Toggle {
                    enabled: self.proposal_trusted,
                },
                a11y: Some(A11y {
                    label: Some("Can propose contacts toggle".into()),
                    hint: Some(
                        "Allow this contact to suggest other people you should connect with".into(),
                    ),
                    role: None,
                }),
                info_key: None,
            }],
        });
        // Recovery permissions group — gate the recovery-trustee toggle
        // (Pair 3 ContactDetail engine extension, 2026-04-28).
        components.push(Component::SettingsGroup {
            id: "recovery_permissions".into(),
            label: "Recovery".into(),
            items: vec![SettingsItem {
                id: "recovery_trusted".into(),
                label: "Trust for recovery".into(),
                kind: SettingsItemKind::Toggle {
                    enabled: self.is_recovery_trusted,
                },
                a11y: Some(A11y {
                    label: Some("Trust for recovery toggle".into()),
                    hint: Some(
                        "Allow this contact to help you recover access if you lose your device"
                            .into(),
                    ),
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
        // Build contact_info items — always show initials, add trust level if set
        let mut contact_info_items = vec![InfoItem {
            icon: None,
            title: "Initials".into(),
            detail: self.contact.avatar_initials.clone(),
        }];
        if !self.trust_level.is_empty() {
            contact_info_items.push(InfoItem {
                icon: None,
                title: "Trust".into(),
                detail: self.trust_level.clone(),
            });
        }
        if show_verified_badge(self.is_verified) {
            contact_info_items.push(InfoItem {
                icon: Some("checkmark.seal".into()),
                title: "Verified".into(),
                detail: "Yes".into(),
            });
        }
        if show_recovery_trusted_indicator(self.is_recovery_trusted) {
            contact_info_items.push(InfoItem {
                icon: Some("shield".into()),
                title: "Recovery Trusted".into(),
                detail: "Yes".into(),
            });
        }
        if !self.fingerprint.is_empty() {
            contact_info_items.push(InfoItem {
                icon: None,
                title: "Fingerprint".into(),
                detail: self.fingerprint.clone(),
            });
        }
        if !self.reciprocity_status.is_empty() {
            contact_info_items.push(InfoItem {
                icon: None,
                title: "Exchange status".into(),
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
        for field in &self.fields {
            components.push(Component::FieldList {
                id: format!("field_{}", field.id),
                fields: vec![field.clone()],
                visibility_mode: VisibilityMode::ReadOnly,
                available_groups: vec![],
                a11y: Some(A11y {
                    label: Some("Contact fields".into()),
                    hint: None,
                    role: None,
                }),
            });
            let note_value = self.field_notes.get(&field.id).cloned().unwrap_or_default();
            components.push(Component::EditableText {
                id: format!("field_note:{}", field.id),
                label: "Private note for this field".into(),
                value: note_value,
                editing: false,
                validation_error: None,
                a11y: Some(A11y {
                    label: Some("Private field note, editable".into()),
                    hint: Some("Double tap to edit".into()),
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
                title: "Status".into(),
                detail: "All delivered".into(),
            });
        } else {
            items.push(InfoItem {
                icon: None,
                title: "Delivered".into(),
                detail: summary.delivered.to_string(),
            });
            if summary.pending > 0 {
                items.push(InfoItem {
                    icon: None,
                    title: "Pending".into(),
                    detail: summary.pending.to_string(),
                });
            }
            if summary.failed > 0 {
                items.push(InfoItem {
                    icon: None,
                    title: "Failed".into(),
                    detail: summary.failed.to_string(),
                });
            }
        }
        Some(Component::InfoPanel {
            id: "delivery_status".into(),
            icon: None,
            title: "Update Delivery".into(),
            items,
            a11y: Some(A11y {
                label: Some("Update Delivery".into()),
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
                title: "They see me as".into(),
                items: vec![InfoItem {
                    icon: None,
                    title: "Display Name".into(),
                    detail: shared.shared_display_name.clone(),
                }],
                a11y: Some(A11y {
                    label: Some("They see me as".into()),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            });
            // My fields — show which groups grant visibility
            components.push(Component::FieldList {
                id: "my_fields".into(),
                fields: shared.my_fields.clone(),
                visibility_mode: VisibilityMode::PerGroup,
                available_groups: shared.visible_groups.clone(),
                a11y: Some(A11y {
                    label: Some("Contact fields".into()),
                    hint: Some("Manage group visibility".into()),
                    role: None,
                }),
            });
        }
        components
    }

    fn delete_confirm(&self) -> Component {
        Component::InlineConfirm {
            id: "delete_contact".into(),
            warning: format!(
                "Permanently delete \"{}\"? This cannot be undone.",
                self.contact.name
            ),
            confirm_text: "Delete".into(),
            cancel_text: "Cancel".into(),
            destructive: true,
            a11y: Some(A11y {
                label: Some("Confirm contact deletion".into()),
                hint: Some("This will permanently delete the contact and cannot be undone".into()),
                role: Some(AccessibilityRole::Alert),
            }),
        }
    }

    fn build_actions(&self) -> Vec<ScreenAction> {
        let mut actions: Vec<ScreenAction> = Vec::new();
        actions.push(ScreenAction {
            id: "edit".into(),
            label: "Edit".into(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: None,
        });
        if verify_button_visible(self.is_verified, self.trust_level_enum) {
            actions.push(ScreenAction {
                id: "verify_fingerprint".into(),
                label: "Verify Fingerprint".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            });
        }
        actions.push(ScreenAction {
            id: "toggle_hidden".into(),
            label: if self.is_hidden {
                "Unhide contact".into()
            } else {
                "Hide contact".into()
            },
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });
        actions.push(ScreenAction {
            id: footer_action_id(self.is_imported).into(),
            label: if self.is_imported {
                "Delete Contact".into()
            } else {
                "Archive Contact".into()
            },
            style: if self.is_imported {
                ActionStyle::Destructive
            } else {
                ActionStyle::Secondary
            },
            enabled: true,
            a11y: None,
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
            // View mode toggle
            UserAction::ItemToggled {
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
                    message: "Contact archived".into(),
                    undo_action_id: Some(format!("undo_archive_contact:{}", self.contact.id)),
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

/// Fallback engine for when a contact is not found.
#[derive(Clone, Debug)]
pub struct ContactNotFoundEngine {
    contact_id: String,
}

impl ContactNotFoundEngine {
    pub fn new(contact_id: String) -> Self {
        Self { contact_id }
    }
}

impl WorkflowEngine for ContactNotFoundEngine {
    fn current_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "contact_not_found".into(),
            title: "Contact Not Found".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "not_found".into(),
                icon: None,
                title: "Not Found".into(),
                items: vec![InfoItem {
                    icon: None,
                    title: "Error".into(),
                    detail: format!("Contact '{}' was not found.", self.contact_id),
                }],
                a11y: Some(A11y {
                    label: Some("Not Found".into()),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            actions: vec![ScreenAction {
                id: "back".into(),
                label: "Back".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: tests access private ContactViewMode and
// ContactDetailEngine internals. Extracted to contact_detail_tests.rs
// to keep this file under the 1000-line src hard limit (M3 S5-12).
#[cfg(test)]
#[path = "contact_detail_tests.rs"]
mod tests;
