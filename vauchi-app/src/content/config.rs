// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuration for remote content updates

use std::path::PathBuf;
use std::time::Duration;
use vauchi_core::crypto::signing::PublicKey;

const PRODUCTION_PUBLISHER_PUBLIC_KEY: [u8; 32] = [
    149, 18, 255, 58, 48, 37, 240, 241, 30, 161, 195, 217, 252, 234, 187, 13, 89, 153, 62, 43, 189,
    135, 252, 19, 184, 228, 36, 89, 216, 200, 184, 45,
];

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
            publisher_public_key: PublicKey::from_bytes(PRODUCTION_PUBLISHER_PUBLIC_KEY),
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
