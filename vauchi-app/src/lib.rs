// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi App Layer
//!
//! Presentation, content, and i18n modules extracted from vauchi-core.
//! Depends on vauchi-core for crypto, storage, and protocol types.

pub mod i18n;
pub use i18n::{
    I18nError, Locale, LocaleInfo, get_all_strings, get_available_locales, get_locale_info,
    get_string, get_string_with_args,
};

pub mod help;
pub use help::{
    FaqItem, HelpCategory, get_faq_by_id, get_faq_by_id_localized, get_faqs, get_faqs_by_category,
    get_faqs_by_category_localized, get_faqs_localized, search_faqs, search_faqs_localized,
};

pub mod theme;
pub use theme::{
    BorderRadiusTokens, DesignTokens, SpacingTokens, Theme, ThemeColors, ThemeError, ThemeMode,
    TypographyTokens, default_theme, load_themes_from_json, validate_hex_color,
};

#[cfg(feature = "content-updates")]
pub mod content;

pub mod aha_moments;
pub use aha_moments::{aha_moment_message_localized, aha_moment_title_localized};

pub mod notification_types;

pub mod notification_emitter;

#[cfg(feature = "network-rustls")]
pub mod activity_log_writer;

pub mod ui;

pub mod orchestrator;
