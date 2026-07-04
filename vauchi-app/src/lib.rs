// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

// Memory safety by construction (ADR-055): the app layer carries no
// unsafe. FFI crates are exempt — forbid is per-crate, not workspace-wide.
#![forbid(unsafe_code)]

//! Vauchi App Layer
//!
//! Presentation, content, and i18n modules extracted from vauchi-core.
//! Depends on vauchi-core for crypto, storage, and protocol types.

// Mirrors vauchi-core (see lib.rs there): every `let _ = fallible_call()`
// site in production code is either propagated to the caller or marked
// `#[allow(clippy::let_underscore_must_use)]` with a justification.
// The routing.rs UI-glue cluster previously discarded ~14 storage
// mutation results and unconditionally showed "success" toasts; that
// pattern is refactored into `try_mutation_with_toast` so a DB fault
// surfaces as ShowAlert instead.
// See `_private/docs/problems/2026-05-21-silent-failures-in-security-paths/`.
#![warn(clippy::let_underscore_must_use)]
// Test code routinely fires `engine.handle_action(...)` for setup and
// asserts on subsequent state, not on the per-call Result. Exempting
// test compilation from the lint keeps the production-code signal
// strong without forcing `#[allow]` on every test-setup line.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

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

pub mod relative_time;
pub use relative_time::format_relative_time;

pub mod notification_types;

pub mod notification_emitter;

#[cfg(feature = "network-rustls")]
pub mod activity_log_writer;

pub mod ui;

pub mod orchestrator;
