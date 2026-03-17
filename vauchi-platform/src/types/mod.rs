// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mobile-friendly data types.
//!
//! These types are wrappers around vauchi-core types that are compatible
//! with UniFFI for cross-language bindings.

mod core_types;
mod device;
mod security;
mod social;
mod ux;

pub use core_types::{
    MobileContact, MobileContactCard, MobileContactField, MobileExchangeResult, MobileFieldType,
};
pub use device::{
    MobileDeliveryRecord, MobileDeliveryStatus, MobileDeliverySummary, MobileDeviceDeliveryRecord,
    MobileDeviceDeliveryStatus, MobileDeviceInfo, MobileDeviceJoinResult,
    MobileDeviceLinkConfirmation, MobileDeviceLinkData, MobileDeviceLinkInfo,
    MobileDeviceLinkRequest, MobileDeviceLinkResult, MobileDeviceType, MobileRetryEntry,
    MobileSyncResult, MobileSyncStatus,
};
pub use security::{
    MobileAuthMode, MobileBroadcastResult, MobileConsentRecord, MobileConsentStatus,
    MobileConsentType, MobileDecoyContact, MobileDeletionInfo, MobileDeletionState,
    MobileDuressSettings, MobileEmergencyConfig, MobileGdprExport, MobileRecoveryClaim,
    MobileRecoveryProgress, MobileRecoveryVerification, MobileRecoveryVoucher, MobileShredReport,
    MobileShredStatus, MobileShredToken, MobileShredVerification, MobileTorConfig, MobileTorStatus,
};
pub use social::{
    MobileFieldValidation, MobileSocialNetwork, MobileTrustLevel, MobileValidationStatus,
    MobileVisibilityLabel, MobileVisibilityLabelDetail,
};
pub use ux::{
    mobile_get_string, mobile_get_string_with_args, MobileAhaMoment, MobileAhaMomentType,
    MobileDemoContact, MobileDemoContactState, MobileFaqItem, MobileHelpCategory,
    MobileHelpCategoryInfo, MobileLocale, MobileLocaleInfo, MobileOnboardingProgress,
    MobileOnboardingStep, MobileTheme, MobileThemeColors, MobileThemeMode,
};
