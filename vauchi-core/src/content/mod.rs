// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Remote content updates module
//!
//! Provides functionality for fetching, caching, and managing remotely
//! updatable content such as:
//! - Social network definitions
//! - Localization strings
//! - Help content
//! - Themes
//!
//! Content is verified using SHA-256 checksums and cached locally.
//! Bundled content serves as fallback when remote content is unavailable.

mod cache;
mod config;
mod fetcher;
mod integrity;
mod manager;
mod types;

pub use cache::{CacheError, ContentCache};
pub use config::ContentConfig;
pub use fetcher::{ContentFetcher, FetchError};
pub use integrity::{compute_checksum, verify_checksum, IntegrityError};
pub use manager::{ApplyResult, ContentError, ContentManager, LocaleStrings, NetworkEntry};
pub use types::{
    ContentEntry, ContentIndex, ContentManifest, ContentType, FileEntry, LocalesEntry, UpdateStatus,
};
