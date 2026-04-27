// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! User experience types.
//!
//! Aha moments, demo contacts, onboarding, theming, localization, and help/FAQ.

use std::collections::HashMap;

/// Type of aha moment milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileAhaMomentType {
    /// Shown when card creation completes
    CardCreationComplete,
    /// Shown on first edit (before having contacts)
    FirstEdit,
    /// Shown when first contact is added
    FirstContactAdded,
    /// Shown when receiving first update from a contact
    FirstUpdateReceived,
    /// Shown when first outbound update is delivered
    FirstOutboundDelivered,
    /// Shown when user edits a field for the first time
    FirstFieldEdit,
    /// Shown when user reaches 3 contacts
    ThreeContactsReached,
    /// Shown when user links a device
    DeviceLinked,
}

impl From<vauchi_core::AhaMomentType> for MobileAhaMomentType {
    fn from(t: vauchi_core::AhaMomentType) -> Self {
        match t {
            vauchi_core::AhaMomentType::CardCreationComplete => {
                MobileAhaMomentType::CardCreationComplete
            }
            vauchi_core::AhaMomentType::FirstEdit => MobileAhaMomentType::FirstEdit,
            vauchi_core::AhaMomentType::FirstContactAdded => MobileAhaMomentType::FirstContactAdded,
            vauchi_core::AhaMomentType::FirstUpdateReceived => {
                MobileAhaMomentType::FirstUpdateReceived
            }
            vauchi_core::AhaMomentType::FirstOutboundDelivered => {
                MobileAhaMomentType::FirstOutboundDelivered
            }
            vauchi_core::AhaMomentType::FirstFieldEdit => MobileAhaMomentType::FirstFieldEdit,
            vauchi_core::AhaMomentType::ThreeContactsReached => {
                MobileAhaMomentType::ThreeContactsReached
            }
            vauchi_core::AhaMomentType::DeviceLinked => MobileAhaMomentType::DeviceLinked,
            _ => MobileAhaMomentType::CardCreationComplete,
        }
    }
}

impl From<MobileAhaMomentType> for vauchi_core::AhaMomentType {
    fn from(t: MobileAhaMomentType) -> Self {
        match t {
            MobileAhaMomentType::CardCreationComplete => {
                vauchi_core::AhaMomentType::CardCreationComplete
            }
            MobileAhaMomentType::FirstEdit => vauchi_core::AhaMomentType::FirstEdit,
            MobileAhaMomentType::FirstContactAdded => vauchi_core::AhaMomentType::FirstContactAdded,
            MobileAhaMomentType::FirstUpdateReceived => {
                vauchi_core::AhaMomentType::FirstUpdateReceived
            }
            MobileAhaMomentType::FirstOutboundDelivered => {
                vauchi_core::AhaMomentType::FirstOutboundDelivered
            }
            MobileAhaMomentType::FirstFieldEdit => vauchi_core::AhaMomentType::FirstFieldEdit,
            MobileAhaMomentType::ThreeContactsReached => {
                vauchi_core::AhaMomentType::ThreeContactsReached
            }
            MobileAhaMomentType::DeviceLinked => vauchi_core::AhaMomentType::DeviceLinked,
        }
    }
}

/// Tab metadata for top-level navigation.
///
/// Mirrors `vauchi_app::ui::TabInfo` for UniFFI consumers so frontends
/// can render tabs without hardcoding labels or icons (G1 of the
/// pure-renderer remediation; ADR-021 / ADR-038).
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileTabInfo {
    /// Stable identifier matching the screen's `screen_id()`.
    pub id: String,
    /// Localized display label resolved by core.
    pub label: String,
    /// Icon name in SF Symbol format. Frontends map to platform equivalents
    /// (Material Icons on Android, Win UI icons on Windows, etc.).
    pub icon: String,
    /// Badge count (e.g. pending contact updates). Zero means no badge.
    pub badge_count: u32,
}

impl From<vauchi_app::ui::TabInfo> for MobileTabInfo {
    fn from(value: vauchi_app::ui::TabInfo) -> Self {
        Self {
            id: value.id,
            label: value.label,
            icon: value.icon,
            badge_count: value.badge_count,
        }
    }
}

/// An aha moment to display to the user.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileAhaMoment {
    /// The type of milestone
    pub moment_type: MobileAhaMomentType,
    /// Title to display
    pub title: String,
    /// Message to display
    pub message: String,
    /// Whether to show animation
    pub has_animation: bool,
}

/// Demo contact card representation for display.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDemoContact {
    /// Contact ID
    pub id: String,
    /// Display name
    pub display_name: String,
    /// Flag indicating this is a demo
    pub is_demo: bool,
    /// Current tip title
    pub tip_title: String,
    /// Current tip content
    pub tip_content: String,
    /// Tip category
    pub tip_category: String,
}

impl From<vauchi_core::DemoContactCard> for MobileDemoContact {
    fn from(card: vauchi_core::DemoContactCard) -> Self {
        MobileDemoContact {
            id: card.id,
            display_name: card.display_name,
            is_demo: card.is_demo,
            tip_title: card.tip_title,
            tip_content: card.tip_content,
            tip_category: card.tip_category,
        }
    }
}

/// Demo contact state for persistence.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDemoContactState {
    /// Whether the demo contact is active
    pub is_active: bool,
    /// Whether it was manually dismissed
    pub was_dismissed: bool,
    /// Whether it was auto-removed after first real exchange
    pub auto_removed: bool,
    /// Number of updates sent
    pub update_count: u32,
}

/// Steps in the onboarding wizard (UniFFI-compatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum MobileOnboardingStep {
    /// Identity check gate
    IdentityCheck,
    /// Link choice (device link or backup import)
    LinkChoice,
    /// Default display name entry
    DefaultName,
    /// Groups setup: create contact groups
    GroupsSetup,
    /// Contact info fields (phone, email)
    ContactInfo,
    /// Choose what to do after onboarding
    WhatNext,
}

impl From<vauchi_core::OnboardingStep> for MobileOnboardingStep {
    fn from(step: vauchi_core::OnboardingStep) -> Self {
        match step {
            vauchi_core::OnboardingStep::IdentityCheck => MobileOnboardingStep::IdentityCheck,
            vauchi_core::OnboardingStep::LinkChoice => MobileOnboardingStep::LinkChoice,
            vauchi_core::OnboardingStep::DefaultName => MobileOnboardingStep::DefaultName,
            vauchi_core::OnboardingStep::GroupsSetup => MobileOnboardingStep::GroupsSetup,
            vauchi_core::OnboardingStep::ContactInfo => MobileOnboardingStep::ContactInfo,
            vauchi_core::OnboardingStep::WhatNext => MobileOnboardingStep::WhatNext,
            _ => MobileOnboardingStep::IdentityCheck,
        }
    }
}

impl From<MobileOnboardingStep> for vauchi_core::OnboardingStep {
    fn from(step: MobileOnboardingStep) -> Self {
        match step {
            MobileOnboardingStep::IdentityCheck => vauchi_core::OnboardingStep::IdentityCheck,
            MobileOnboardingStep::LinkChoice => vauchi_core::OnboardingStep::LinkChoice,
            MobileOnboardingStep::DefaultName => vauchi_core::OnboardingStep::DefaultName,
            MobileOnboardingStep::GroupsSetup => vauchi_core::OnboardingStep::GroupsSetup,
            MobileOnboardingStep::ContactInfo => vauchi_core::OnboardingStep::ContactInfo,
            MobileOnboardingStep::WhatNext => vauchi_core::OnboardingStep::WhatNext,
        }
    }
}

/// Onboarding progress state (UniFFI-compatible).
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileOnboardingProgress {
    /// The step the user is currently on
    pub current_step: MobileOnboardingStep,
    /// Steps that have been completed
    pub completed_steps: Vec<MobileOnboardingStep>,
    /// Timestamp when onboarding was started (Unix epoch seconds)
    pub started_at: Option<u64>,
    /// Timestamp when onboarding was completed (Unix epoch seconds)
    pub completed_at: Option<u64>,
    /// Whether the user skipped the backup step
    pub skipped_backup: bool,
    /// Completion percentage (0-100)
    pub completion_percentage: u8,
    /// Whether onboarding is complete
    pub is_complete: bool,
}

impl From<&vauchi_core::OnboardingProgress> for MobileOnboardingProgress {
    fn from(progress: &vauchi_core::OnboardingProgress) -> Self {
        MobileOnboardingProgress {
            current_step: progress.current_step.into(),
            completed_steps: progress
                .completed_steps
                .iter()
                .map(|s| (*s).into())
                .collect(),
            started_at: progress.started_at,
            completed_at: progress.completed_at,
            skipped_backup: progress.skipped_backup,
            completion_percentage: progress.completion_percentage(),
            is_complete: progress.is_complete(),
        }
    }
}

// ============================================================
// Theme Types
// ============================================================

/// Theme mode (light or dark)
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileThemeMode {
    Light,
    Dark,
}

impl From<vauchi_app::theme::ThemeMode> for MobileThemeMode {
    fn from(mode: vauchi_app::theme::ThemeMode) -> Self {
        match mode {
            vauchi_app::theme::ThemeMode::Light => MobileThemeMode::Light,
            vauchi_app::theme::ThemeMode::Dark => MobileThemeMode::Dark,
            _ => MobileThemeMode::Light,
        }
    }
}

impl From<MobileThemeMode> for vauchi_app::theme::ThemeMode {
    fn from(mode: MobileThemeMode) -> Self {
        match mode {
            MobileThemeMode::Light => vauchi_app::theme::ThemeMode::Light,
            MobileThemeMode::Dark => vauchi_app::theme::ThemeMode::Dark,
        }
    }
}

/// Theme colors for UI styling.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileThemeColors {
    /// Primary background color (hex).
    pub bg_primary: String,
    /// Secondary background color (hex).
    pub bg_secondary: String,
    /// Tertiary background color (hex).
    pub bg_tertiary: String,
    /// Primary text color (hex).
    pub text_primary: String,
    /// Secondary text color (hex).
    pub text_secondary: String,
    /// Accent color (hex).
    pub accent: String,
    /// Dark accent color (hex).
    pub accent_dark: String,
    /// Success color (hex).
    pub success: String,
    /// Error color (hex).
    pub error: String,
    /// Warning color (hex).
    pub warning: String,
    /// Border color (hex).
    pub border: String,
}

impl From<&vauchi_app::theme::ThemeColors> for MobileThemeColors {
    fn from(colors: &vauchi_app::theme::ThemeColors) -> Self {
        MobileThemeColors {
            bg_primary: colors.bg_primary.clone(),
            bg_secondary: colors.bg_secondary.clone(),
            bg_tertiary: colors.bg_tertiary.clone(),
            text_primary: colors.text_primary.clone(),
            text_secondary: colors.text_secondary.clone(),
            accent: colors.accent.clone(),
            accent_dark: colors.accent_dark.clone(),
            success: colors.success.clone(),
            error: colors.error.clone(),
            warning: colors.warning.clone(),
            border: colors.border.clone(),
        }
    }
}

/// Design tokens: spacing scale.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSpacingTokens {
    pub xs: u16,
    pub sm: u16,
    pub md: u16,
    pub lg: u16,
    pub xl: u16,
}

/// Design tokens: directional spacing.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSpacingDirectionTokens {
    pub content_start: u16,
    pub content_end: u16,
    pub list_item_start: u16,
    pub list_item_end: u16,
}

/// Design tokens: typography sizes.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileTypographyTokens {
    pub title_size: u16,
    pub subtitle_size: u16,
    pub body_size: u16,
    pub caption_size: u16,
}

/// Design tokens: border radius.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileBorderRadiusTokens {
    pub sm: u16,
    pub md: u16,
    pub md_lg: u16,
    pub lg: u16,
}

/// Design tokens: touch target sizes.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileTouchTargetTokens {
    pub minimum: u16,
}

/// Design tokens: animation durations in milliseconds.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileMotionTokens {
    pub enter_duration_ms: u16,
    pub exit_duration_ms: u16,
    pub emphasis_duration_ms: u16,
}

/// Complete design tokens for cross-platform rendering consistency.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDesignTokens {
    pub spacing: MobileSpacingTokens,
    pub spacing_direction: MobileSpacingDirectionTokens,
    pub typography: MobileTypographyTokens,
    pub border_radius: MobileBorderRadiusTokens,
    pub touch_target: MobileTouchTargetTokens,
    pub motion: MobileMotionTokens,
}

impl From<&vauchi_app::theme::DesignTokens> for MobileDesignTokens {
    fn from(t: &vauchi_app::theme::DesignTokens) -> Self {
        MobileDesignTokens {
            spacing: MobileSpacingTokens {
                xs: t.spacing.xs,
                sm: t.spacing.sm,
                md: t.spacing.md,
                lg: t.spacing.lg,
                xl: t.spacing.xl,
            },
            spacing_direction: MobileSpacingDirectionTokens {
                content_start: t.spacing_direction.content_start,
                content_end: t.spacing_direction.content_end,
                list_item_start: t.spacing_direction.list_item_start,
                list_item_end: t.spacing_direction.list_item_end,
            },
            typography: MobileTypographyTokens {
                title_size: t.typography.title_size,
                subtitle_size: t.typography.subtitle_size,
                body_size: t.typography.body_size,
                caption_size: t.typography.caption_size,
            },
            border_radius: MobileBorderRadiusTokens {
                sm: t.border_radius.sm,
                md: t.border_radius.md,
                md_lg: t.border_radius.md_lg,
                lg: t.border_radius.lg,
            },
            touch_target: MobileTouchTargetTokens {
                minimum: t.touch_target.minimum,
            },
            motion: MobileMotionTokens {
                enter_duration_ms: t.motion.enter_duration_ms,
                exit_duration_ms: t.motion.exit_duration_ms,
                emphasis_duration_ms: t.motion.emphasis_duration_ms,
            },
        }
    }
}

/// A complete theme definition.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileTheme {
    /// Theme identifier.
    pub id: String,
    /// Theme display name.
    pub name: String,
    /// Theme version.
    pub version: String,
    /// Theme author (optional).
    pub author: Option<String>,
    /// Theme license (optional).
    pub license: Option<String>,
    /// Theme source URL (optional).
    pub source: Option<String>,
    /// Theme mode (light or dark).
    pub mode: MobileThemeMode,
    /// Theme colors.
    pub colors: MobileThemeColors,
    /// Design tokens for layout consistency.
    pub tokens: MobileDesignTokens,
}

impl From<&vauchi_app::theme::Theme> for MobileTheme {
    fn from(theme: &vauchi_app::theme::Theme) -> Self {
        MobileTheme {
            id: theme.id.clone(),
            name: theme.name.clone(),
            version: theme.version.clone(),
            author: theme.author.clone(),
            license: theme.license.clone(),
            source: theme.source.clone(),
            mode: theme.mode.into(),
            colors: MobileThemeColors::from(&theme.colors),
            tokens: MobileDesignTokens::from(&theme.tokens),
        }
    }
}

// ============================================================
// i18n Types
// ============================================================

/// Form-factor lens for tab-resolution queries (§1D pure-renderer
/// remediation). Mobile collapses Settings/Recovery/Help/etc under
/// `More`; Desktop has them as first-class sidebar items. Frontends
/// pass the layout matching their nav surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileTabLayout {
    Mobile,
    Desktop,
}

impl From<MobileTabLayout> for vauchi_app::ui::TabLayout {
    fn from(layout: MobileTabLayout) -> Self {
        match layout {
            MobileTabLayout::Mobile => Self::Mobile,
            MobileTabLayout::Desktop => Self::Desktop,
        }
    }
}

/// Supported locales for the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileLocale {
    English,
    German,
    French,
    Spanish,
    Italian,
}

impl From<vauchi_app::i18n::Locale> for MobileLocale {
    fn from(locale: vauchi_app::i18n::Locale) -> Self {
        match locale {
            vauchi_app::i18n::Locale::English => MobileLocale::English,
            vauchi_app::i18n::Locale::German => MobileLocale::German,
            vauchi_app::i18n::Locale::French => MobileLocale::French,
            vauchi_app::i18n::Locale::Spanish => MobileLocale::Spanish,
            vauchi_app::i18n::Locale::Italian => MobileLocale::Italian,
            _ => MobileLocale::English,
        }
    }
}

impl From<MobileLocale> for vauchi_app::i18n::Locale {
    fn from(locale: MobileLocale) -> Self {
        match locale {
            MobileLocale::English => vauchi_app::i18n::Locale::English,
            MobileLocale::German => vauchi_app::i18n::Locale::German,
            MobileLocale::French => vauchi_app::i18n::Locale::French,
            MobileLocale::Spanish => vauchi_app::i18n::Locale::Spanish,
            MobileLocale::Italian => vauchi_app::i18n::Locale::Italian,
        }
    }
}

/// Information about a locale.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileLocaleInfo {
    /// ISO 639-1 language code.
    pub code: String,
    /// Native name of the language.
    pub name: String,
    /// English name of the language.
    pub english_name: String,
    /// Whether the language is right-to-left.
    pub is_rtl: bool,
}

impl From<vauchi_app::i18n::LocaleInfo> for MobileLocaleInfo {
    fn from(info: vauchi_app::i18n::LocaleInfo) -> Self {
        MobileLocaleInfo {
            code: info.code.to_string(),
            name: info.name.to_string(),
            english_name: info.english_name.to_string(),
            is_rtl: info.is_rtl,
        }
    }
}

// ============================================================
// Help Types
// ============================================================

/// Categories of help content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileHelpCategory {
    GettingStarted,
    Privacy,
    Recovery,
    Contacts,
    Updates,
    Features,
}

impl From<vauchi_app::help::HelpCategory> for MobileHelpCategory {
    fn from(category: vauchi_app::help::HelpCategory) -> Self {
        match category {
            vauchi_app::help::HelpCategory::GettingStarted => MobileHelpCategory::GettingStarted,
            vauchi_app::help::HelpCategory::Privacy => MobileHelpCategory::Privacy,
            vauchi_app::help::HelpCategory::Recovery => MobileHelpCategory::Recovery,
            vauchi_app::help::HelpCategory::Contacts => MobileHelpCategory::Contacts,
            vauchi_app::help::HelpCategory::Updates => MobileHelpCategory::Updates,
            vauchi_app::help::HelpCategory::Features => MobileHelpCategory::Features,
            _ => MobileHelpCategory::GettingStarted,
        }
    }
}

impl From<MobileHelpCategory> for vauchi_app::help::HelpCategory {
    fn from(category: MobileHelpCategory) -> Self {
        match category {
            MobileHelpCategory::GettingStarted => vauchi_app::help::HelpCategory::GettingStarted,
            MobileHelpCategory::Privacy => vauchi_app::help::HelpCategory::Privacy,
            MobileHelpCategory::Recovery => vauchi_app::help::HelpCategory::Recovery,
            MobileHelpCategory::Contacts => vauchi_app::help::HelpCategory::Contacts,
            MobileHelpCategory::Updates => vauchi_app::help::HelpCategory::Updates,
            MobileHelpCategory::Features => vauchi_app::help::HelpCategory::Features,
        }
    }
}

/// Help category with display name.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileHelpCategoryInfo {
    /// Category identifier.
    pub category: MobileHelpCategory,
    /// Display name for the category.
    pub display_name: String,
}

/// A frequently asked question with answer.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileFaqItem {
    /// Unique identifier.
    pub id: String,
    /// Category this FAQ belongs to.
    pub category: MobileHelpCategory,
    /// The question.
    pub question: String,
    /// The answer (may contain markdown).
    pub answer: String,
    /// Related FAQ IDs.
    pub related: Vec<String>,
}

impl From<&vauchi_app::help::FaqItem> for MobileFaqItem {
    fn from(faq: &vauchi_app::help::FaqItem) -> Self {
        MobileFaqItem {
            id: faq.id.clone(),
            category: faq.category.into(),
            question: faq.question.clone(),
            answer: faq.answer.clone(),
            related: faq.related.clone(),
        }
    }
}

// ============================================================
// i18n Helper Functions
// ============================================================

/// Get a localized string by key.
pub fn mobile_get_string(locale: MobileLocale, key: String) -> String {
    vauchi_app::i18n::get_string(locale.into(), &key)
}

/// Get a localized string with argument interpolation.
pub fn mobile_get_string_with_args(
    locale: MobileLocale,
    key: String,
    args: HashMap<String, String>,
) -> String {
    let args_vec: Vec<(&str, &str)> = args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    vauchi_app::i18n::get_string_with_args(locale.into(), &key, &args_vec)
}
