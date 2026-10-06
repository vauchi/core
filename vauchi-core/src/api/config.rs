// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! API Configuration
//!
//! Configuration types for the Vauchi API layer.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::crypto::SymmetricKey;
use crate::network::{
    MultiRelayConfig, PinnedCertificate, ProxyConfig, RelayClientConfig, TransportConfig,
};
/// Configuration for Vauchi instance.
///
/// Use `VauchiConfig::default()` with field overrides:
/// ```ignore
/// let config = VauchiConfig {
///     storage_path: "/custom/path".into(),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VauchiConfig {
    /// Storage directory for identity, contacts, and sync state.
    pub storage_path: PathBuf,

    /// Relay server configuration.
    pub relay: RelayConfig,

    /// Sync configuration.
    pub sync: SyncConfig,

    /// Auto-save configuration.
    pub auto_save: bool,

    /// Storage encryption key.
    /// If None, a random key will be generated (not persistent across sessions).
    pub storage_key: Option<SymmetricKey>,

    /// Whether to send delivery receipts for received messages.
    pub delivery_receipts_enabled: bool,

    /// Whether to suppress presence (online/offline status) at the relay.
    /// When true, the relay will not notify contacts of this client's online status.
    pub suppress_presence: bool,

    /// Recovery configuration for social key recovery.
    pub recovery: RecoveryConfig,

    /// Multi-relay configuration (for federation client support).
    pub relay_list: Option<MultiRelayConfig>,

    /// OHTTP privacy configuration.
    pub ohttp: OhttpConfig,

    /// Whether to send OS notification when a new contact is added via sync.
    /// Default: false (opt-in).
    pub contact_added_notifications: bool,

    /// Whether to send an OS notification when a contact updates their card —
    /// the product's core heartbeat. Default: **true** (M4 S3). Self-seeded
    /// from `SettingsFlags` so it follows the user across devices.
    pub card_update_notifications: bool,

    /// Reduce/eliminate UI motion. Category-2 accessibility flag, self-seeded
    /// from `SettingsFlags` so it follows the user across devices (ADR-047
    /// Addendum 2026-07-05). Default: false.
    pub reduce_motion: bool,

    /// Enlarge touch targets + list spacing. Category-2 accessibility flag
    /// (see `reduce_motion`). Default: false.
    pub large_touch: bool,

    /// Draw the info action on every context bar and the help icons on
    /// form fields (vauchi/private#479). Default: true; a person who knows
    /// the app turns them off once.
    pub show_help_icons: bool,

    /// New contact-card entries start Visible instead of the hidden default
    /// (2026-07-05-ungrouped-contacts-default-open, Decision 2). Self-seeded
    /// from `SettingsFlags`. Default: false.
    pub new_field_default_visible: bool,
}

impl Default for VauchiConfig {
    fn default() -> Self {
        VauchiConfig {
            storage_path: PathBuf::from("./vauchi_data"),
            relay: RelayConfig::default(),
            sync: SyncConfig::default(),
            auto_save: true,
            storage_key: None,
            delivery_receipts_enabled: true,
            suppress_presence: false,
            recovery: RecoveryConfig::default(),
            relay_list: None,
            ohttp: OhttpConfig::default(),
            contact_added_notifications: false,
            card_update_notifications: true,
            reduce_motion: false,
            large_touch: false,
            show_help_icons: true,
            new_field_default_visible: false,
        }
    }
}

impl crate::types::SettingsFlags {
    /// Overwrites the settings-toggle subset from a config, preserving every
    /// non-toggle flag (e.g. `field_centric_visibility_migrated`). Replaces
    /// the former `From<&VauchiConfig>` projection, whose whole-struct
    /// construction silently reset non-toggle state on every toggle persist.
    /// The single source of the config→flags mapping — callers (e.g. the
    /// humble `persist_settings_toggle`) load-merge-save through this.
    pub fn merge_config_toggles(&mut self, config: &VauchiConfig) {
        self.delivery_receipts_enabled = config.delivery_receipts_enabled;
        self.suppress_presence = config.suppress_presence;
        self.contact_added_notifications = config.contact_added_notifications;
        self.card_update_notifications = config.card_update_notifications;
        self.reduce_motion = config.reduce_motion;
        self.large_touch = config.large_touch;
        self.show_help_icons = config.show_help_icons;
        self.new_field_default_visible = config.new_field_default_visible;
    }
}

impl VauchiConfig {
    /// Creates a new configuration with the given storage path.
    pub fn with_storage_path(storage_path: impl Into<PathBuf>) -> Self {
        VauchiConfig {
            storage_path: storage_path.into(),
            ..Default::default()
        }
    }

    /// Sets the relay server URL. Clears any OHTTP trust anchor: it belonged
    /// to the previous relay (decision 0.9); use [`Self::with_relay`] to set
    /// the new relay's.
    pub fn with_relay_url(mut self, url: impl Into<String>) -> Self {
        self.relay.server_url = url.into();
        self.relay.ohttp_anchor = None;
        self.relay.ohttp_backup_commitment = None;
        self
    }

    /// Sets the relay server URL together with its OHTTP trust anchor. Clears
    /// any backup commitment: it belonged to the previous anchor.
    pub fn with_relay(mut self, url: impl Into<String>, ohttp_anchor: [u8; 32]) -> Self {
        self.relay.server_url = url.into();
        self.relay.ohttp_anchor = Some(ohttp_anchor);
        self.relay.ohttp_backup_commitment = None;
        self
    }

    /// Sets the commitment to the backup anchor that may replace the relay's
    /// OHTTP trust anchor (decision 0.13). Set after [`Self::with_relay`].
    pub fn with_ohttp_backup_commitment(mut self, commitment: [u8; 32]) -> Self {
        self.relay.ohttp_backup_commitment = Some(commitment);
        self
    }

    /// Explicitly sets the OHTTP-relay URL (where OHTTP traffic is sent).
    ///
    /// Only needed by self-hosters running a separate OHTTP relay; the
    /// production relay and local/e2e setups are handled by the default
    /// derivation (see [`ohttp_endpoint`]).
    pub fn with_ohttp_relay_url(mut self, url: impl Into<String>) -> Self {
        self.relay.ohttp_relay_url = Some(url.into());
        self
    }

    /// Sets the multi-relay configuration.
    pub fn with_relay_list(mut self, config: MultiRelayConfig) -> Self {
        self.relay_list = Some(config);
        self
    }

    /// Disables auto-save.
    pub fn without_auto_save(mut self) -> Self {
        self.auto_save = false;
        self
    }

    /// Sets the storage encryption key.
    /// Use this to persist data across sessions.
    pub fn with_storage_key(mut self, key: SymmetricKey) -> Self {
        self.storage_key = Some(key);
        self
    }
}

/// Relay server configuration.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Relay server URL.
    pub server_url: String,

    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,

    /// Read/write timeout in milliseconds.
    pub io_timeout_ms: u64,

    /// Maximum reconnection attempts.
    pub max_reconnect_attempts: u32,

    /// Base delay for exponential backoff (milliseconds).
    pub reconnect_base_delay_ms: u64,

    /// Maximum concurrent pending messages.
    pub max_pending_messages: usize,

    /// Acknowledgment timeout in milliseconds.
    pub ack_timeout_ms: u64,

    /// Maximum message retries before giving up.
    pub max_retries: u32,

    /// Proxy configuration (SOCKS5 proxy support).
    pub proxy: ProxyConfig,

    /// Pinned relay certificates for TLS certificate pinning.
    /// When non-empty, verifies the server's leaf certificate matches a pin.
    pub pinned_certs: Vec<PinnedCertificate>,

    /// Pinned certificates for the **OHTTP relay** host (`ohttp.vauchi.app`
    /// in production) — a distinct entity from the data relay with its own
    /// TLS key (ADR-037). Applied to the outer TLS of the OHTTP transport
    /// (`POST /v2/ohttp` and the `GET /v2/ohttp-key` bootstrap) whenever the
    /// OHTTP endpoint resolves to a different host than `server_url`.
    ///
    /// Empty = no OHTTP-host pinning: self-hosters who don't run a separate
    /// OHTTP relay (OHTTP shares `server_url`), or who pin it themselves.
    /// Must NOT reuse `pinned_certs` — the data relay and OHTTP relay have
    /// distinct keys (problem 2026-05-25-relay-ohttp-forward-hop-502: pinning
    /// the relay's SPKI against `ohttp.vauchi.app` made every sync fail).
    pub ohttp_pinned_certs: Vec<PinnedCertificate>,

    /// TTL for cached pin configurations in seconds (default 86400 = 24h).
    ///
    /// Controls how long relay-served pin updates are trusted before
    /// the client re-fetches. Shorter = more frequent checks, more
    /// network traffic. Longer = more risk of stale pins after rotation.
    pub pin_ttl_secs: u64,

    /// Ed25519 public key for verifying signed pin-config responses.
    ///
    /// When `Some`, the client fetches and verifies pin updates from
    /// the relay's `/v2/pin-config` endpoint. The relay must sign
    /// pin-config responses with the corresponding private key.
    ///
    /// When `None` (default), pin rotation is disabled — only the
    /// bundled `pinned_certs` are used. This is the safe default:
    /// unauthenticated pin updates would allow a MITM to replace
    /// the pin set permanently.
    pub pin_config_verify_key: Option<[u8; 32]>,

    /// Explicit OHTTP-relay base URL — where OHTTP traffic (`POST /v2/ohttp`
    /// and the `GET /v2/ohttp-key` bootstrap) is sent: the IP-stripping hop
    /// per ADR-037.
    ///
    /// When `None` (default), the endpoint is derived: the production relay
    /// (`relay.vauchi.app`) routes through `ohttp.vauchi.app`; any other
    /// `server_url` (self-hosted / local / e2e) uses `server_url` itself.
    /// Self-hosters running a separate OHTTP relay set this explicitly.
    /// Never point OHTTP traffic straight at the data relay — that listener
    /// does not serve `/v2/ohttp` and leaks client IP (problem
    /// 2026-05-25-relay-ohttp-forward-hop-502).
    pub ohttp_relay_url: Option<String>,

    /// Ed25519 trust anchor of `server_url`'s OHTTP gateway (#288,
    /// decisions 0.6 and 0.9): its gateway keys are accepted only through a
    /// chain signed by this key. Set together with the URL through
    /// [`VauchiConfig::with_relay`]; a new URL alone clears it.
    ///
    /// `None` (default) means Vauchi's anchor for the production relay and
    /// no anchor for any other relay — see [`Self::ohttp_trust_anchor`].
    pub ohttp_anchor: Option<[u8; 32]>,

    /// Commitment to the backup anchor that may replace `ohttp_anchor`
    /// through the relay's rollover chain (#288 decision 0.13). `None`
    /// (default) means Vauchi's for the production relay and none for any
    /// other — see [`Self::ohttp_trust_anchor`]; without one, no rollover
    /// is followed.
    pub ohttp_backup_commitment: Option<[u8; 32]>,
}

/// SPKI SHA-256 pin for relay.vauchi.app leaf certificate.
///
/// Extracted via:
/// ```sh
/// echo | openssl s_client -connect relay.vauchi.app:443 -servername relay.vauchi.app 2>/dev/null \
///   | openssl x509 -pubkey -noout | openssl pkey -pubin -outform DER | openssl dgst -sha256 -binary | xxd -p
/// ```
///
/// Update when the relay's TLS key pair rotates (not on every cert renewal —
/// SPKI pinning survives renewals that reuse the same key).
const RELAY_PROD_SPKI_PIN: [u8; 32] = [
    0xba, 0xae, 0x88, 0x27, 0xcb, 0xce, 0xf3, 0xe5, 0xa1, 0xcc, 0xe3, 0xe0, 0x00, 0x9d, 0x4e, 0x06,
    0xe1, 0x70, 0x0f, 0xb1, 0x00, 0xeb, 0x37, 0x84, 0xb8, 0xc3, 0x4f, 0x4e, 0x26, 0xb0, 0x6d, 0x00,
];

/// SPKI SHA-256 pin for the `ohttp.vauchi.app` leaf certificate — the
/// IP-stripping OHTTP relay (ADR-037), a **distinct entity** from the data
/// relay with its own TLS key pair. This is NOT the relay's key.
///
/// Extracted via (same recipe as the relay pin, against the OHTTP host):
/// ```sh
/// echo | openssl s_client -connect ohttp.vauchi.app:443 -servername ohttp.vauchi.app 2>/dev/null \
///   | openssl x509 -pubkey -noout | openssl pkey -pubin -outform DER | openssl dgst -sha256 -binary | xxd -p
/// ```
///
/// Update when `ohttp.vauchi.app`'s TLS key pair rotates (operator-managed,
/// independently of the relay's).
const OHTTP_PROD_SPKI_PIN: [u8; 32] = [
    0xc5, 0xae, 0x02, 0xab, 0xae, 0x82, 0x9b, 0xac, 0xed, 0x50, 0x7c, 0x48, 0xed, 0x46, 0x8b, 0x72,
    0x24, 0xb5, 0x3d, 0x78, 0xb7, 0x74, 0x99, 0x08, 0xfe, 0xe4, 0xbd, 0x87, 0x7b, 0x41, 0xb0, 0x2c,
];

/// SPKI SHA-256 pin of the dedicated OHTTP host (affectionate-raman-vps,
/// #30), pinned alongside the current one so `ohttp.vauchi.app` can move
/// there once enough clients carry it. The key was generated on the host by
/// `infra/ansible/playbooks/ohttp-tls-key.yml` and is not served yet, so the
/// `openssl s_client` recipe above cannot reproduce it until the cutover.
/// Drop `OHTTP_PROD_SPKI_PIN` one release after the cutover.
const OHTTP_DEDICATED_HOST_SPKI_PIN: [u8; 32] = [
    0x1b, 0x6b, 0xd4, 0x2c, 0x2d, 0x5c, 0x11, 0xac, 0x88, 0xd5, 0x5a, 0x97, 0x9a, 0xf7, 0xb9, 0x7f,
    0xe8, 0x3a, 0xa3, 0x03, 0x79, 0x55, 0x80, 0x3f, 0x08, 0x19, 0x16, 0xdf, 0xe4, 0x31, 0x6d, 0x5d,
];

impl Default for RelayConfig {
    /// Production relay configuration with SPKI certificate pinning.
    ///
    /// Self-hosters should use [`RelayConfig::unpinned`] and set their own
    /// `server_url` and optionally `pinned_certs`.
    fn default() -> Self {
        RelayConfig {
            server_url: DEFAULT_RELAY_URL.to_string(),
            connect_timeout_ms: 10_000,
            io_timeout_ms: 30_000,
            max_reconnect_attempts: 5,
            reconnect_base_delay_ms: 1_000,
            max_pending_messages: 100,
            ack_timeout_ms: 30_000,
            max_retries: 5,
            proxy: ProxyConfig::None,
            pinned_certs: vec![PinnedCertificate::new(RELAY_PROD_SPKI_PIN)],
            ohttp_pinned_certs: vec![
                PinnedCertificate::new(OHTTP_PROD_SPKI_PIN),
                PinnedCertificate::new(OHTTP_DEDICATED_HOST_SPKI_PIN),
            ],
            pin_ttl_secs: 86_400,        // 24 hours
            pin_config_verify_key: None, // disabled until relay signs pin-config
            ohttp_relay_url: None,       // derived from server_url (see ohttp_endpoint)
            ohttp_anchor: None,          // production resolves to PROD_OHTTP_ANCHOR
            ohttp_backup_commitment: None,
        }
    }
}

/// Production data relay host.
pub(crate) const PROD_RELAY_HOST: &str = "relay.vauchi.app";
/// Default data relay URL. Used both by [`RelayConfig::default`] and by the
/// startup seed guard (a persisted relay URL is applied only when config
/// still holds this default, so an explicit `with_relay_url` override wins).
pub(crate) const DEFAULT_RELAY_URL: &str = "https://relay.vauchi.app";
/// Vauchi's OHTTP trust anchor for the production relay (#288). `None` until
/// the first ceremony (`runbooks/2026-10-06-ohttp-anchor-ceremony.md` §2)
/// has created it; set it then, test first, like an SPKI pin.
pub(crate) const PROD_OHTTP_ANCHOR: Option<[u8; 32]> = None;
/// The commitment to the backup of [`PROD_OHTTP_ANCHOR`], created in the
/// same ceremony (decision 0.13) and set with it.
pub(crate) const PROD_OHTTP_BACKUP_COMMITMENT: Option<[u8; 32]> = None;

/// Production OHTTP relay (IP-stripping hop, ADR-037) the client sends
/// OHTTP traffic to when the data relay is `relay.vauchi.app`.
pub(crate) const PROD_OHTTP_RELAY_URL: &str = "https://ohttp.vauchi.app";

/// Resolve the OHTTP endpoint — the base URL for `POST /v2/ohttp` and the
/// `GET /v2/ohttp-key` bootstrap.
///
/// Precedence: an explicit `ohttp_relay_url` wins; otherwise the production
/// relay routes through [`PROD_OHTTP_RELAY_URL`] (the IP-stripping hop), and
/// any other `server_url` (self-hosted / local / e2e) is used as-is. Never
/// returns the production *data* relay — that host doesn't serve `/v2/ohttp`
/// and would leak client IP (problem 2026-05-25-relay-ohttp-forward-hop-502).
pub(crate) fn ohttp_endpoint(server_url: &str, ohttp_relay_url: Option<&str>) -> String {
    if let Some(explicit) = ohttp_relay_url {
        return explicit.to_string();
    }
    if is_production_relay(server_url) {
        return PROD_OHTTP_RELAY_URL.to_string();
    }
    server_url.to_string()
}

/// Whether `server_url`'s host is the production data relay.
///
/// Matches the host component exactly (after stripping scheme, port, path)
/// so `relay.vauchi.app.evil.com` does not pass.
pub(crate) fn is_production_relay(server_url: &str) -> bool {
    let after_scheme = server_url
        .split_once("://")
        .map_or(server_url, |(_, rest)| rest);
    let host = after_scheme
        .split(['/', ':'])
        .next()
        .unwrap_or(after_scheme);
    host == PROD_RELAY_HOST
}

impl RelayConfig {
    /// Returns the production relay certificate pins.
    ///
    /// Use this when constructing `HttpTransportConfig` outside of
    /// `RelayConfig` (e.g. CLI GDPR, TUI health check) to ensure
    /// the production relay pin is applied.
    pub fn default_pins() -> Vec<PinnedCertificate> {
        vec![PinnedCertificate::new(RELAY_PROD_SPKI_PIN)]
    }

    /// Creates a relay config with no pinned certificates.
    ///
    /// Intended for self-hosted relays where the operator controls the TLS
    /// certificate. Callers should set `server_url` to their relay address
    /// and optionally provide their own `pinned_certs`.
    pub fn unpinned(server_url: String) -> Self {
        RelayConfig {
            server_url,
            pinned_certs: Vec::new(),
            ohttp_pinned_certs: Vec::new(),
            ..Default::default()
        }
    }

    /// The anchor `server_url`'s gateway keys must chain to: the configured
    /// one, else Vauchi's for the exact production host, else none — a
    /// relay without an anchor gets no OHTTP key accepted (decision 0.9).
    pub fn ohttp_trust_anchor(&self) -> Option<[u8; 32]> {
        self.ohttp_anchor.or_else(|| {
            is_production_relay(&self.server_url)
                .then_some(PROD_OHTTP_ANCHOR)
                .flatten()
        })
    }

    /// The commitment to the backup of [`Self::ohttp_trust_anchor`],
    /// resolved the same way: none for a relay whose anchor has no backup.
    pub fn ohttp_backup_commitment(&self) -> Option<[u8; 32]> {
        if self.ohttp_anchor.is_some() {
            return self.ohttp_backup_commitment;
        }
        is_production_relay(&self.server_url)
            .then_some(PROD_OHTTP_BACKUP_COMMITMENT)
            .flatten()
    }

    /// The OHTTP endpoint this configuration sends to — the base URL for
    /// `POST /v2/ohttp` and the `GET /v2/ohttp-key` bootstrap, resolved the
    /// same way sync resolves it (see [`ohttp_endpoint`]).
    pub fn ohttp_endpoint(&self) -> String {
        ohttp_endpoint(&self.server_url, self.ohttp_relay_url.as_deref())
    }

    /// Bundled pins for [`Self::ohttp_endpoint`]: the OHTTP host's own key
    /// when it is a distinct host (ADR-037), otherwise the relay's — the
    /// choice sync makes when it bootstraps the OHTTP key.
    pub fn ohttp_endpoint_pins(&self) -> Vec<PinnedCertificate> {
        if self.ohttp_endpoint() == self.server_url {
            self.pinned_certs.clone()
        } else {
            self.ohttp_pinned_certs.clone()
        }
    }

    /// Converts to TransportConfig for the network layer.
    pub fn to_transport_config(&self) -> TransportConfig {
        TransportConfig {
            server_url: self.server_url.clone(),
            connect_timeout_ms: self.connect_timeout_ms,
            io_timeout_ms: self.io_timeout_ms,
            max_reconnect_attempts: self.max_reconnect_attempts,
            reconnect_base_delay_ms: self.reconnect_base_delay_ms,
            proxy: self.proxy.clone(),
            pinned_certs: self.pinned_certs.clone(),
        }
    }

    /// Converts to RelayClientConfig for the network layer.
    pub fn to_relay_client_config(
        &self,
        delivery_receipts_enabled: bool,
        suppress_presence: bool,
    ) -> RelayClientConfig {
        RelayClientConfig {
            transport: self.to_transport_config(),
            max_pending_messages: self.max_pending_messages,
            ack_timeout_ms: self.ack_timeout_ms,
            max_retries: self.max_retries,
            delivery_receipts_enabled,
            suppress_presence,
        }
    }
}

/// Sync configuration.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Automatically sync on contact card changes.
    pub auto_sync: bool,

    /// Sync interval in milliseconds (0 = manual only).
    pub sync_interval_ms: u64,

    /// Maximum pending updates before forcing sync.
    pub max_pending_updates: usize,

    /// Maximum updates to send per sync cycle (#64).
    /// Prevents blocking the thread when a large backlog exists
    /// (e.g., after a long offline period). Remaining updates are
    /// sent in subsequent sync cycles.
    /// `None` means no limit (send all pending).
    pub batch_size: Option<usize>,

    /// Minimum delay before first sync after exchange (milliseconds).
    /// Prevents timing correlation between exchange and first relay contact.
    pub post_exchange_delay_min_ms: u64,

    /// Maximum delay before first sync after exchange (milliseconds).
    pub post_exchange_delay_max_ms: u64,

    /// Jitter percentage applied to `sync_interval_ms` (0-50, default 15).
    /// Actual interval = `sync_interval_ms` +/- jitter%.
    pub sync_interval_jitter_percent: u32,

    /// Enable payload padding to bucket sizes (256, 512, 1024, 4096).
    /// Prevents message size analysis. Aligned with relay bucket sizes.
    pub padding_enabled: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            auto_sync: true,
            sync_interval_ms: 60_000, // 1 minute
            max_pending_updates: 50,
            batch_size: Some(20),
            post_exchange_delay_min_ms: 30_000,  // 30 seconds
            post_exchange_delay_max_ms: 300_000, // 5 minutes
            sync_interval_jitter_percent: 15,
            padding_enabled: true,
        }
    }
}

impl SyncConfig {
    /// Returns a random delay duration in the configured post-exchange range.
    ///
    /// Prevents timing correlation between an in-person exchange event
    /// and the first relay contact that follows.
    pub fn random_post_exchange_delay(&self, rng: &dyn crate::rng::SecureRng) -> Duration {
        let min = self.post_exchange_delay_min_ms;
        let max = self.post_exchange_delay_max_ms;
        if min >= max {
            return Duration::from_millis(min);
        }
        let ms = rng.random_in_range_u64(min, max);
        Duration::from_millis(ms)
    }

    /// Returns the sync interval with random jitter applied.
    ///
    /// Jitter is capped at 50% to prevent degenerate intervals.
    pub fn jittered_sync_interval(&self, rng: &dyn crate::rng::SecureRng) -> Duration {
        let base = self.sync_interval_ms;
        let pct = self.sync_interval_jitter_percent.min(50) as u64;
        if pct == 0 {
            return Duration::from_millis(base);
        }
        let delta = base * pct / 100;
        let min = base.saturating_sub(delta);
        let max = base + delta;
        let ms = rng.random_in_range_u64(min, max);
        Duration::from_millis(ms)
    }
}

/// OHTTP privacy configuration.
#[derive(Debug, Clone)]
pub struct OhttpConfig {
    /// Client-side key TTL in seconds (default 43200 = 12h).
    pub key_ttl_secs: u64,
    /// Allow direct (non-OHTTP) data requests.
    /// Only for dev/testing — production must be false.
    pub allow_direct: bool,
}

impl Default for OhttpConfig {
    fn default() -> Self {
        Self {
            key_ttl_secs: 43200,
            allow_direct: false,
        }
    }
}

/// Configuration for social key recovery.
///
/// Controls how many vouchers (trusted contacts) are needed
/// to recover an identity and whether automatic reminders are sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Number of vouchers needed to recover an identity.
    pub threshold: u32,

    /// Whether to automatically send reminders to vouchers.
    pub auto_remind: bool,

    /// Number of days between reminder messages.
    pub remind_interval_days: u32,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        RecoveryConfig {
            threshold: 3,
            auto_remind: true,
            remind_interval_days: 7,
        }
    }
}

// INLINE_TEST_REQUIRED: the SPKI pin constants and `ohttp_endpoint` are
// crate-private; external tests would need pub accessors, widening the API
// surface for nothing. The pin guards sit next to the bytes they protect.
#[cfg(test)]
mod tests {
    use super::*;

    // ── OHTTP endpoint derivation (problem 2026-05-25-relay-ohttp-forward-hop-502) ──

    // @internal
    #[test]
    fn ohttp_endpoint_routes_production_relay_through_ohttp_relay() {
        // The production data relay must NOT receive OHTTP traffic directly
        // (its main listener doesn't serve /v2/ohttp and it would leak IP);
        // route through the IP-stripping ohttp.vauchi.app hop instead.
        assert_eq!(
            ohttp_endpoint("https://relay.vauchi.app", None),
            "https://ohttp.vauchi.app",
        );
    }

    // @internal
    #[test]
    fn ohttp_endpoint_honours_explicit_override() {
        assert_eq!(
            ohttp_endpoint(
                "https://relay.vauchi.app",
                Some("https://ohttp.self.example")
            ),
            "https://ohttp.self.example",
        );
    }

    // @internal
    #[test]
    fn ohttp_endpoint_uses_server_url_for_local_and_custom() {
        // Local / e2e / self-hosted (no explicit override): OHTTP goes to the
        // same URL the caller configured — preserves existing behaviour.
        assert_eq!(
            ohttp_endpoint("http://127.0.0.1:8081", None),
            "http://127.0.0.1:8081",
        );
        assert_eq!(
            ohttp_endpoint("https://relay.self-hosted.example", None),
            "https://relay.self-hosted.example",
        );
    }

    // @internal
    #[test]
    fn is_production_relay_matches_host_exactly() {
        assert!(is_production_relay("https://relay.vauchi.app"));
        assert!(is_production_relay("wss://relay.vauchi.app"));
        assert!(is_production_relay("https://relay.vauchi.app:443/v2"));
        // Not the production data relay:
        assert!(!is_production_relay("https://ohttp.vauchi.app"));
        assert!(!is_production_relay("https://relay.vauchi.app.evil.com"));
        assert!(!is_production_relay("http://127.0.0.1:8081"));
        assert!(!is_production_relay("https://relay.self-hosted.example"));
    }

    // @internal
    #[test]
    fn default_relay_config_derives_ohttp_relay_for_production() {
        // The shipped default targets the production relay, so OHTTP must
        // derive to ohttp.vauchi.app even though the field itself is None.
        let cfg = RelayConfig::default();
        assert_eq!(cfg.server_url, "https://relay.vauchi.app");
        assert_eq!(cfg.ohttp_relay_url, None);
        assert_eq!(
            ohttp_endpoint(&cfg.server_url, cfg.ohttp_relay_url.as_deref()),
            "https://ohttp.vauchi.app",
        );
    }

    // @internal
    #[test]
    fn relay_config_exposes_the_ohttp_endpoint_sync_uses() {
        assert_eq!(
            RelayConfig::default().ohttp_endpoint(),
            "https://ohttp.vauchi.app",
            "production derives the distinct IP-stripping hop"
        );
        assert_eq!(
            RelayConfig::unpinned("https://relay.self.example".into()).ohttp_endpoint(),
            "https://relay.self.example",
            "a self-hosted relay serves OHTTP itself"
        );
        let mut explicit = RelayConfig::unpinned("https://relay.self.example".into());
        explicit.ohttp_relay_url = Some("https://ohttp.self.example".into());
        assert_eq!(explicit.ohttp_endpoint(), "https://ohttp.self.example");
    }

    // @internal
    #[test]
    fn ohttp_endpoint_pins_follow_the_host_the_endpoint_resolves_to() {
        let production = RelayConfig::default();
        assert_eq!(
            production.ohttp_endpoint_pins(),
            production.ohttp_pinned_certs,
            "a distinct OHTTP host is pinned by its own key"
        );

        let same_host = RelayConfig {
            server_url: "https://relay.self.example".into(),
            ..Default::default()
        };
        assert_eq!(
            same_host.ohttp_endpoint_pins(),
            same_host.pinned_certs,
            "OHTTP on the relay's own host uses the relay pins"
        );

        assert!(
            RelayConfig::unpinned("https://relay.self.example".into())
                .ohttp_endpoint_pins()
                .is_empty()
        );
    }

    /// #30 cutover: clients must trust the dedicated OHTTP host's key before
    /// `ohttp.vauchi.app` moves to it. The pin is the SPKI the
    /// `ohttp-tls-key.yml` play printed on affectionate-raman-vps (2026-10-06).
    // @scenario: pinning :: production default pins the OHTTP host distinctly
    #[test]
    fn default_relay_config_pins_the_dedicated_ohttp_host_before_cutover() {
        assert_eq!(
            hex::encode(OHTTP_DEDICATED_HOST_SPKI_PIN),
            "1b6bd42c2d5c11ac88d55a979af7b97fe83aa3037955803f081916dfe4316d5d"
        );
        assert_ne!(OHTTP_DEDICATED_HOST_SPKI_PIN, OHTTP_PROD_SPKI_PIN);
        assert_ne!(OHTTP_DEDICATED_HOST_SPKI_PIN, RELAY_PROD_SPKI_PIN);
        assert!(
            !RelayConfig::default()
                .pinned_certs
                .contains(&PinnedCertificate::new(OHTTP_DEDICATED_HOST_SPKI_PIN)),
            "the OHTTP host's key must never pin the data relay"
        );
    }

    // @scenario: pinning :: production default pins the OHTTP host distinctly
    #[test]
    fn default_relay_config_pins_ohttp_host_with_distinct_key() {
        // The OHTTP relay (ohttp.vauchi.app) is a distinct entity with its
        // own TLS key (ADR-037). The default must pin THAT key — not the
        // data relay's — or every production sync fails pin verification
        // (problem 2026-05-25-relay-ohttp-forward-hop-502).
        let cfg = RelayConfig::default();
        assert_eq!(
            cfg.ohttp_pinned_certs,
            vec![
                PinnedCertificate::new(OHTTP_PROD_SPKI_PIN),
                PinnedCertificate::new(OHTTP_DEDICATED_HOST_SPKI_PIN),
            ],
            "default config must pin the OHTTP-relay host's SPKI, current and dedicated",
        );
        // A copy-paste of the relay pin would silently reintroduce the bug.
        assert_ne!(
            OHTTP_PROD_SPKI_PIN, RELAY_PROD_SPKI_PIN,
            "OHTTP-host pin must differ from the data-relay pin",
        );
        assert_ne!(
            cfg.ohttp_pinned_certs, cfg.pinned_certs,
            "OHTTP-host pin set must differ from the data-relay pin set",
        );
    }

    // @internal
    #[test]
    fn unpinned_relay_config_has_no_ohttp_host_pins() {
        // Self-hosters opt out of vauchi's bundled pins entirely.
        let cfg = RelayConfig::unpinned("https://relay.self.example".into());
        assert!(cfg.ohttp_pinned_certs.is_empty());
        assert!(cfg.pinned_certs.is_empty());
    }

    // @internal
    #[test]
    fn with_ohttp_relay_url_sets_explicit_override() {
        let cfg = VauchiConfig::default().with_ohttp_relay_url("https://ohttp.self.example");
        assert_eq!(
            cfg.relay.ohttp_relay_url.as_deref(),
            Some("https://ohttp.self.example"),
        );
    }
}
