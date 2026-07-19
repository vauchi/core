// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rust -> native log bridge (Android Logcat / iOS+macOS os_log).
//!
//! Permanent replacement for the temporary `android_logger`/`oslog`
//! bridges used to diagnose the BLE handshake failure
//! (`2026-06-08-magic-audio-proximity-driver`, which explicitly deferred
//! this). Routes the existing `log::warn!`/`log::error!` call sites in
//! `vauchi-app` (BLE/exchange/sync failure paths) to the platform's native
//! log viewer. Info level only, per `.claude/rules/logging-rules.md` — the
//! existing call sites already log only error-type context, never
//! card/key/token content.

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
    /// platform install; every later call (app relaunch, Activity
    /// recreation) is a safe no-op.
    fn should_install(&self) -> bool {
        !self.done.swap(true, Ordering::SeqCst)
    }
}

static LOG_INIT: LogInit = LogInit::new();

/// Install the platform log backend. Safe to call on every app
/// launch/Activity recreation — only the first call takes effect.
#[uniffi::export]
pub fn init_mobile_logging() {
    if LOG_INIT.should_install() {
        install();
        // Deterministic per-launch confirmation the backend actually
        // installed — distinct from the platform-side startup banner
        // (that only proves the UniFFI layer loaded, not that this log::
        // backend registered). Its absence in the native log viewer is
        // itself the failure signal.
        log::info!("mobile logging bridge active");
    }
}

#[cfg(target_os = "android")]
fn install() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("Vauchi"),
    );
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn install() {
    // Double-init would return Err; LOG_INIT already prevents that, but
    // oslog's own init() can also be called independently by a test
    // harness, so stay defensive rather than unwrap.
    let _ = oslog::OsLogger::new("app.vauchi.rust")
        .level_filter(log::LevelFilter::Info)
        .init();
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
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
        init_mobile_logging();
        init_mobile_logging();
        init_mobile_logging();
    }

    // The shipped-binary posture: with `dev-logging` off, init must
    // register no backend, so the `log` facade's runtime max level stays
    // `Off` and the existing vauchi-app `log::` call sites are silent
    // no-ops. Guards against a native backend leaking into release builds
    // (logging-rules.md privacy boundary).
    // @internal
    #[cfg(not(feature = "dev-logging"))]
    #[test]
    fn release_build_installs_no_log_backend() {
        init_mobile_logging();
        assert_eq!(log::max_level(), log::LevelFilter::Off);
    }
}
