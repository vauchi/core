// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rust -> native `tracing` bridge (Android Logcat / iOS+macOS os_log).
//!
//! Routes `vauchi-app`'s `tracing::warn!`/`tracing::error!` call sites —
//! the canonical client logging facade per ADR-067 — to the platform's
//! native log viewer. Dev-only: the `tracing` subscriber is installed
//! solely under the `dev-logging` cargo feature; a release/store binary
//! installs none, so events are dropped (`LevelFilter::OFF`). Info level
//! by default; `RUST_LOG` overrides. Error-type context only, never
//! card/key/token content (`logging-rules.md`).

use std::sync::atomic::{AtomicBool, Ordering};

struct LogInit {
    done: AtomicBool,
}

impl LogInit {
    const fn new() -> Self {
        Self {
            done: AtomicBool::new(false),
        }
    }

    /// Returns `true` exactly once — the first caller performs the actual
    /// subscriber install; every later call (app relaunch, Activity
    /// recreation) is a safe no-op.
    fn should_install(&self) -> bool {
        !self.done.swap(true, Ordering::SeqCst)
    }
}

static LOG_INIT: LogInit = LogInit::new();

/// Install the platform `tracing` subscriber. Safe to call on every app
/// launch/Activity recreation — only the first call takes effect.
#[uniffi::export]
pub fn init_mobile_logging() {
    if LOG_INIT.should_install() {
        install();
        // Deterministic per-launch confirmation the subscriber installed —
        // its absence in the native log viewer is itself the signal that
        // this binding has no dev-logging backend (a release/store build).
        // A no-op without a subscriber, so it never leaks in production.
        tracing::info!("mobile logging bridge active");
    }
}

#[cfg(all(feature = "dev-logging", any(target_os = "ios", target_os = "macos")))]
fn install() {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // try_init returns Err (ignored) if a global subscriber is already
    // installed — LOG_INIT prevents that here, but a test harness may also
    // install one, so stay defensive rather than expect().
    let _ = tracing_subscriber::registry()
        .with(tracing_oslog::OsLogger::new("app.vauchi.rust", "default"))
        .with(filter)
        .try_init();
}

#[cfg(all(feature = "dev-logging", target_os = "android"))]
fn install() {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(paranoid_android::layer("Vauchi"))
        .with(filter)
        .try_init();
}

// Release posture (feature off) and unsupported desktop targets: no
// subscriber, so `tracing` events are dropped — see logging-rules.md.
#[cfg(not(all(
    feature = "dev-logging",
    any(target_os = "android", target_os = "ios", target_os = "macos")
)))]
fn install() {}

// INLINE_TEST_REQUIRED: LogInit and should_install() are private to this
// module (not pub) — an external tests/it integration test has no access;
// testing the idempotency invariant requires a fresh non-static instance,
// which only an inline unit test can construct.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn should_install_only_on_the_first_call() {
        let init = LogInit::new();
        assert!(init.should_install());
        assert!(!init.should_install());
        assert!(!init.should_install());
    }

    // @internal
    #[test]
    fn init_mobile_logging_does_not_panic_when_called_repeatedly() {
        // allow(zero_assertions): repeated init must not panic — there is
        // no observable state to assert beyond that.
        init_mobile_logging();
        init_mobile_logging();
        init_mobile_logging();
    }

    // Shipped-binary posture: with `dev-logging` off, init installs no
    // subscriber, so `tracing`'s global max level stays `OFF` and
    // vauchi-app's `tracing::` events are dropped. Guards a native
    // subscriber against leaking into release builds (logging-rules.md).
    // Relies on nextest process-per-test isolation for the global
    // dispatcher — do not install another subscriber elsewhere in this
    // crate's `#[cfg(test)]` tree.
    // @internal
    #[cfg(not(feature = "dev-logging"))]
    #[test]
    fn release_build_installs_no_log_backend() {
        init_mobile_logging();
        assert_eq!(
            tracing::level_filters::LevelFilter::current(),
            tracing::level_filters::LevelFilter::OFF
        );
    }
}
