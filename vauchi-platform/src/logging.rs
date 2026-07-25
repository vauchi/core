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
///
/// Returns a short, non-PII status string so the shell can log the
/// outcome via its *own* native logger (Logcat/os_log) — the only way to
/// observe an install failure, since a failed `try_init` leaves no
/// subscriber to carry the confirmation event. Values: `installed-*`
/// (subscriber attached), `already-installed` (idempotent no-op),
/// `try_init-failed: …` (a global subscriber was already set),
/// `no-backend` (release/store build, feature off).
#[uniffi::export]
pub fn init_mobile_logging() -> String {
    if LOG_INIT.should_install() {
        let status = install();
        // Deterministic per-launch confirmation the subscriber installed —
        // dropped if `install` failed, so its absence is itself a signal.
        // A no-op without a subscriber, so it never leaks in production.
        tracing::info!("mobile logging bridge active");
        status
    } else {
        "already-installed".to_string()
    }
}

#[cfg(all(feature = "dev-logging", any(target_os = "ios", target_os = "macos")))]
fn install() -> String {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // try_init returns Err (surfaced in the status) if a global subscriber
    // is already installed — LOG_INIT prevents that here, but a test harness
    // may also install one, so stay defensive rather than expect().
    match tracing_subscriber::registry()
        .with(tracing_oslog::OsLogger::new("app.vauchi.rust", "default"))
        .with(filter)
        .try_init()
    {
        Ok(()) => "installed-oslog".to_string(),
        Err(e) => format!("try_init-failed: {e}"),
    }
}

#[cfg(all(feature = "dev-logging", target_os = "android"))]
fn install() -> String {
    use tracing_subscriber::prelude::*;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        // Logcat stamps its own timestamp; the classic writer adds the tag.
        .without_time()
        .with_writer(logcat::MakeLogcatWriter);
    match tracing_subscriber::registry()
        .with(fmt_layer)
        .with(filter)
        .try_init()
    {
        Ok(()) => "installed-logcat-classic".to_string(),
        Err(e) => format!("try_init-failed: {e}"),
    }
}

/// Classic `__android_log_write` (API 1+) sink — works on every Android
/// version, unlike paranoid-android's structured-log API
/// (`__android_log_message`, API 30+) which silently dropped events on
/// API 26-29 devices (Galaxy S7), leaving dev-logging dark on older test
/// hardware (2026-07-25).
#[cfg(all(feature = "dev-logging", target_os = "android"))]
mod logcat {
    use std::ffi::CString;
    use std::io::Write;
    use std::os::raw::c_int;

    const ANDROID_LOG_INFO: c_int = 4; // <android/log.h>, stable ABI
    const TAG: &[u8] = b"Vauchi\0";

    /// Accumulates one formatted event and emits it whole to Logcat on
    /// drop — the fmt layer may `write` a single event across several calls,
    /// and one `__android_log_write` per event keeps lines intact.
    pub struct LogcatWriter {
        buf: Vec<u8>,
    }

    impl Write for LogcatWriter {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.buf.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Drop for LogcatWriter {
        fn drop(&mut self) {
            if self.buf.is_empty() {
                return;
            }
            let msg = String::from_utf8_lossy(&self.buf);
            // An interior NUL would truncate the C string — strip defensively.
            let Ok(cmsg) = CString::new(msg.trim_end().replace('\0', "")) else {
                return;
            };
            // SAFETY: TAG is a valid NUL-terminated static and `cmsg` is a
            // valid owned C string live for the duration of the call;
            // `__android_log_write` copies both and retains neither pointer.
            unsafe {
                ndk_sys::__android_log_write(ANDROID_LOG_INFO, TAG.as_ptr().cast(), cmsg.as_ptr());
            }
        }
    }

    pub struct MakeLogcatWriter;

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MakeLogcatWriter {
        type Writer = LogcatWriter;
        fn make_writer(&'a self) -> Self::Writer {
            LogcatWriter { buf: Vec::new() }
        }
    }
}

// Release posture (feature off) and unsupported desktop targets: no
// subscriber, so `tracing` events are dropped — see logging-rules.md.
#[cfg(not(all(
    feature = "dev-logging",
    any(target_os = "android", target_os = "ios", target_os = "macos")
)))]
fn install() -> String {
    "no-backend".to_string()
}

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
        let _ = init_mobile_logging();
        let _ = init_mobile_logging();
        let _ = init_mobile_logging();
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
        assert_eq!(init_mobile_logging(), "no-backend");
        assert_eq!(
            tracing::level_filters::LevelFilter::current(),
            tracing::level_filters::LevelFilter::OFF
        );
    }
}
