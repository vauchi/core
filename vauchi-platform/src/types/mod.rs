// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mobile-friendly data types.
//!
//! These types are wrappers around vauchi-core types that are compatible
//! with UniFFI for cross-language bindings.

mod core_types;
mod device;
pub mod notification;
mod security;
mod social;
mod ux;

pub use core_types::{
    MobileAvatarOption, MobileContact, MobileContactCard, MobileContactDisplayOptions,
    MobileContactField, MobileContactTrustLevel, MobileDuplicatePair, MobileExchangeResult,
    MobileFieldNote, MobileFieldType, MobileNameOption,
};
pub use device::{
    MobileDeliveryRecord, MobileDeliveryStatus, MobileDeliverySummary, MobileDeviceDeliveryRecord,
    MobileDeviceDeliveryStatus, MobileDeviceInfo, MobileDeviceJoinResult,
    MobileDeviceLinkConfirmation, MobileDeviceLinkData, MobileDeviceLinkInfo,
    MobileDeviceLinkRequest, MobileDeviceLinkResult, MobileRetryEntry, MobileSyncIndicatorState,
    MobileSyncResult, MobileSyncStatus, MobileSyncStatusKind, MobileSyncStatusView,
    sync_status_view,
};
pub use notification::{MobileNotificationCategory, MobilePendingNotification};
pub use security::{
    MobileAuthMode, MobileBiometricUnlockOutcome, MobileBroadcastResult, MobileConsentRecord,
    MobileConsentStatus, MobileConsentType, MobileDecoyContact, MobileDeletionInfo,
    MobileDeletionState, MobileDuressSettings, MobileEmergencyConfig, MobileGdprExport,
    MobileRecoveryClaim, MobileRecoveryProgress, MobileRecoveryVerification, MobileRecoveryVoucher,
    MobileShredReport, MobileShredStatus, MobileShredToken,
};
pub use social::{
    MobileLabelContactBadge, MobileLabelContactRow, MobileLabelContactStatus, MobileSocialNetwork,
    MobileVisibilityLabel, MobileVisibilityLabelDetail,
};
pub use ux::{
    MobileAhaMoment, MobileAhaMomentType, MobileBorderRadiusTokens, MobileDemoContact,
    MobileDemoContactState, MobileDesignTokens, MobileLocale, MobileLocaleInfo, MobileMotionTokens,
    MobileOnboardingProgress, MobileOnboardingStep, MobileSpacingDirectionTokens,
    MobileSpacingTokens, MobileTabInfo, MobileTabLayout, MobileTheme, MobileThemeColors,
    MobileThemeMode, MobileTouchTargetTokens, MobileTypographyTokens, mobile_get_string,
    mobile_get_string_with_args,
};
