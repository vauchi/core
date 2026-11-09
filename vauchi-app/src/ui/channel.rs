// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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

use super::exchange::success::ExchangeSuccessSummary;
use super::fingerprint_verify::VerifyAction;
use super::onboarding::OnboardingData;
use vauchi_core::exchange::{
    AccelerometerProximityState, AudioProximityState, LinkFeedback, ProtocolState, QrPayload,
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
    /// The Edit Contact values that changed; `None` means leave as stored
    /// (Discard sets both to `None`). Redacted in `Debug`: personal data.
    ContactEdit {
        display_name: Option<String>,
        personal_note: Option<String>,
    },
    /// The backup/restore form state (password redacted in `Debug`).
    Backup(BackupFormSnapshot),
    /// PIN/password typed on the lock screen (redacted in `Debug`).
    /// Absent (engine returns `None`) while the entry is empty.
    Lock { pin: String },
    /// PIN typed to confirm a device link (redacted in `Debug`). Absent
    /// while the entry is empty or the screen is past the PIN step.
    DeviceLinkPin { pin: String },
    /// PIN typed to confirm a data export (redacted in `Debug`). Absent
    /// while the entry is empty or the screen is past the PIN step.
    ExportPin { pin: String },
    /// PIN typed at the step in front of a credential change (redacted in
    /// `Debug`) (#777). Absent while the entry is empty.
    SessionPin { pin: String },
    /// The export waits on the OS user-presence check (#712).
    AwaitingUserPresence,
    /// The export password the encrypted data export is sealed with
    /// (redacted in `Debug`) (#712).
    GdprExportEncrypted { password: String },
    /// A verified Glance peer code the user typed instead of scanning;
    /// the AppEngine pins the peer from it as from a camera scan. Absent
    /// until a code is accepted. `Debug` prints the length only: a code
    /// carries key material (logging rules).
    GlancePeerCode { data: String },
    /// Per-field show/hide toggles on the contact-visibility screen,
    /// as `(field_id, visible)` pairs.
    ContactVisibility { toggles: Vec<(String, bool)> },
    /// The privacy/GDPR operation the user confirmed.
    Gdpr(GdprChoice),
    /// A form-dialog submission, typed per dialog kind — replaces the
    /// retired newline-packed `collected_input` strings.
    Form(FormInput),
    /// Change-password form (both credentials redacted in `Debug`).
    ChangePassword { current: String, new: String },
    /// Duress-PIN setup (PIN redacted in `Debug`).
    DuressPin(DuressPinSetup),
    /// Device-management screen: index confirmed for revocation, if any,
    /// and a request to remove the identity from this device
    /// (`Some(wipe_if_not_told)`, #915).
    DeviceManagement {
        confirmed_revoke_index: Option<u32>,
        remove_this_device: Option<bool>,
    },
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
    /// The Recovery screen's inputs: the old key, and the pasted voucher
    /// while vouchers are being collected (`None` at every other step).
    Recovery {
        old_key_input: String,
        voucher_input: Option<String>,
        /// The passphrase typed on the restore step, `Some` only there.
        restore_passphrase: Option<String>,
        restore_same_identity: bool,
        /// The owner's new passphrase and its repetition, `Some` only on
        /// the passphrase step.
        new_passphrase: Option<(String, String)>,
    },
    /// The pasted claim input on the RecoveryHelp screen.
    RecoveryHelp { claim_input: String },
    /// Decoy-contacts form state.
    DecoyContacts {
        new_name: String,
        pending_delete_id: Option<String>,
    },
    /// The emergency broadcast form: who receives the alert, the message
    /// and the location choice. `Debug` prints counts only: the recipient
    /// list and the message are personal data.
    EmergencyBroadcast {
        trusted_contact_ids: Vec<String>,
        message: String,
        include_location: bool,
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
    ContactDetail {
        is_hidden: bool,
        /// The wipe choice in the block question (ADR-056 point 4).
        wipe_their_copy: bool,
    },
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
    BackupRecovery(BackupRecoveryUpdate),
    DeviceLink(DeviceLinkUpdate),
    DeviceLinkJoin(DeviceLinkJoinUpdate),
    LinkResponder(LinkResponderUpdate),
    LinkExchange(LinkExchangeUpdate),
    /// Flip the BLE exchange chrome to its terminal Success screen. Carries
    /// the rich exchange summary when the AppEngine persisted the contact
    /// (M2 S6) — `None` degrades to the minimal completion chrome.
    BleForceSuccess {
        summary: Option<Box<crate::ui::exchange::success::ExchangeSuccessSummary>>,
    },
    /// Flip the BLE exchange chrome to its terminal Failed screen.
    BleForceFailure {
        reason: Option<String>,
    },
    /// The person granted a transport permission the shell asked the OS
    /// for; a BLE search still looking for a peer restarts its discovery
    /// budget, so the time the prompt was open does not count
    /// (vauchi/private#1022).
    TransportPermissionGranted,
    ContactDetail(ContactDetailUpdate),
    ContactList(ContactListUpdate),
    Recovery(RecoveryUpdate),
    RecoveryHelp(RecoveryHelpUpdate),
    Onboarding(OnboardingUpdate),
    /// Tags/Places management screens: commit the armed row delete.
    ConfirmPendingDelete,
    MyInfoEntryDetail(MyInfoEntryDetailUpdate),
    /// The shell reported (or re-reported) its hardware. Pushed into the
    /// live engine so a screen gated on a capability re-prepares without a
    /// rebuild that would drop its state (the lock screen's attempt count).
    DeviceCapabilities(vauchi_core::exchange::capability::types::DeviceCapabilities),
    /// The biometric prompt succeeded but a duress PIN is set, so the lock
    /// screen asks for the password only (ADR-032, #591).
    BiometricUnlockPassed,
    /// The PIN typed at a session PIN step was not the session's (#777).
    SessionPinRejected,
    GdprExport(GdprExportUpdate),
    /// Removing the identity from this device could not tell the other
    /// devices; nothing is wiped yet (#915).
    DeviceRemovalNotTold,
    /// An emergency broadcast went out: how many of the trusted contacts
    /// got an alert queued (ADR-036 `BroadcastResult`).
    EmergencyBroadcastSent {
        sent: usize,
        total: usize,
    },
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
    /// Peer-frame stall presentation, derived from
    /// `MultiStageMachine::is_frame_stalled` on every poll tick / scanned
    /// frame — see `_private/docs/backlog/
    /// 2026-09-10-exchange-stall-and-ble-fallback-states/README.md`.
    Stalled(bool),
    /// Whether this phone reads the other one, or the other one needs an
    /// update; from `MultiStageMachine::link_feedback` on the same ticks
    /// as `Stalled` (vauchi/private#450).
    LinkFeedback(LinkFeedback),
    /// The other phone said it saved too; from
    /// `MultiStageMachine::peer_confirmed` on the same ticks (plan 5.3).
    PeerConfirmed(bool),
}

/// The AppEngine's verdict on the PIN typed to confirm a data export.
pub enum GdprExportUpdate {
    /// The PIN that opened the session: run the pending export once.
    Verified,
    PinRejected,
    /// The export ran (or failed): forget its passwords and return to the
    /// overview, so nothing of it outlives the one export.
    Finished,
    /// The shell cannot run the OS user-presence check after all.
    UserPresenceUnavailable,
}

/// Cycle-thread bridge updates for the device-linking engine.
pub enum DeviceLinkUpdate {
    QrPending,
    QrReady {
        /// Data to encode in the QR code. For device-link join this is the
        /// `DeviceLinkJoinInvitation` URL; for legacy flows it may still be
        /// the bare QR data until all callers migrate.
        invitation_url: String,
        expires_at: u64,
    },
    QrExpired,
    RequestReceived {
        device_name: String,
        confirmation_code: String,
    },
    Completed,
    Failed(String),
    /// Ask for the PIN before any link code exists (#469).
    PinRequired,
    /// The PIN did not match the one that opened this session.
    PinRejected,
}

/// Cycle-thread bridge updates for the device-link join (responder) engine.
pub enum DeviceLinkJoinUpdate {
    /// The user confirmed/edited the device name; move from name entry to
    /// posting the request on the relay.
    NameAccepted,
    /// The join request has been posted to the relay. The confirmation code
    /// must match the one shown on the initiator device.
    RequestPosted { confirmation_code: String },
    /// The initiator posted the encrypted response; core will adopt it next.
    ResponseReady,
    /// The response was adopted and the identity persisted.
    Completed,
    /// Terminal failure (stable reason id).
    Failed(String),
}

/// Terminal updates for the deep-link responder engine.
pub enum LinkResponderUpdate {
    /// The typed code was well formed; the exchange has started.
    CodeAccepted,
    /// The typed code is not six digits; nothing has started.
    CodeInvalid,
    Completed(ExchangeSuccessSummary),
    Failed(String),
}

/// State updates for the link-exchange initiator engine.
pub enum LinkExchangeUpdate {
    ShareUrl(String),
    /// The code the sender passes on another way than the link.
    Code(vauchi_core::exchange::link_pake::LinkCode),
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
    /// Recovery trust is being turned on and a guardian group exists:
    /// ask which group the contact joins (vauchi/private#796, D17).
    AskGuardianGroup(Vec<super::GuardianGroupRow>),
    /// The guardian group question is answered or dismissed.
    CloseGuardianGroup,
    ToggleHidden,
    ToggleIgnored,
    /// Storage changed the block flag (ADR-056); the question closes.
    SetBlocked(bool),
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
    ClaimRenewalFailed,
    BackupTransfer(HandBackTransfer),
    HandBackRestoreReady {
        handed_back: usize,
        needs_passphrase: bool,
    },
    /// The stored proof after a voucher was taken in: each signer's
    /// short key fingerprint, and the threshold the proof needs.
    VouchersCollected {
        signers: Vec<String>,
        threshold: usize,
    },
    VoucherRefused(String),
    ProofSubmitted,
    /// Guardians handed a backup back (vauchi/private#796, step 3b).
    RestoreOffered {
        handed_back: usize,
        needs_passphrase: bool,
    },
    /// The restore brought this many contacts back.
    Restored(usize),
    /// The restore did not open; the localized reason.
    RestoreRefused(String),
    /// Whether the owner's recovery passphrase is set, after loading,
    /// saving or removing it (vauchi/private#796, D16).
    PassphraseSet(bool),
    /// The passphrase was not saved; the localized reason.
    PassphraseRefused(String),
    /// The contacts restored and not yet moved to this phone, by name,
    /// each with whether it did not answer in 14 days (vauchi/private#796).
    RekeyWaiting(Vec<(String, bool)>),
}

/// Results pushed back into the recovery-help engine.
pub enum RecoveryHelpUpdate {
    ClaimScanned(String),
    HandBackReady(String),
    HandBackTransfer(HandBackTransfer),
    ParsedClaim(super::recovery_help::ParsedClaimSummary),
    ClaimParseError(String),
    VoucherData(String),
}

/// How a hand-back's Bluetooth transfer stands, on either phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HandBackTransfer {
    #[default]
    Idle,
    InProgress,
    Finished,
    Failed,
}

/// Updates for the onboarding engine.
/// Updates for the backup/restore wizard.
#[derive(Debug, Clone)]
pub enum BackupRecoveryUpdate {
    /// Deliver a backup chosen through the file picker.
    ///
    /// Restore cannot take a real backup through the paste field: a
    /// 10k-contact export is ~2.2 MB against a 4 KB
    /// `MAX_EVENT_INPUT_VALUE_BYTES`, so the value is dropped and the
    /// screen dead-ends with Continue disabled (problem record
    /// 2026-09-08-full-backup-restore-cannot-be-pasted). File bytes are
    /// not carried by that bounded channel.
    PickedBackupBytes(Vec<u8>),
}

pub enum OnboardingUpdate {
    /// Stash picked backup bytes and transition to password entry.
    PendingBackupBytes(Vec<u8>),
    /// Consume the pending backup after the hub read it off the
    /// snapshot — re-submitting without re-picking stays impossible.
    ClearPendingBackup,
    /// Rewind to IdentityCheck after a failed restore.
    ResetToIdentityCheck,
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
    /// The contacts the entry is sent to changed without a group toggle
    /// (the all-contacts switch).
    Viewers {
        visible_contacts: Vec<super::my_info_entry_detail::EntryContactInfo>,
    },
}

impl EngineUpdate {
    /// Variant path for logging — payloads never reach logs.
    pub fn name(&self) -> &'static str {
        match self {
            Self::BackupRecovery(u) => match u {
                BackupRecoveryUpdate::PickedBackupBytes(_) => "BackupRecovery::PickedBackupBytes",
            },
            Self::MultiStage(u) => match u {
                MultiStageUpdate::State(_) => "MultiStage::State",
                MultiStageUpdate::QrPayload(_) => "MultiStage::QrPayload",
                MultiStageUpdate::Finalized(_) => "MultiStage::Finalized",
                MultiStageUpdate::SuccessSummary(_) => "MultiStage::SuccessSummary",
                MultiStageUpdate::SessionEnded => "MultiStage::SessionEnded",
                MultiStageUpdate::AudioProximity(_) => "MultiStage::AudioProximity",
                MultiStageUpdate::AccelProximity(_) => "MultiStage::AccelProximity",
                MultiStageUpdate::Stalled(_) => "MultiStage::Stalled",
                MultiStageUpdate::LinkFeedback(_) => "MultiStage::LinkFeedback",
                MultiStageUpdate::PeerConfirmed(_) => "MultiStage::PeerConfirmed",
            },
            Self::DeviceLink(u) => match u {
                DeviceLinkUpdate::QrPending => "DeviceLink::QrPending",
                DeviceLinkUpdate::QrReady { .. } => "DeviceLink::QrReady",
                DeviceLinkUpdate::QrExpired => "DeviceLink::QrExpired",
                DeviceLinkUpdate::RequestReceived { .. } => "DeviceLink::RequestReceived",
                DeviceLinkUpdate::Completed => "DeviceLink::Completed",
                DeviceLinkUpdate::Failed(_) => "DeviceLink::Failed",
                DeviceLinkUpdate::PinRequired => "DeviceLink::PinRequired",
                DeviceLinkUpdate::PinRejected => "DeviceLink::PinRejected",
            },
            Self::DeviceLinkJoin(u) => match u {
                DeviceLinkJoinUpdate::NameAccepted => "DeviceLinkJoin::NameAccepted",
                DeviceLinkJoinUpdate::RequestPosted { .. } => "DeviceLinkJoin::RequestPosted",
                DeviceLinkJoinUpdate::ResponseReady => "DeviceLinkJoin::ResponseReady",
                DeviceLinkJoinUpdate::Completed => "DeviceLinkJoin::Completed",
                DeviceLinkJoinUpdate::Failed(_) => "DeviceLinkJoin::Failed",
            },
            Self::LinkResponder(u) => match u {
                LinkResponderUpdate::CodeAccepted => "LinkResponder::CodeAccepted",
                LinkResponderUpdate::CodeInvalid => "LinkResponder::CodeInvalid",
                LinkResponderUpdate::Completed(_) => "LinkResponder::Completed",
                LinkResponderUpdate::Failed(_) => "LinkResponder::Failed",
            },
            Self::LinkExchange(u) => match u {
                LinkExchangeUpdate::ShareUrl(_) => "LinkExchange::ShareUrl",
                LinkExchangeUpdate::Code(_) => "LinkExchange::Code",
                LinkExchangeUpdate::Waiting => "LinkExchange::Waiting",
                LinkExchangeUpdate::Retrieving => "LinkExchange::Retrieving",
                LinkExchangeUpdate::Succeeded(_) => "LinkExchange::Succeeded",
                LinkExchangeUpdate::Failed(_) => "LinkExchange::Failed",
            },
            Self::BleForceSuccess { .. } => "BleForceSuccess",
            Self::BleForceFailure { .. } => "BleForceFailure",
            Self::TransportPermissionGranted => "TransportPermissionGranted",
            Self::ContactDetail(u) => match u {
                ContactDetailUpdate::ToggleProposalTrusted => {
                    "ContactDetail::ToggleProposalTrusted"
                }
                ContactDetailUpdate::ToggleRecoveryTrusted => {
                    "ContactDetail::ToggleRecoveryTrusted"
                }
                ContactDetailUpdate::AskGuardianGroup(_) => "ContactDetail::AskGuardianGroup",
                ContactDetailUpdate::CloseGuardianGroup => "ContactDetail::CloseGuardianGroup",
                ContactDetailUpdate::ToggleHidden => "ContactDetail::ToggleHidden",
                ContactDetailUpdate::ToggleIgnored => "ContactDetail::ToggleIgnored",
                ContactDetailUpdate::SetBlocked(_) => "ContactDetail::SetBlocked",
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
                RecoveryUpdate::ClaimRenewalFailed => "Recovery::ClaimRenewalFailed",
                RecoveryUpdate::BackupTransfer(_) => "Recovery::BackupTransfer",
                RecoveryUpdate::HandBackRestoreReady { .. } => "Recovery::HandBackRestoreReady",
                RecoveryUpdate::VouchersCollected { .. } => "Recovery::VouchersCollected",
                RecoveryUpdate::VoucherRefused(_) => "Recovery::VoucherRefused",
                RecoveryUpdate::ProofSubmitted => "Recovery::ProofSubmitted",
                RecoveryUpdate::RestoreOffered { .. } => "Recovery::RestoreOffered",
                RecoveryUpdate::Restored(_) => "Recovery::Restored",
                RecoveryUpdate::RestoreRefused(_) => "Recovery::RestoreRefused",
                RecoveryUpdate::PassphraseSet(_) => "Recovery::PassphraseSet",
                RecoveryUpdate::PassphraseRefused(_) => "Recovery::PassphraseRefused",
                RecoveryUpdate::RekeyWaiting(_) => "Recovery::RekeyWaiting",
            },
            Self::RecoveryHelp(u) => match u {
                RecoveryHelpUpdate::ClaimScanned(_) => "RecoveryHelp::ClaimScanned",
                RecoveryHelpUpdate::HandBackReady(_) => "RecoveryHelp::HandBackReady",
                RecoveryHelpUpdate::HandBackTransfer(_) => "RecoveryHelp::HandBackTransfer",
                RecoveryHelpUpdate::ParsedClaim(_) => "RecoveryHelp::ParsedClaim",
                RecoveryHelpUpdate::ClaimParseError(_) => "RecoveryHelp::ClaimParseError",
                RecoveryHelpUpdate::VoucherData(_) => "RecoveryHelp::VoucherData",
            },
            Self::Onboarding(u) => match u {
                OnboardingUpdate::PendingBackupBytes(_) => "Onboarding::PendingBackupBytes",
                OnboardingUpdate::ClearPendingBackup => "Onboarding::ClearPendingBackup",
                OnboardingUpdate::ResetToIdentityCheck => "Onboarding::ResetToIdentityCheck",
                OnboardingUpdate::PushField(_) => "Onboarding::PushField",
            },
            Self::ConfirmPendingDelete => "ConfirmPendingDelete",
            Self::DeviceCapabilities(_) => "DeviceCapabilities",
            Self::BiometricUnlockPassed => "BiometricUnlockPassed",
            Self::SessionPinRejected => "SessionPinRejected",
            Self::DeviceRemovalNotTold => "DeviceRemovalNotTold",
            Self::EmergencyBroadcastSent { .. } => "EmergencyBroadcastSent",
            Self::GdprExport(u) => match u {
                GdprExportUpdate::Verified => "GdprExport::Verified",
                GdprExportUpdate::PinRejected => "GdprExport::PinRejected",
                GdprExportUpdate::Finished => "GdprExport::Finished",
                GdprExportUpdate::UserPresenceUnavailable => "GdprExport::UserPresenceUnavailable",
            },
            Self::MyInfoEntryDetail(u) => match u {
                MyInfoEntryDetailUpdate::GroupVisibility { .. } => {
                    "MyInfoEntryDetail::GroupVisibility"
                }
                MyInfoEntryDetailUpdate::Viewers { .. } => "MyInfoEntryDetail::Viewers",
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
}

impl std::fmt::Debug for DuressPinSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DuressPinSetup")
            .field("enabled", &self.enabled)
            .field("pin", &"<redacted>")
            .finish()
    }
}

/// Operation confirmed on the privacy/GDPR screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdprChoice {
    Export,
    /// Schedule Leave Vauchi with what it carries out when it runs: the
    /// purge also erases the relay's list of trusted contacts (#711); the
    /// identity's other devices wipe (#916).
    Delete {
        erase_guardian_set: bool,
        wipe_other_devices: bool,
    },
    CancelDeletion,
    Execute,
    Shred,
    /// The paper heirloom: a printable, plaintext contact book (private#363).
    Heirloom,
    /// Every contact as vCard 4.0, behind the unprotected-file warning (#712).
    ExportVcard,
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
    NamePlace {
        name: String,
    },
    EditRelayUrl {
        url: String,
        anchor: String,
    },
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
    /// The decoy password of a full export from the real profile with a
    /// duress PIN; empty otherwise (ADR-080 point 7).
    pub decoy_password: String,
}

impl std::fmt::Debug for BackupFormSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupFormSnapshot")
            .field("restore_mode", &self.restore_mode)
            .field("restore_data_len", &self.restore_data.len())
            .field("password", &"<redacted>")
            .field("full_level", &self.full_level)
            .field("decoy_password", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for EngineOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FingerprintVerify(a) => f.debug_tuple("FingerprintVerify").field(a).finish(),
            Self::Onboarding(d) => f.debug_tuple("Onboarding").field(d).finish(),
            Self::ContactEdit {
                display_name,
                personal_note,
            } => f
                .debug_struct("ContactEdit")
                .field("display_name_changed", &display_name.is_some())
                .field("personal_note_changed", &personal_note.is_some())
                .finish(),
            Self::Backup(s) => f.debug_tuple("Backup").field(s).finish(),
            Self::Lock { .. } => f.debug_struct("Lock").field("pin", &"<redacted>").finish(),
            Self::DeviceLinkPin { .. } => f
                .debug_struct("DeviceLinkPin")
                .field("pin", &"<redacted>")
                .finish(),
            Self::ExportPin { .. } => f
                .debug_struct("ExportPin")
                .field("pin", &"<redacted>")
                .finish(),
            Self::SessionPin { .. } => f
                .debug_struct("SessionPin")
                .field("pin", &"<redacted>")
                .finish(),
            Self::AwaitingUserPresence => f.write_str("AwaitingUserPresence"),
            Self::GdprExportEncrypted { .. } => f
                .debug_struct("GdprExportEncrypted")
                .field("password", &"<redacted>")
                .finish(),
            Self::GlancePeerCode { data } => f
                .debug_struct("GlancePeerCode")
                .field("len", &data.len())
                .finish(),
            Self::ContactVisibility { toggles } => f
                .debug_struct("ContactVisibility")
                .field("toggles", toggles)
                .finish(),
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
                remove_this_device,
            } => f
                .debug_struct("DeviceManagement")
                .field("confirmed_revoke_index", confirmed_revoke_index)
                .field("remove_this_device", remove_this_device)
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
            Self::Recovery {
                old_key_input,
                voucher_input,
                restore_passphrase,
                new_passphrase,
                restore_same_identity,
            } => f
                .debug_struct("Recovery")
                .field("restore_same_identity", restore_same_identity)
                .field("old_key_input_len", &old_key_input.len())
                .field(
                    "voucher_input_len",
                    &voucher_input.as_ref().map(String::len),
                )
                .field(
                    "restore_passphrase_len",
                    &restore_passphrase.as_ref().map(String::len),
                )
                .field(
                    "new_passphrase_len",
                    &new_passphrase.as_ref().map(|(new, _)| new.len()),
                )
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
            Self::EmergencyBroadcast {
                trusted_contact_ids,
                message,
                include_location,
            } => f
                .debug_struct("EmergencyBroadcast")
                .field("trusted_contacts", &trusted_contact_ids.len())
                .field("message_chars", &message.chars().count())
                .field("include_location", include_location)
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
            Self::ContactDetail {
                is_hidden,
                wipe_their_copy,
            } => f
                .debug_struct("ContactDetail")
                .field("is_hidden", is_hidden)
                .field("wipe_their_copy", wipe_their_copy)
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
