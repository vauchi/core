// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed engine↔`AppEngine` channels.
//!
//! `EngineOutput` (engine → hub) carries the salient state a workflow
//! engine exposes at completion or interception time. It replaces the
//! `as_any` downcast reads and the stringly-typed `collected_input`
//! channel: every payload that crosses the seam is a closed-enum
//! variant, so a wrong discriminator is a compile error, not a silent
//! runtime no-op.
//!
//! Mismatch policy: when the active engine is not the one a hub site
//! expects (overlay or lock engine active while a stale async result
//! lands), `engine_output()` yields `None` or a foreign variant — hub
//! sites `tracing::warn!` and degrade exactly as a failed downcast did.
//!
//! Record: `2026-06-10-appengine-typed-engine-channel`.

use super::emergency_broadcast::EmergencyOutcome;
use super::exchange::success::ExchangeSuccessSummary;
use super::fingerprint_verify::VerifyAction;
use super::onboarding::OnboardingData;
use vauchi_core::exchange::{
    AccelerometerProximityState, AudioProximityState, ProtocolState, QrPayload,
};

/// Salient typed state an engine exposes to `AppEngine`.
///
/// One variant per engine that the hub reads; each variant is the
/// engine's full interception-relevant snapshot so a single
/// parameterless getter suffices.
#[derive(Clone, PartialEq)]
#[non_exhaustive]
pub enum EngineOutput {
    /// Outcome of the fingerprint-verification screen.
    FingerprintVerify(VerifyAction),
    /// Everything captured during onboarding (display name, group
    /// selection, ContactInfo fields), plus the wizard step and any
    /// pending backup-restore file, read at completion time.
    Onboarding(Box<OnboardingSnapshot>),
    /// The configured emergency broadcast and the user's chosen outcome.
    EmergencyBroadcast(EmergencyBroadcastPlan),
    /// The display name as edited on the contact-edit screen.
    ContactEdit { display_name: String },
    /// The backup/restore form state (password redacted in `Debug`).
    Backup(BackupFormSnapshot),
    /// PIN/password typed on the lock screen (redacted in `Debug`).
    /// Absent (engine returns `None`) while the entry is empty.
    Lock { pin: String },
    /// Per-field show/hide toggles on the contact-visibility screen,
    /// as `(field_id, visible)` pairs.
    ContactVisibility { toggles: Vec<(String, bool)> },
    /// The sync screen's pressed action.
    Sync(SyncChoice),
    /// The privacy/GDPR operation the user confirmed.
    Gdpr(GdprChoice),
    /// A form-dialog submission, typed per dialog kind — replaces the
    /// retired newline-packed `collected_input` strings.
    Form(FormInput),
    /// Change-password form (both credentials redacted in `Debug`).
    ChangePassword { current: String, new: String },
    /// Duress-PIN setup (PIN redacted in `Debug`).
    DuressPin(DuressPinSetup),
    /// Device-management screen: index confirmed for revocation, if any.
    DeviceManagement { confirmed_revoke_index: Option<u32> },
    /// Avatar-editor result (`Debug` prints the byte length only).
    AvatarEditor {
        removed: bool,
        avatar: Option<Vec<u8>>,
    },
    /// Outcome of the device-replacement decommission screen.
    DeviceReplacement(super::device_replacement::CompletionOutcome),
    /// Whether the user granted the deep-link consent gate.
    DeepLinkConsent { granted: bool },
    /// Whether the active multi-stage exchange engine runs Hover mode.
    MultiStageExchange { hover_mode: bool },
    /// The old-key input on the Recovery screen.
    Recovery { old_key_input: String },
    /// The pasted claim input on the RecoveryHelp screen.
    RecoveryHelp { claim_input: String },
    /// Decoy-contacts form state.
    DecoyContacts {
        new_name: String,
        pending_delete_id: Option<String>,
    },
    /// Tag armed for deletion on the Tags management screen.
    Tags { pending_delete_id: Option<String> },
    /// Place armed for deletion on the Places management screen.
    Places { pending_delete_id: Option<String> },
    /// Reviewed field selection on the tag-promotion screen.
    TagPromotion { selected_field_ids: Vec<String> },
    /// Selected pair on the duplicate-detection screen.
    DuplicateDetection { selected_pair_index: Option<usize> },
    /// Entry-detail snapshot: field text + per-group visibility rows
    /// (`(group_id, group_name, visible)`).
    MyInfoEntryDetail {
        label: String,
        value: String,
        note: Option<String>,
        groups: Vec<(String, String, bool)>,
    },
    /// Contact-detail hidden flag (hide/unhide toggle interception).
    ContactDetail { is_hidden: bool },
    /// Contact-list faceted-search state: live query + facet opt-ins
    /// (`(tags, comment, place)`).
    ContactList {
        query: String,
        any_facet: bool,
        facets: (bool, bool, bool),
    },
}

/// Typed hub→engine state delivery — replaces the `as_any_mut`
/// downcast pokes. Returns are routed through
/// [`WorkflowEngine::apply_update`], which reports `false` when the
/// active engine is not the addressee (the caller warns + degrades,
/// matching the failed-downcast semantics).
///
/// `Debug` prints the variant *name only*: payloads carry QR data,
/// backup bytes, and claim material that must never reach logs.
///
/// [`WorkflowEngine::apply_update`]: super::WorkflowEngine::apply_update
pub enum EngineUpdate {
    MultiStage(MultiStageUpdate),
    DeviceLink(DeviceLinkUpdate),
    LinkResponder(LinkResponderUpdate),
    LinkExchange(LinkExchangeUpdate),
    /// Flip the BLE exchange chrome to its terminal Success screen.
    BleForceSuccess,
    ContactDetail(ContactDetailUpdate),
    ContactList(ContactListUpdate),
    Recovery(RecoveryUpdate),
    RecoveryHelp(RecoveryHelpUpdate),
    Onboarding(OnboardingUpdate),
    /// Tags/Places management screens: commit the armed row delete.
    ConfirmPendingDelete,
    MyInfoEntryDetail(MyInfoEntryDetailUpdate),
}

/// Cycle-thread bridge updates for the multi-stage exchange engine.
pub enum MultiStageUpdate {
    State(ProtocolState),
    QrPayload(QrPayload),
    Finalized(String),
    SuccessSummary(ExchangeSuccessSummary),
    SessionEnded,
    AudioProximity(AudioProximityState),
    AccelProximity(AccelerometerProximityState),
}

/// Cycle-thread bridge updates for the device-linking engine.
pub enum DeviceLinkUpdate {
    QrPending,
    QrReady {
        qr_data: String,
        expires_at: u64,
    },
    QrExpired,
    RequestReceived {
        device_name: String,
        confirmation_code: String,
        challenge_hex: String,
    },
    Completed,
    Failed(String),
}

/// Terminal updates for the deep-link responder engine.
pub enum LinkResponderUpdate {
    Completed(ExchangeSuccessSummary),
    Failed(String),
}

/// State updates for the link-exchange initiator engine.
pub enum LinkExchangeUpdate {
    ShareUrl(String),
    Waiting,
    Retrieving,
    Succeeded(ExchangeSuccessSummary),
    Failed(String),
}

/// Optimistic display updates for the contact-detail engine after the
/// hub persisted the corresponding change.
pub enum ContactDetailUpdate {
    ToggleProposalTrusted,
    ToggleRecoveryTrusted,
    ToggleHidden,
    TagQuery {
        query: String,
        suggestions: Vec<String>,
    },
    TagAdded(super::contact_detail_rules::ContactTag),
    TagRemoved(String),
    PlaceQuery {
        query: String,
        suggestions: Vec<String>,
    },
    PlaceNamed(String),
    ClearExchangePlace,
}

/// Faceted-search updates for the contact-list engine.
pub enum ContactListUpdate {
    ToggleFacet(String),
    SearchQuery(String),
    FacetedIds(Option<Vec<String>>),
}

/// Results pushed back into the recovery engine.
pub enum RecoveryUpdate {
    ClaimGenerated(String),
    ClaimCreateError(String),
}

/// Results pushed back into the recovery-help engine.
pub enum RecoveryHelpUpdate {
    ParsedClaim(super::recovery_help::ParsedClaimSummary),
    ClaimParseError(String),
    VoucherData(String),
}

/// Updates for the onboarding engine.
pub enum OnboardingUpdate {
    /// Stash picked backup bytes and transition to password entry.
    PendingBackupBytes(Vec<u8>),
    /// Consume the pending backup after the hub read it off the
    /// snapshot — re-submitting without re-picking stays impossible.
    ClearPendingBackup,
    /// Rewind to LinkChoice after a failed restore.
    ResetToLinkChoice,
    /// Buffer a field added via the form dialog mid-onboarding.
    PushField(super::onboarding::FieldSetup),
}

/// Updates for the my-info entry-detail engine.
pub enum MyInfoEntryDetailUpdate {
    GroupVisibility {
        group_id: String,
        visible: bool,
        visible_contacts: Vec<super::my_info_entry_detail::EntryContactInfo>,
    },
}

impl EngineUpdate {
    /// Variant path for logging — payloads never reach logs.
    pub fn name(&self) -> &'static str {
        match self {
            Self::MultiStage(u) => match u {
                MultiStageUpdate::State(_) => "MultiStage::State",
                MultiStageUpdate::QrPayload(_) => "MultiStage::QrPayload",
                MultiStageUpdate::Finalized(_) => "MultiStage::Finalized",
                MultiStageUpdate::SuccessSummary(_) => "MultiStage::SuccessSummary",
                MultiStageUpdate::SessionEnded => "MultiStage::SessionEnded",
                MultiStageUpdate::AudioProximity(_) => "MultiStage::AudioProximity",
                MultiStageUpdate::AccelProximity(_) => "MultiStage::AccelProximity",
            },
            Self::DeviceLink(u) => match u {
                DeviceLinkUpdate::QrPending => "DeviceLink::QrPending",
                DeviceLinkUpdate::QrReady { .. } => "DeviceLink::QrReady",
                DeviceLinkUpdate::QrExpired => "DeviceLink::QrExpired",
                DeviceLinkUpdate::RequestReceived { .. } => "DeviceLink::RequestReceived",
                DeviceLinkUpdate::Completed => "DeviceLink::Completed",
                DeviceLinkUpdate::Failed(_) => "DeviceLink::Failed",
            },
            Self::LinkResponder(u) => match u {
                LinkResponderUpdate::Completed(_) => "LinkResponder::Completed",
                LinkResponderUpdate::Failed(_) => "LinkResponder::Failed",
            },
            Self::LinkExchange(u) => match u {
                LinkExchangeUpdate::ShareUrl(_) => "LinkExchange::ShareUrl",
                LinkExchangeUpdate::Waiting => "LinkExchange::Waiting",
                LinkExchangeUpdate::Retrieving => "LinkExchange::Retrieving",
                LinkExchangeUpdate::Succeeded(_) => "LinkExchange::Succeeded",
                LinkExchangeUpdate::Failed(_) => "LinkExchange::Failed",
            },
            Self::BleForceSuccess => "BleForceSuccess",
            Self::ContactDetail(u) => match u {
                ContactDetailUpdate::ToggleProposalTrusted => {
                    "ContactDetail::ToggleProposalTrusted"
                }
                ContactDetailUpdate::ToggleRecoveryTrusted => {
                    "ContactDetail::ToggleRecoveryTrusted"
                }
                ContactDetailUpdate::ToggleHidden => "ContactDetail::ToggleHidden",
                ContactDetailUpdate::TagQuery { .. } => "ContactDetail::TagQuery",
                ContactDetailUpdate::TagAdded(_) => "ContactDetail::TagAdded",
                ContactDetailUpdate::TagRemoved(_) => "ContactDetail::TagRemoved",
                ContactDetailUpdate::PlaceQuery { .. } => "ContactDetail::PlaceQuery",
                ContactDetailUpdate::PlaceNamed(_) => "ContactDetail::PlaceNamed",
                ContactDetailUpdate::ClearExchangePlace => "ContactDetail::ClearExchangePlace",
            },
            Self::ContactList(u) => match u {
                ContactListUpdate::ToggleFacet(_) => "ContactList::ToggleFacet",
                ContactListUpdate::SearchQuery(_) => "ContactList::SearchQuery",
                ContactListUpdate::FacetedIds(_) => "ContactList::FacetedIds",
            },
            Self::Recovery(u) => match u {
                RecoveryUpdate::ClaimGenerated(_) => "Recovery::ClaimGenerated",
                RecoveryUpdate::ClaimCreateError(_) => "Recovery::ClaimCreateError",
            },
            Self::RecoveryHelp(u) => match u {
                RecoveryHelpUpdate::ParsedClaim(_) => "RecoveryHelp::ParsedClaim",
                RecoveryHelpUpdate::ClaimParseError(_) => "RecoveryHelp::ClaimParseError",
                RecoveryHelpUpdate::VoucherData(_) => "RecoveryHelp::VoucherData",
            },
            Self::Onboarding(u) => match u {
                OnboardingUpdate::PendingBackupBytes(_) => "Onboarding::PendingBackupBytes",
                OnboardingUpdate::ClearPendingBackup => "Onboarding::ClearPendingBackup",
                OnboardingUpdate::ResetToLinkChoice => "Onboarding::ResetToLinkChoice",
                OnboardingUpdate::PushField(_) => "Onboarding::PushField",
            },
            Self::ConfirmPendingDelete => "ConfirmPendingDelete",
            Self::MyInfoEntryDetail(u) => match u {
                MyInfoEntryDetailUpdate::GroupVisibility { .. } => {
                    "MyInfoEntryDetail::GroupVisibility"
                }
            },
        }
    }
}

impl std::fmt::Debug for EngineUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Snapshot of the onboarding engine at completion time.
#[derive(Clone, Debug, PartialEq)]
pub struct OnboardingSnapshot {
    pub data: OnboardingData,
    pub step: vauchi_core::types::OnboardingStep,
    pub pending_backup: Option<PendingBackup>,
}

/// Backup file + password staged on the onboarding engine during a
/// restore. The hub consumes it at completion and sends
/// [`OnboardingUpdate::ClearPendingBackup`] so re-submitting without
/// re-picking the file stays impossible.
#[derive(Clone, PartialEq)]
pub struct PendingBackup {
    pub bytes: Vec<u8>,
    pub password: String,
}

impl std::fmt::Debug for PendingBackup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingBackup")
            .field("bytes_len", &self.bytes.len())
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Snapshot of the duress-PIN setup form.
///
/// `pin` is a live credential — `Debug` is hand-written to redact it.
#[derive(Clone, PartialEq)]
pub struct DuressPinSetup {
    pub enabled: bool,
    pub pin: String,
    pub alert_contact_ids: Vec<String>,
    pub alert_message: String,
    pub include_location: bool,
}

impl std::fmt::Debug for DuressPinSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DuressPinSetup")
            .field("enabled", &self.enabled)
            .field("pin", &"<redacted>")
            .field("alert_contact_ids", &self.alert_contact_ids)
            .field("include_location", &self.include_location)
            .finish()
    }
}

/// Action pressed on the sync-status screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncChoice {
    SyncNow,
    TestConnection,
}

/// Operation confirmed on the privacy/GDPR screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdprChoice {
    Export,
    Delete,
    CancelDeletion,
    Execute,
    Shred,
}

/// Typed form-dialog submission, one variant per [`FormDialogType`].
///
/// [`FormDialogType`]: super::form_dialog::FormDialogType
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormInput {
    EditName {
        name: String,
    },
    EditField {
        value: String,
        note: String,
    },
    AddField {
        entry_type: String,
        label: String,
        value: String,
        note: String,
        groups: Vec<String>,
    },
    CreateGroup {
        name: String,
    },
    RenameGroup {
        name: String,
    },
    EditRelayUrl {
        url: String,
    },
}

/// Snapshot of the emergency-broadcast engine at completion time.
#[derive(Clone, Debug, PartialEq)]
pub struct EmergencyBroadcastPlan {
    pub outcome: Option<EmergencyOutcome>,
    pub contact_ids: Vec<String>,
    pub message: String,
    pub include_location: bool,
}

/// Snapshot of the backup/restore form.
///
/// `password` is a live credential: it must never reach logs, so
/// [`EngineOutput`]'s `Debug` is hand-written to redact it (hub
/// mismatch warns print the foreign variant with `?other`).
#[derive(Clone, PartialEq)]
pub struct BackupFormSnapshot {
    pub restore_mode: bool,
    pub restore_data: String,
    pub password: String,
    pub full_level: bool,
}

impl std::fmt::Debug for BackupFormSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupFormSnapshot")
            .field("restore_mode", &self.restore_mode)
            .field("restore_data_len", &self.restore_data.len())
            .field("password", &"<redacted>")
            .field("full_level", &self.full_level)
            .finish()
    }
}

impl std::fmt::Debug for EngineOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FingerprintVerify(a) => f.debug_tuple("FingerprintVerify").field(a).finish(),
            Self::Onboarding(d) => f.debug_tuple("Onboarding").field(d).finish(),
            Self::EmergencyBroadcast(p) => f.debug_tuple("EmergencyBroadcast").field(p).finish(),
            Self::ContactEdit { display_name } => f
                .debug_struct("ContactEdit")
                .field("display_name", display_name)
                .finish(),
            Self::Backup(s) => f.debug_tuple("Backup").field(s).finish(),
            Self::Lock { .. } => f.debug_struct("Lock").field("pin", &"<redacted>").finish(),
            Self::ContactVisibility { toggles } => f
                .debug_struct("ContactVisibility")
                .field("toggles", toggles)
                .finish(),
            Self::Sync(c) => f.debug_tuple("Sync").field(c).finish(),
            Self::Gdpr(c) => f.debug_tuple("Gdpr").field(c).finish(),
            Self::Form(i) => f.debug_tuple("Form").field(i).finish(),
            Self::ChangePassword { .. } => f
                .debug_struct("ChangePassword")
                .field("current", &"<redacted>")
                .field("new", &"<redacted>")
                .finish(),
            Self::DuressPin(s) => f.debug_tuple("DuressPin").field(s).finish(),
            Self::DeviceManagement {
                confirmed_revoke_index,
            } => f
                .debug_struct("DeviceManagement")
                .field("confirmed_revoke_index", confirmed_revoke_index)
                .finish(),
            Self::AvatarEditor { removed, avatar } => f
                .debug_struct("AvatarEditor")
                .field("removed", removed)
                .field("avatar_len", &avatar.as_ref().map(|a| a.len()))
                .finish(),
            Self::DeviceReplacement(o) => f.debug_tuple("DeviceReplacement").field(o).finish(),
            Self::DeepLinkConsent { granted } => f
                .debug_struct("DeepLinkConsent")
                .field("granted", granted)
                .finish(),
            Self::MultiStageExchange { hover_mode } => f
                .debug_struct("MultiStageExchange")
                .field("hover_mode", hover_mode)
                .finish(),
            Self::Recovery { old_key_input } => f
                .debug_struct("Recovery")
                .field("old_key_input_len", &old_key_input.len())
                .finish(),
            Self::RecoveryHelp { claim_input } => f
                .debug_struct("RecoveryHelp")
                .field("claim_input_len", &claim_input.len())
                .finish(),
            Self::DecoyContacts {
                new_name,
                pending_delete_id,
            } => f
                .debug_struct("DecoyContacts")
                .field("new_name", new_name)
                .field("pending_delete_id", pending_delete_id)
                .finish(),
            Self::Tags { pending_delete_id } => f
                .debug_struct("Tags")
                .field("pending_delete_id", pending_delete_id)
                .finish(),
            Self::Places { pending_delete_id } => f
                .debug_struct("Places")
                .field("pending_delete_id", pending_delete_id)
                .finish(),
            Self::TagPromotion { selected_field_ids } => f
                .debug_struct("TagPromotion")
                .field("selected_field_ids", selected_field_ids)
                .finish(),
            Self::DuplicateDetection {
                selected_pair_index,
            } => f
                .debug_struct("DuplicateDetection")
                .field("selected_pair_index", selected_pair_index)
                .finish(),
            Self::MyInfoEntryDetail { label, groups, .. } => f
                .debug_struct("MyInfoEntryDetail")
                .field("label", label)
                .field("groups", &groups.len())
                .finish(),
            Self::ContactDetail { is_hidden } => f
                .debug_struct("ContactDetail")
                .field("is_hidden", is_hidden)
                .finish(),
            Self::ContactList {
                query,
                any_facet,
                facets,
            } => f
                .debug_struct("ContactList")
                .field("query", query)
                .field("any_facet", any_facet)
                .field("facets", facets)
                .finish(),
        }
    }
}
