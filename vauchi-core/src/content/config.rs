//! Configuration for remote content updates

use std::path::PathBuf;
use std::time::Duration;

/// Configuration for the content update system
#[derive(Debug, Clone)]
pub struct ContentConfig {
    /// Local storage path for cache
    pub storage_path: PathBuf,

    /// Remote content URL (e.g., "https://vauchi.app/app-files")
    pub content_url: String,

    /// Enable/disable remote updates
    pub remote_updates_enabled: bool,

    /// Minimum interval between update checks
    pub check_interval: Duration,

    /// HTTP timeout for fetches
    pub timeout: Duration,

    /// Maximum content file size (bytes)
    pub max_content_size: u64,

    /// Proxy URL (for Tor support)
    pub proxy_url: Option<String>,
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from("."),
            content_url: "https://vauchi.app/app-files".to_string(),
            remote_updates_enabled: true,
            check_interval: Duration::from_secs(3600), // 1 hour
            timeout: Duration::from_secs(30),
            max_content_size: 5 * 1024 * 1024, // 5 MB
            proxy_url: None,
        }
    }
}

impl ContentConfig {
    /// Configure with Tor proxy
    ///
    /// Uses the default Tor SOCKS5 proxy at 127.0.0.1:9050 and
    /// increases the timeout to 60 seconds to account for Tor latency.
    pub fn with_tor(mut self) -> Self {
        self.proxy_url = Some("socks5://127.0.0.1:9050".to_string());
        self.timeout = Duration::from_secs(60); // Longer timeout for Tor
        self
    }

    /// Configure with custom proxy
    pub fn with_proxy(mut self, proxy_url: String) -> Self {
        self.proxy_url = Some(proxy_url);
        self
    }

    /// Disable remote updates (use bundled content only)
    pub fn without_remote_updates(mut self) -> Self {
        self.remote_updates_enabled = false;
        self
    }
}
