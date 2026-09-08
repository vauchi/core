// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Constructors and `with_*` builders for `ContactDetailEngine`, split
//! out of `contact_detail.rs` (file-size, VRS04). Child module via
//! `#[path]` so the builders keep private-field access.

use super::*;

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
            editing_component_id: None,
            editing_value: None,
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
            locale: Locale::English,
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
            editing_component_id: None,
            editing_value: None,
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
            locale: Locale::English,
        }
    }

    /// Attach delivery status summary for card updates to this contact.
    pub fn with_delivery_summary(mut self, summary: DeliverySummary) -> Self {
        self.delivery_summary = Some(summary);
        self
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-12).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
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

    /// Set the avatar image data for the ImageCircle component.
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
