// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuration for remote content updates

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use vauchi_core::crypto::signing::PublicKey;

const PRODUCTION_PUBLISHER_PUBLIC_KEY: [u8; 32] = [
    149, 18, 255, 58, 48, 37, 240, 241, 30, 161, 195, 217, 252, 234, 187, 13, 89, 153, 62, 43, 189,
    135, 252, 19, 184, 228, 36, 89, 216, 200, 184, 45,
];

/// The publisher key a dev build trusts instead of the production one, given
/// at build time, so a device can take a manifest signed on a workstation
/// from a local relay (vauchi/private#733 T4.2). Off in release bindings.
#[cfg(feature = "dev-content-publisher-key")]
const DEV_PUBLISHER_PUBLIC_KEY_HEX: &str = env!("VAUCHI_DEV_CONTENT_PUBLISHER_KEY");

/// A publisher key from exactly 32 bytes of hex.
pub fn publisher_key_from_hex(hex: &str) -> Option<PublicKey> {
    let bytes: [u8; 32] = hex::decode(hex).ok()?.try_into().ok()?;
    Some(PublicKey::from_bytes(bytes))
}

#[cfg(feature = "dev-content-publisher-key")]
fn default_publisher_public_key() -> PublicKey {
    publisher_key_from_hex(DEV_PUBLISHER_PUBLIC_KEY_HEX)
        .expect("VAUCHI_DEV_CONTENT_PUBLISHER_KEY is not 32 bytes of hex")
}

#[cfg(not(feature = "dev-content-publisher-key"))]
fn default_publisher_public_key() -> PublicKey {
    PublicKey::from_bytes(PRODUCTION_PUBLISHER_PUBLIC_KEY)
}

/// Where content files come from instead of straight from the CDN: the
/// relay's OHTTP `content` action, so the server that learns which language
/// was asked for cannot tie it to an IP address (vauchi/private#733 D8).
/// The manager still checks the signed manifest, checksums and sizes.
pub trait ContentSource: Send + Sync {
    /// The bytes of the content file at `path`, such as `locales/pt.json`.
    fn fetch(&self, path: &str) -> Result<Vec<u8>, String>;
}

/// A shared [`ContentSource`] in a [`ContentConfig`].
#[derive(Clone)]
pub struct ContentSourceHandle(pub Arc<dyn ContentSource>);

impl std::fmt::Debug for ContentSourceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContentSource")
    }
}

/// Configuration for the content update system
#[derive(Debug, Clone)]
pub struct ContentConfig {
    /// Local storage path for cache
    pub storage_path: PathBuf,

    /// Remote content URL (e.g., "https://cdn.vauchi.app/v1")
    pub content_url: String,

    /// Enable/disable remote updates
    pub remote_updates_enabled: bool,

    /// Minimum interval between update checks
    pub check_interval: Duration,

    /// HTTP timeout for fetches
    pub timeout: Duration,

    /// Maximum content file size (bytes)
    pub max_content_size: u64,

    /// Proxy URL (optional SOCKS5 proxy)
    pub proxy_url: Option<String>,

    /// Publisher's Ed25519 public key for manifest signature verification.
    ///
    /// Custom content origins must explicitly replace the production key.
    pub publisher_public_key: PublicKey,

    /// The language codes to fetch and keep: English, the picked and the
    /// system language (vauchi/private#733 D7). `None` fetches every listed
    /// locale file.
    pub wanted_locales: Option<Vec<String>>,

    /// Fetch through this source instead of `content_url`; `None` fetches
    /// from the CDN directly (tools and tests).
    pub source: Option<ContentSourceHandle>,
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from("."),
            content_url: "https://cdn.vauchi.app/v1".to_string(),
            remote_updates_enabled: true,
            check_interval: Duration::from_secs(3600), // 1 hour
            timeout: Duration::from_secs(30),
            max_content_size: 5 * 1024 * 1024, // 5 MB
            proxy_url: None,
            publisher_public_key: default_publisher_public_key(),
            wanted_locales: None,
            source: None,
        }
    }
}

impl ContentConfig {
    /// Configure with default SOCKS5 proxy
    ///
    /// Uses the default SOCKS5 proxy at 127.0.0.1:9050 and
    /// increases the timeout to 60 seconds to account for proxy latency.
    pub fn with_proxy(mut self) -> Self {
        self.proxy_url = Some("socks5://127.0.0.1:9050".to_string());
        self.timeout = Duration::from_secs(60); // Longer timeout for proxy
        self
    }

    /// Configure with custom proxy URL
    pub fn with_custom_proxy(mut self, proxy_url: String) -> Self {
        self.proxy_url = Some(proxy_url);
        self
    }

    /// Disable remote updates (use bundled content only)
    pub fn without_remote_updates(mut self) -> Self {
        self.remote_updates_enabled = false;
        self
    }
}
